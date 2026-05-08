//! G4 (Architecture) parallel-query orchestrator + edge-union + BFS
//! blast-radius merger — feature `P3-W9-F09`, master-plan §5.4 lines
//! 483-500.
//!
//! # Pipeline shape
//!
//! Master-plan §5.4 prescribes the G4 (Architecture) fan-out as
//! "Query → ALL of {`CodeGraphContext`, LSP call hierarchy,
//! dependency-cruiser, deptry, …} run in parallel" so the host adapter
//! sees one fused dependency surface even when one architecture
//! source stalls.  Per-source outputs are then unioned by
//! `(source, target, edge_kind)` and a bidirectional BFS spreads the
//! `query.changed_nodes` outward — depth-capped at
//! [`G4Query::max_blast_depth`], coupling-weighted multiplicatively
//! per master-plan §5.4 line 495 ("BFS from changed nodes, weight by
//! coupling strength").
//!
//! [`execute_g4`] mirrors `executor::execute_g1` (WO-0047) and
//! [`crate::g3::execute_g3`] (WO-0070):
//!
//! 1. Each [`G4Source::execute`] runs under a per-source
//!    `tokio::time::timeout` parameterised by [`G4_PER_SOURCE_DEADLINE`].
//! 2. The whole fan-out runs under
//!    `tokio::time::timeout(master_deadline, ...)`.
//! 3. On master-deadline trip, [`G4SourceStatus::TimedOut`] placeholders
//!    are synthesised in input order so `results[i].source_id` matches
//!    `sources[i].source_id()` either way.
//!
//! # Merge contract
//!
//! [`merge_g4_dependency_union`] groups every [`G4DependencyEdge`] from
//! `Available` sources by `(source, target, edge_kind)`.  When two or
//! more sources contribute the same edge, the deduped output carries
//! every contributing `source_id` (lexicographically sorted) and the
//! maximum `coupling_weight` (highest-confidence-wins, the §5.4
//! adaptation of the §5.3-style invariant).  `Errored` and `TimedOut`
//! sources contribute zero edges to the merge.
//!
//! After dedup, the merger caps `unified_edges` at
//! [`G4Query::max_edges`] (top-N by `coupling_weight`) and runs a
//! depth-capped bidirectional BFS from every `changed_nodes` seed.
//! Each visited node carries (a) its BFS depth, (b) the multiplicative
//! product of `coupling_weight` along the BFS path (so weak-coupling
//! chains decay quickly per master-plan §5.4 line 495), and (c) the
//! `(source, target)` of the edge that brought the node into the
//! blast radius.  Seed nodes have depth `0`, cumulative coupling
//! `1.0`, and an empty `contributing_edges` list.
//!
//! # Ground-truth-on-conflict sentinel (master-plan §5.4 line 494)
//!
//! Master-plan §5.4 line 494 ("parse actual imports as ground truth,
//! merge inferred relations") prescribes a future hook: when two
//! sources contribute the same `(source, target, edge_kind)` tuple but
//! with different [`G4EdgeOrigin`] (one `Inferred`, one
//! `GroundTruth`), the resolver should upgrade the `GroundTruth`
//! origin over the `Inferred` origin.  F09 ships only `Inferred`
//! sources (`CodeGraphContext` + LSP call hierarchy), so this branch is
//! NOT yet load-bearing — see the `TODO(P3-W10-F14 / future)`
//! sentinel inside [`merge_g4_dependency_union`] for the pending hook.
//! The matching shape is WO-0070's `validity_window` deferral on
//! [`crate::g3::G3FactObservation`].
//!
//! # No-substitute-impls policy
//!
//! Per master-plan §15.4 + CLAUDE.md "no substitute impls of critical
//! deps", this module — its public traits, types, and orchestrator —
//! does NOT contain placeholder implementations of MCP servers,
//! JSON-RPC transports, or `tokio::process::Command` subprocess
//! runners.  The module ships the trait + orchestrator + merger only;
//! production [`G4Source`] impls (e.g. `CodeGraphContextG4Source`
//! calling the F08 plugin via `PluginManager::health_check_with_timeout`,
//! `LSPCallHierarchyG4Source` calling the P1-W5-F06 LSP bridge) are
//! deferred to a follow-up production-wiring WO that bundles G4 into
//! the cross-group executor.  The frozen acceptance test
//! [`crate::executor::test_g4_architecture_query`] supplies UCIL-
//! internal `G4Source` impls (`DEC-0008` §4 dependency-inversion seam)
//! under `#[cfg(test)]`.

#![allow(clippy::module_name_repetitions)]

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::task::Poll;
use std::time::Duration;

use serde::{Deserialize, Serialize};

// ── Constants ─────────────────────────────────────────────────────────────

/// Master deadline for the G4 (Architecture) parallel-execution
/// orchestrator.
///
/// Master-plan §5.4 + §6.1 line 606 prescribe a 5 s overall deadline
/// for any group fan-out so the daemon can return partial results to
/// the host adapter when one architecture source stalls.  When this
/// deadline elapses, [`execute_g4`] returns a [`G4Outcome`] with
/// `master_timed_out = true` and per-source [`G4SourceStatus::TimedOut`]
/// placeholders for any source that had not yet completed.
pub const G4_MASTER_DEADLINE: Duration = Duration::from_millis(5_000);

/// Per-source deadline applied to each [`G4Source::execute`] call.
///
/// **Held as an unconditional `const`, NOT `min`'d with the caller-
/// supplied `master_deadline`.**  WO-0068 + WO-0070 lessons-learned
/// (For executor #2 / For planner #3) demonstrated that capping per-
/// source by master collapses both timeouts on tight masters and the
/// inner per-source wins, hiding the master trip.  The 4.5 s / 5 s
/// margin keeps per-source as the primary path under default config;
/// tight-master cases (e.g. 100 ms test masters) let the master fire
/// first deterministically.
///
/// * Per-source wins only on a true global stall (sleeper > 4.5 s).
/// * Master wins only on tight budgets (master < 4.5 s).
pub const G4_PER_SOURCE_DEADLINE: Duration = Duration::from_millis(4_500);

// ── Public types ──────────────────────────────────────────────────────────

/// G4 (Architecture) query input — the seed-node set whose blast
/// radius the caller wants to compute.
///
/// Live wiring will derive `changed_nodes` from a host adapter's
/// `get_architecture` / `trace_dependencies` / `blast_radius` request
/// through the master-plan §6.1 query-pipeline classifier; the unit
/// test constructs them directly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct G4Query {
    /// Seed symbols whose blast radius the caller wants computed.
    /// An empty `changed_nodes` vector yields an empty
    /// [`G4UnionOutcome::blast_radius`] but still emits the unioned
    /// edge list.
    pub changed_nodes: Vec<String>,
    /// Maximum BFS depth (inclusive) — only nodes whose BFS distance
    /// to a seed in `changed_nodes` is `<= max_blast_depth` appear in
    /// [`G4UnionOutcome::blast_radius`].
    pub max_blast_depth: u32,
    /// Maximum number of unique edges emitted in
    /// [`G4UnionOutcome::unified_edges`] — when the unioned set
    /// exceeds this cap, the merger keeps the top-N by
    /// [`G4DependencyEdge::coupling_weight`] (ties broken by
    /// lexicographic `(source, target)`) and the BFS uses only the
    /// surviving edges.
    pub max_edges: usize,
}

/// Kind of dependency edge — master-plan §5.4 line 488.
///
/// The four canonical kinds are "Import, call, inheritance,
/// implementation"; `Other(String)` is the extension point for any
/// architecture source that emits a non-standard edge kind without
/// forcing a schema bump.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum G4EdgeKind {
    /// `import x from y` style edges (Python `import`, JS `import`,
    /// Rust `use`, …).
    Import,
    /// Function-/method-call edges (`a()` invokes `b()`).
    Call,
    /// Class-/trait-inheritance edges (`class A extends B`,
    /// Rust `trait A: B`, …).
    Inherits,
    /// Trait-/interface-implementation edges (`impl Trait for Ty`,
    /// `class A implements I`, …).
    Implements,
    /// Source-defined extension kind — e.g. `Other("uses")` or
    /// `Other("references")` from a source whose vocabulary does not
    /// fit the four standard kinds.
    Other(String),
}

/// Provenance of a dependency edge.
///
/// `Inferred` covers edges derived from analysis
/// (`CodeGraphContext`, LSP); `GroundTruth` covers edges parsed
/// directly from the source AST (dependency-cruiser, deptry,
/// dependency-aware AST parsers).
///
/// Master-plan §5.4 line 494 ("parse actual imports as ground truth,
/// merge inferred relations") prescribes `GroundTruth` edges
/// over-ride `Inferred` edges on conflict.  F09 ships only `Inferred`
/// sources, so the resolution branch is NOT yet load-bearing — see
/// the `TODO(P3-W10-F14 / future)` sentinel inside
/// [`merge_g4_dependency_union`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum G4EdgeOrigin {
    /// Edge derived from analysis (`CodeGraphContext`, LSP call
    /// hierarchy, …) — the default origin in F09.
    Inferred,
    /// Edge parsed from the source AST as ground truth
    /// (dependency-cruiser, deptry, …) — reserved for the future
    /// hook.
    GroundTruth,
}

/// One dependency edge emitted by an architecture source.
///
/// The merger groups by `(source, target, edge_kind)` and resolves
/// duplicates by max `coupling_weight` (highest-confidence-wins),
/// recording every contributing `source_id` in the deduped
/// [`G4UnifiedEdge::contributing_source_ids`].  The `origin` field
/// is reserved for the future ground-truth-on-conflict resolver
/// branch (master-plan §5.4 line 494) — see [`G4EdgeOrigin`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct G4DependencyEdge {
    /// Symbol the edge originates from (e.g. `TaskManager`).
    pub source: String,
    /// Symbol the edge targets (e.g. `Database`).
    pub target: String,
    /// Kind of dependency — see [`G4EdgeKind`].
    pub edge_kind: G4EdgeKind,
    /// Identifier of the [`G4Source`] that emitted this edge.  Maps
    /// 1-to-1 to [`G4SourceOutput::source_id`] / [`G4Source::source_id`].
    pub source_id: String,
    /// Provenance — see [`G4EdgeOrigin`].  F09 ships only `Inferred`;
    /// `GroundTruth` lights up when dependency-cruiser / deptry plug
    /// in (P3-W10-F14).
    pub origin: G4EdgeOrigin,
    /// Source-defined coupling strength in `[0.0, 1.0]`.  Used by the
    /// merger to break ties on the dedup branch (highest wins) and to
    /// weight the BFS multiplicatively per master-plan §5.4 line 495.
    pub coupling_weight: f64,
}

/// Disposition of one G4 source on a given fan-out call.
///
/// Each variant is a discriminant with no inner data — the data lives
/// on [`G4SourceOutput`] via `edges` / `error` / `elapsed_ms`.
/// Master-plan §5.4 + §6.1 prescribes per-source dispositions so
/// partial outcomes remain usable: a single
/// [`G4SourceStatus::Errored`] does not turn the entire fan-out into
/// a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum G4SourceStatus {
    /// The source returned its edges within the per-source deadline.
    Available,
    /// The source's per-source `tokio::time::timeout` elapsed before
    /// it returned a response.
    TimedOut,
    /// The source returned an error (transport / decode / internal).
    Errored,
}

/// One source's contribution to a G4 fan-out outcome.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct G4SourceOutput {
    /// Identifier of the source that emitted this output.
    pub source_id: String,
    /// Disposition of the source on this fan-out call.
    pub status: G4SourceStatus,
    /// Wall-clock time the source spent before returning, in
    /// milliseconds.
    pub elapsed_ms: u64,
    /// Source-emitted dependency edges.  Empty when `status` is
    /// `TimedOut` or `Errored`; the merger ignores those statuses.
    pub edges: Vec<G4DependencyEdge>,
    /// Operator-readable error description for any non-`Available`
    /// status.  `None` for [`G4SourceStatus::Available`].
    pub error: Option<String>,
}

/// Aggregate outcome of one [`execute_g4`] fan-out call.
///
/// `results` is a `Vec` whose order matches the input `sources`
/// argument so callers can correlate by index.  `master_timed_out` is
/// `true` when the outer master deadline elapsed before all per-source
/// futures completed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct G4Outcome {
    /// Per-source outputs, in the same order as the input `sources`.
    pub results: Vec<G4SourceOutput>,
    /// Wall-clock time the orchestrator spent, in milliseconds.
    pub wall_elapsed_ms: u64,
    /// `true` iff the outer master deadline elapsed before all
    /// per-source futures completed.
    pub master_timed_out: bool,
}

/// One unioned dependency edge emitted by [`merge_g4_dependency_union`].
///
/// The `edge` field carries the winning [`G4DependencyEdge`] (max
/// `coupling_weight` across contributing sources, structurally keyed
/// by `(source, target, edge_kind)`).  `contributing_source_ids`
/// lists every [`G4Source::source_id`] that emitted this edge,
/// lexicographically sorted for deterministic output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct G4UnifiedEdge {
    /// The deduped edge — see [`G4DependencyEdge`].
    pub edge: G4DependencyEdge,
    /// Source ids that emitted this edge, lexicographically sorted.
    pub contributing_source_ids: Vec<String>,
}

/// One node in the BFS blast radius — see
/// [`merge_g4_dependency_union`].
///
/// `depth` is the BFS distance from the closest seed in
/// [`G4Query::changed_nodes`] (so seeds carry `depth = 0`).
/// `cumulative_coupling` is the multiplicative product of
/// `coupling_weight` along the BFS path (seeds carry `1.0`); the
/// multiplicative shape lets weak-coupling chains decay quickly per
/// master-plan §5.4 line 495.  `contributing_edges` lists the
/// `(source, target)` of the edge that brought this node into the
/// blast radius (empty for seed nodes).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct G4BlastRadiusEntry {
    /// Symbol that landed in the blast radius.
    pub node: String,
    /// BFS distance from the closest seed in
    /// [`G4Query::changed_nodes`].
    pub depth: u32,
    /// Multiplicative product of `coupling_weight` along the BFS
    /// path (seeds carry `1.0`).
    pub cumulative_coupling: f64,
    /// `(source, target)` of the edge that brought this node into
    /// the radius — empty for seed nodes.
    pub contributing_edges: Vec<(String, String)>,
}

/// Aggregate outcome of one [`merge_g4_dependency_union`] call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct G4UnionOutcome {
    /// One [`G4UnifiedEdge`] per distinct `(source, target,
    /// edge_kind)` tuple observed across every `Available` source,
    /// optionally truncated to [`G4Query::max_edges`].  Sorted by
    /// `(source, target)` for deterministic output.
    pub unified_edges: Vec<G4UnifiedEdge>,
    /// One [`G4BlastRadiusEntry`] per node reachable from any seed
    /// in [`G4Query::changed_nodes`] within
    /// [`G4Query::max_blast_depth`] hops, sorted by `(depth, node)`
    /// for deterministic output.
    pub blast_radius: Vec<G4BlastRadiusEntry>,
    /// Sum of `edges.len()` across every `Available` source —
    /// pre-dedup count.
    pub total_edges_in: usize,
    /// `unified_edges.len()` — surfaced separately so consumers do
    /// not need to recompute it.
    pub total_unique_edges_out: usize,
    /// Number of distinct `source_id`s that contributed at least one
    /// edge that survived the dedup + truncation.
    pub sources_contributing: usize,
}

// ── Trait + helpers ───────────────────────────────────────────────────────

/// Dependency-inversion seam for one G4 (Architecture) source.
///
/// Per `DEC-0008` §4 this trait is UCIL-owned — it is **not** a
/// re-export or adapter of any external wire format.  The frozen
/// acceptance test [`crate::executor::test_g4_architecture_query`]
/// supplies local trait impls of [`G4Source`] (UCIL's own abstraction
/// boundary); production wiring of real subprocess clients (e.g.
/// `CodeGraphContextG4Source` calling the F08 plugin via
/// `tokio::process::Command`, `LSPCallHierarchyG4Source` calling the
/// P1-W5-F06 LSP bridge) is deferred to a follow-up production-
/// wiring WO.
///
/// Same shape as the WO-0047 [`crate::executor::G1Source`] trait, the
/// WO-0068 [`crate::g2_search::G2SourceProvider`] trait, and the
/// WO-0070 [`crate::g3::G3Source`] trait.  `Send + Sync` bounds are
/// required so trait objects can live in
/// `Vec<Box<dyn G4Source + Send + Sync + 'static>>` inside the
/// daemon's long-lived server state once the production-wiring WO
/// lands.
#[async_trait::async_trait]
pub trait G4Source: Send + Sync {
    /// Identifies this source without runtime introspection so
    /// [`execute_g4`] can label results by source.  The returned
    /// string is expected to be stable across the source's lifetime
    /// and unique within one [`execute_g4`] call.
    fn source_id(&self) -> &str;

    /// Run this source's architecture query.
    ///
    /// Implementations are responsible for emitting their own
    /// [`G4SourceOutput`] with the appropriate
    /// [`G4SourceStatus`] — the orchestrator only overrides the status
    /// to [`G4SourceStatus::TimedOut`] when its per-source
    /// `tokio::time::timeout` elapses.
    async fn execute(&self, query: &G4Query) -> G4SourceOutput;
}

/// Run one source under [`G4_PER_SOURCE_DEADLINE`], converting a
/// per-source timeout into a [`G4SourceStatus::TimedOut`]
/// [`G4SourceOutput`] without ever panicking.
///
/// The helper keeps [`execute_g4`] focused on the fan-out shape —
/// per-source timeout handling lives here so the orchestrator does not
/// need a `match` arm per disposition.  Mirrors `run_g1_source` in
/// `executor.rs` and `run_g3_source` in `g3.rs` adapted to the
/// [`G4SourceOutput`] shape.
async fn run_g4_source(
    source: &(dyn G4Source + Send + Sync),
    query: &G4Query,
    per_source_deadline: Duration,
) -> G4SourceOutput {
    let source_id = source.source_id().to_owned();
    let start = std::time::Instant::now();
    tokio::time::timeout(per_source_deadline, source.execute(query))
        .await
        .unwrap_or_else(|_| {
            let elapsed_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
            G4SourceOutput {
                source_id,
                status: G4SourceStatus::TimedOut,
                elapsed_ms,
                edges: vec![],
                error: Some(format!(
                    "per-source deadline {} ms exceeded",
                    per_source_deadline.as_millis()
                )),
            }
        })
}

/// Poll a `Vec` of pinned-boxed futures concurrently and collect every
/// output once all are ready.
///
/// Behaviourally equivalent to `futures::future::join_all` but uses
/// the same `poll_fn` fan-out shape as `executor::join_all_g1` and
/// `g3::join_all_g3` — `tokio` ships everything we need for a
/// 3-to-N-way fan-out without introducing an additional poll-set
/// abstraction.
///
/// `#[allow(dead_code)]` covers the verifier's M1 mutation contract:
/// the M1 mutation swaps [`execute_g4`]'s parallel join site for a
/// sequential `for fut in futures { … .await }` loop, leaving this
/// helper unreferenced.  Without the allow, `#![deny(warnings)]` (set
/// at the crate root in `lib.rs`) would convert the dead-code lint
/// into a compile error and the verifier would observe a compile
/// failure instead of the SA1 panic the contract expects.
#[allow(dead_code)]
async fn join_all_g4<'a, T>(
    mut futures: Vec<Pin<Box<dyn Future<Output = T> + Send + 'a>>>,
) -> Vec<T>
where
    T: 'a,
{
    let len = futures.len();
    let mut slots: Vec<Option<T>> = (0..len).map(|_| None).collect();
    std::future::poll_fn(|cx| {
        let mut any_pending = false;
        for (i, fut) in futures.iter_mut().enumerate() {
            if slots[i].is_some() {
                continue;
            }
            match fut.as_mut().poll(cx) {
                Poll::Ready(out) => {
                    slots[i] = Some(out);
                }
                Poll::Pending => {
                    any_pending = true;
                }
            }
        }
        if any_pending {
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    })
    .await;
    slots
        .into_iter()
        .map(|r| r.expect("join_all_g4: every slot must be filled before returning"))
        .collect()
}

// ── Orchestrator ──────────────────────────────────────────────────────────

/// G4 (Architecture) parallel-execution orchestrator.
///
/// Master-plan §5.4 lines 483-500 prescribes the fan-out shape:
/// `Query → ALL of {CodeGraphContext, LSP call hierarchy,
/// dependency-cruiser, deptry, …} run in parallel`, with a 5 s
/// overall deadline so partial outcomes stay usable when one
/// architecture source stalls.
///
/// Implementation:
///
/// 1. The per-source deadline is held at [`G4_PER_SOURCE_DEADLINE`]
///    **unconditionally** — it is NOT `min`'d with `master_deadline`.
///    Per WO-0068 + WO-0070 lessons-learned (For executor #2 / For
///    planner #3), capping per-source by master collapses both
///    timeouts on tight masters and the inner per-source wins,
///    hiding the master trip.  The 4.5 s / 5 s margin keeps
///    per-source as the primary path under default config (master =
///    5 s); tight-master cases (e.g. 100 ms test masters) let the
///    master fire first deterministically.
/// 2. Each per-source future is wrapped in
///    `tokio::time::timeout(G4_PER_SOURCE_DEADLINE, ...)` via
///    [`run_g4_source`] which returns
///    [`G4SourceStatus::TimedOut`] on elapse.
/// 3. Build one boxed future per source and poll them concurrently
///    through [`join_all_g4`] (the same `poll_fn` fan-out shape as
///    `execute_g1` / `execute_g3`).
/// 4. Wrap the whole join in
///    `tokio::time::timeout(master_deadline, ...)`.  On
///    `Err(Elapsed)`, return a [`G4Outcome`] with
///    [`G4SourceStatus::TimedOut`] placeholders for every source and
///    `master_timed_out = true` so downstream code never sees an
///    empty result vector when the master deadline fires.
///
/// The orchestrator never `panic!`s and never `?` propagates an error
/// out — partial results are valid output per master-plan §5.4 +
/// §6.1 line 606.
///
/// Per master-plan §15.2, this orchestrator emits a `tracing` span
/// `ucil.group.architecture` (parallel to `ucil.group.knowledge` for
/// G3 and `ucil.group.structural` for G1).  The instrument decorator
/// is appropriate here because `execute_g4` is async/IO orchestration
/// — unlike the deterministic `ceqp::parse_reason` (WO-0067) which
/// intentionally has no span.
///
/// # Examples
///
/// ```no_run
/// use std::time::Duration;
/// use ucil_daemon::g4::{
///     execute_g4, G4Query, G4Source, G4_MASTER_DEADLINE,
/// };
///
/// # async fn demo(sources: Vec<Box<dyn G4Source + Send + Sync + 'static>>) {
/// let q = G4Query {
///     changed_nodes: vec!["TaskManager".to_owned()],
///     max_blast_depth: 3,
///     max_edges: 256,
/// };
/// let outcome = execute_g4(q, sources, G4_MASTER_DEADLINE).await;
/// assert!(!outcome.master_timed_out || !outcome.results.is_empty());
/// # }
/// ```
#[tracing::instrument(
    name = "ucil.group.architecture",
    level = "debug",
    skip(sources),
    fields(source_count = sources.len()),
)]
pub async fn execute_g4(
    query: G4Query,
    sources: Vec<Box<dyn G4Source + Send + Sync + 'static>>,
    master_deadline: Duration,
) -> G4Outcome {
    // Step 1 + Step 2: start time + per-source deadline.
    //
    // The per-source deadline is held at [`G4_PER_SOURCE_DEADLINE`]
    // unconditionally so the master deadline ALWAYS wins on a tight
    // `master_deadline` (e.g. SA8 with 100 ms): when
    // `master_deadline < G4_PER_SOURCE_DEADLINE`, the outer
    // `tokio::time::timeout(master_deadline, ...)` fires first and the
    // master-trip path synthesises in-order
    // [`G4SourceStatus::TimedOut`] placeholders.  Capping
    // `per_source_deadline` by `master_deadline` would race the two
    // timers and let the inner per-source timeout resolve the inner
    // future first, hiding the master trip (WO-0068 + WO-0070
    // lessons-learned, For executor #2 / For planner #3).
    let start = std::time::Instant::now();
    let per_source_deadline = G4_PER_SOURCE_DEADLINE;

    // Step 3: build one boxed future per source and poll them
    // concurrently.
    let q_ref = &query;
    let mut futures: Vec<Pin<Box<dyn Future<Output = G4SourceOutput> + Send + '_>>> =
        Vec::with_capacity(sources.len());
    for s in &sources {
        futures.push(Box::pin(run_g4_source(
            s.as_ref(),
            q_ref,
            per_source_deadline,
        )));
    }

    // Step 4: outer master-deadline wrap.
    let outer = tokio::time::timeout(master_deadline, join_all_g4(futures)).await;
    let wall_elapsed_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

    // Step 5: preserve input order. On master-deadline trip,
    // synthesise `TimedOut` placeholders in input order so
    // `results[i].source_id == sources[i].source_id()` holds either
    // way.
    outer.map_or_else(
        |_| {
            let results = sources
                .iter()
                .map(|s| G4SourceOutput {
                    source_id: s.source_id().to_owned(),
                    status: G4SourceStatus::TimedOut,
                    elapsed_ms: wall_elapsed_ms,
                    edges: vec![],
                    error: Some(format!(
                        "G4 master deadline {} ms elapsed",
                        master_deadline.as_millis()
                    )),
                })
                .collect();
            G4Outcome {
                results,
                wall_elapsed_ms,
                master_timed_out: true,
            }
        },
        |results| G4Outcome {
            results,
            wall_elapsed_ms,
            master_timed_out: false,
        },
    )
}

// ── BFS state alias ───────────────────────────────────────────────────────

/// Per-node BFS state used by [`compute_blast_radius`]:
/// `(depth, cumulative_coupling, contributing_edge)`.  Held as a
/// type alias so the `BTreeMap<String, BfsState>` value side stays
/// under `clippy::type_complexity`.
type BfsState = (u32, f64, Option<(String, String)>);

// ── Edge-union + BFS-blast-radius merger ──────────────────────────────────

/// Edge-union + BFS-blast-radius merger for G4 (Architecture)
/// outputs.
///
/// Master-plan §5.4 lines 483-500 prescribes the merge contract:
///
/// 1. Collect every [`G4DependencyEdge`] from sources whose status is
///    [`G4SourceStatus::Available`] (`Errored` / `TimedOut` sources
///    contribute zero edges).
/// 2. Deduplicate by `(source, target, edge_kind)`.  When ≥ 2 sources
///    contribute the same edge, the deduped output carries every
///    contributing `source_id` (lexicographically sorted) and the
///    maximum `coupling_weight` (highest-confidence-wins, the §5.4
///    adaptation of the §5.3-style invariant).
/// 3. **Ground-truth-on-conflict (master-plan §5.4 line 494) — NOT
///    yet load-bearing.**  When dependency-cruiser (P3-W10-F14) /
///    deptry / other ground-truth sources plug in, the resolver
///    upgrades [`G4EdgeOrigin::GroundTruth`] edges over
///    [`G4EdgeOrigin::Inferred`] edges for the same `(source, target,
///    edge_kind)` tuple.  F09 ships only `Inferred` sources
///    (`CodeGraphContext` + LSP call hierarchy) so this branch is
///    currently unreachable — see the `TODO(P3-W10-F14 / future)`
///    sentinel inside the merge body.  Same shape as WO-0070's
///    `validity_window` deferral on
///    [`crate::g3::G3FactObservation`].
/// 4. Cap [`G4UnionOutcome::unified_edges`] at
///    [`G4Query::max_edges`] (top-N by `coupling_weight`, ties
///    broken by lexicographic `(source, target)`).
/// 5. Bidirectional BFS from every seed in [`G4Query::changed_nodes`]
///    over the surviving unified edges:
///    * Every edge contributes both `(source → target)` and
///      `(target → source)` to the adjacency map (master-plan §5.4
///      line 495 "BFS from changed nodes").
///    * Track `depth` per node (BFS layer); cap at
///      [`G4Query::max_blast_depth`].
///    * Track `cumulative_coupling` per node multiplicatively:
///      `cumulative_coupling[child] = cumulative_coupling[parent] *
///      edge.coupling_weight`, with `cumulative_coupling[seed] = 1.0`.
///      The multiplicative shape lets weak-coupling chains decay
///      quickly per master-plan §5.4 line 495 "weight by coupling
///      strength".
///
/// The merger is pure (no IO, no async, no logging).  Output ordering
/// is deterministic: `unified_edges` sorted by `(source, target)`,
/// `blast_radius` sorted by `(depth, node)`.
///
/// # Examples
///
/// ```
/// use ucil_daemon::g4::{
///     merge_g4_dependency_union, G4DependencyEdge, G4EdgeKind,
///     G4EdgeOrigin, G4Query, G4SourceOutput, G4SourceStatus,
/// };
///
/// let outputs = vec![G4SourceOutput {
///     source_id: "alpha".to_owned(),
///     status: G4SourceStatus::Available,
///     elapsed_ms: 42,
///     edges: vec![G4DependencyEdge {
///         source: "TaskManager".to_owned(),
///         target: "Database".to_owned(),
///         edge_kind: G4EdgeKind::Import,
///         source_id: "alpha".to_owned(),
///         origin: G4EdgeOrigin::Inferred,
///         coupling_weight: 0.85,
///     }],
///     error: None,
/// }];
/// let q = G4Query {
///     changed_nodes: vec!["TaskManager".to_owned()],
///     max_blast_depth: 3,
///     max_edges: 256,
/// };
/// let merged = merge_g4_dependency_union(&outputs, &q);
/// assert_eq!(merged.unified_edges.len(), 1);
/// assert_eq!(merged.blast_radius.len(), 2);
/// ```
#[allow(clippy::too_many_lines)]
#[must_use]
pub fn merge_g4_dependency_union(outputs: &[G4SourceOutput], query: &G4Query) -> G4UnionOutcome {
    // Step 1: collect every edge from `Available` sources (Errored /
    // TimedOut sources contribute zero).
    let mut total_edges_in: usize = 0;
    let mut all_edges: Vec<G4DependencyEdge> = Vec::new();
    for output in outputs {
        if output.status != G4SourceStatus::Available {
            continue;
        }
        for edge in &output.edges {
            total_edges_in += 1;
            all_edges.push(edge.clone());
        }
    }

    // Step 2: dedup by (source, target, edge_kind).  For every edge,
    // either fold it into the existing unified entry (max
    // coupling_weight, append source_id) or append a fresh entry.
    //
    // ── M2 mutation site ─────────────────────────────────────────
    // The verifier's M2 mutation flips `==` to `!=` on the first
    // line of the `position()` predicate below.  Under the mutation,
    // `position()` never finds a matching unified edge → every input
    // edge becomes a fresh `unified_edges` entry → dedup is broken.
    // The pre-baked SA2 panic (`(SA2) unified_edges.len() == 2;
    // left: 3, right: 2`) catches the regression.
    //
    // ── Ground-truth-on-conflict TODO sentinel ───────────────────
    // TODO(P3-W10-F14 / future): when GroundTruth-origin sources
    // (dependency-cruiser, deptry) plug in, the dedup branch should
    // upgrade GroundTruth-origin edges over Inferred-origin edges
    // for the same (source, target, edge_kind) tuple — master-plan
    // §5.4 line 494 ("parse actual imports as ground truth, merge
    // inferred relations").  F09 ships only Inferred sources
    // (CodeGraphContext + LSP call hierarchy) so this branch is
    // currently unreachable.  Same shape as WO-0070's
    // validity_window deferral on G3FactObservation.
    let mut unified_edges: Vec<G4UnifiedEdge> = Vec::new();
    for edge in &all_edges {
        let existing_idx = unified_edges.iter().position(|ue| {
            ue.edge.source == edge.source
                && ue.edge.target == edge.target
                && ue.edge.edge_kind == edge.edge_kind
        });
        match existing_idx {
            Some(idx) => {
                let unified = &mut unified_edges[idx];
                if edge.coupling_weight > unified.edge.coupling_weight {
                    unified.edge.coupling_weight = edge.coupling_weight;
                }
                if !unified.contributing_source_ids.contains(&edge.source_id) {
                    unified.contributing_source_ids.push(edge.source_id.clone());
                }
            }
            None => {
                unified_edges.push(G4UnifiedEdge {
                    edge: edge.clone(),
                    contributing_source_ids: vec![edge.source_id.clone()],
                });
            }
        }
    }

    // Lexicographically sort contributing_source_ids per unified
    // edge for deterministic output (SA2 asserts on the ordering).
    for ue in &mut unified_edges {
        ue.contributing_source_ids.sort();
    }

    // Step 4: cap unified_edges at query.max_edges (top-N by
    // coupling_weight, ties broken by lexicographic source/target).
    if unified_edges.len() > query.max_edges {
        unified_edges.sort_by(|a, b| {
            b.edge
                .coupling_weight
                .partial_cmp(&a.edge.coupling_weight)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.edge.source.cmp(&b.edge.source))
                .then_with(|| a.edge.target.cmp(&b.edge.target))
        });
        unified_edges.truncate(query.max_edges);
    }

    // Sort unified_edges by (source, target) for deterministic
    // output regardless of insertion order.
    unified_edges
        .sort_by(|a, b| (&a.edge.source, &a.edge.target).cmp(&(&b.edge.source, &b.edge.target)));

    // sources_contributing — distinct source_ids across surviving
    // unified edges.
    let sources_contributing: usize = unified_edges
        .iter()
        .flat_map(|ue| ue.contributing_source_ids.iter().cloned())
        .collect::<BTreeSet<String>>()
        .len();

    // Step 5: bidirectional BFS blast radius.
    let blast_radius = compute_blast_radius(&unified_edges, query);

    let total_unique_edges_out = unified_edges.len();
    G4UnionOutcome {
        unified_edges,
        blast_radius,
        total_edges_in,
        total_unique_edges_out,
        sources_contributing,
    }
}

/// Bidirectional BFS over `unified_edges` from every seed in
/// `query.changed_nodes`.
///
/// Visiting state is keyed by node name; each edge contributes both
/// `(source → target)` and `(target → source)` to the adjacency map.
/// `depth` is BFS layer (so seeds carry `0`) — capped at
/// `query.max_blast_depth`.  `cumulative_coupling` is the
/// multiplicative product along the BFS path (seeds carry `1.0`).
///
/// The output is sorted by `(depth, node)` for deterministic
/// downstream consumption.
fn compute_blast_radius(
    unified_edges: &[G4UnifiedEdge],
    query: &G4Query,
) -> Vec<G4BlastRadiusEntry> {
    if query.changed_nodes.is_empty() {
        return Vec::new();
    }

    // Build bidirectional adjacency.  Per master-plan §5.4 line 495,
    // the blast radius is undirected: a change to a node propagates
    // to both its dependents and its dependencies.
    let mut adj: HashMap<String, Vec<(String, f64)>> = HashMap::new();
    for ue in unified_edges {
        let s = ue.edge.source.clone();
        let t = ue.edge.target.clone();
        let w = ue.edge.coupling_weight;
        adj.entry(s.clone()).or_default().push((t.clone(), w));
        adj.entry(t).or_default().push((s, w));
    }

    // BFS state — node -> (depth, cumulative_coupling, edge that
    // brought the node into the radius).  `BTreeMap` keeps iteration
    // deterministic (HashMap would also work but an ordered map is
    // friendlier to debugging).
    let mut visited: BTreeMap<String, BfsState> = BTreeMap::new();
    let mut queue: VecDeque<String> = VecDeque::new();

    for node in &query.changed_nodes {
        if !visited.contains_key(node) {
            visited.insert(node.clone(), (0, 1.0, None));
            queue.push_back(node.clone());
        }
    }

    while let Some(current) = queue.pop_front() {
        let (current_depth, current_weight, _) = visited[&current].clone();
        // ── M3 mutation site ─────────────────────────────────────
        // The verifier's M3 mutation flips `+ 1` to `+ 2` on this
        // line.  Under the mutation, every BFS hop advances by two
        // layers instead of one — child1 lands at depth 2 (instead
        // of 1) and grandchild/cousin land at depth 4 (instead of
        // 2), tripping the depth cap and the SA4 panic
        // (`(SA4) BFS depth child1 == 1; left: 2, right: 1`).
        let next_depth = current_depth + 1;
        if next_depth > query.max_blast_depth {
            continue;
        }
        let neighbors = adj.get(&current).cloned().unwrap_or_default();
        for (neighbor, edge_weight) in neighbors {
            if visited.contains_key(&neighbor) {
                continue;
            }
            let new_weight = current_weight * edge_weight;
            // The edge that brought `neighbor` into the radius is
            // the unified edge between `current` and `neighbor`
            // (in either direction, since BFS is bidirectional).
            let edge_pair = unified_edges.iter().find_map(|ue| {
                if (ue.edge.source == current && ue.edge.target == neighbor)
                    || (ue.edge.target == current && ue.edge.source == neighbor)
                {
                    Some((ue.edge.source.clone(), ue.edge.target.clone()))
                } else {
                    None
                }
            });
            visited.insert(neighbor.clone(), (next_depth, new_weight, edge_pair));
            queue.push_back(neighbor);
        }
    }

    let mut blast_radius: Vec<G4BlastRadiusEntry> = visited
        .into_iter()
        .map(|(node, (depth, weight, edge_pair))| G4BlastRadiusEntry {
            node,
            depth,
            cumulative_coupling: weight,
            contributing_edges: edge_pair.into_iter().collect(),
        })
        .collect();
    blast_radius.sort_by(|a, b| (a.depth, &a.node).cmp(&(b.depth, &b.node)));
    blast_radius
}
