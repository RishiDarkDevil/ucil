//! G8 (Testing) parallel-execution orchestrator + dedup-by-test-path
//! merge fusion.
//!
//! Feature `P3-W11-F09`, master-plan §3.2 row 13 (`review_changes`
//! routes to G7+G8+G4+G1) + row 15 (`run_tests` routes to G8) lines
//! 227+229, §5.8 lines 561-579 (G8 Testing group: "Discover ALL
//! relevant tests via ALL methods: 1. Convention-based / 2. Import-
//! based / 3. KG-based — concurrently, then merge"), §6.1 lines 605-608
//! (Per-group timeout: 5s default; failed groups → empty results +
//! `_meta.degraded_groups`), §15.2 line 1521 (`ucil.group.testing`
//! span), §17.2 line 1636 (`g8.rs` placement), §18 Phase 3 Week 11
//! (G7/G8 Quality + Testing).
//!
//! # Pipeline shape
//!
//! Master-plan §5.8 prescribes the G8 (Testing) fan-out as
//! "Discover ALL relevant tests via ALL methods: 1. Convention-based /
//! 2. Import-based / 3. KG-based — concurrently, then merge": every
//! installed discovery source (`ConventionG8Source`, `ImportG8Source`,
//! `KgRelationsG8Source`, …) runs in parallel under a master deadline;
//! per-source candidates are then merged by `test_path` with method
//! provenance unioned across discovering sources.
//!
//! Same shape as WO-0070 G3 (Knowledge), WO-0083 G4 (Architecture),
//! and WO-0085 G7 (Quality):
//!
//! 1. Each [`G8Source::execute`] runs under a per-source
//!    `tokio::time::timeout` parameterised by [`G8_PER_SOURCE_DEADLINE`].
//! 2. The whole fan-out runs under
//!    `tokio::time::timeout(master_deadline, ...)`.
//! 3. On master-deadline trip, [`G8SourceStatus::TimedOut`]
//!    placeholders are synthesised in input order so
//!    `results[i].source_id` matches `sources[i].source_id()` either
//!    way.
//!
//! # Merge contract
//!
//! [`merge_g8_test_discoveries`] groups every [`G8TestCandidate`] from
//! `Available` sources by `test_path`. Within each group the algorithm
//! unions every [`TestDiscoveryMethod`] that discovered the
//! `test_path`, takes `max(confidence)` across contributors, and
//! unions every `Some(source_path)` deduplicated alphabetical. The
//! output [`MergedG8TestCandidate`] vec is sorted alphabetical by
//! `test_path`.
//!
//! `Errored` or `TimedOut` sources contribute zero candidates to the
//! merge.
//!
//! # No-substitute-impls policy
//!
//! Per master-plan §15.4 + CLAUDE.md "no substitute impls of critical
//! deps", this module — its public traits, types, and orchestrator —
//! does NOT contain placeholder implementations of convention walkers,
//! import parsers, or KG relation queries. The module ships the trait
//! plus orchestrator plus merger only; production [`G8Source`] impls
//! (e.g. `ConventionG8Source` walking `tests/<lang>-project` fixture
//! conventions, `ImportG8Source` parsing import statements, and
//! `KgRelationsG8Source` querying `SQLite` `tested_by` relations from
//! the WO-0011 KG schema) wiring the 3 methods to real corpus / KG /
//! file-system surfaces are deferred to a follow-up production-wiring
//! WO. The frozen acceptance test
//! [`crate::executor::test_g8_test_discovery_all_methods`] supplies
//! UCIL-internal [`G8Source`] impls (`DEC-0008` §4
//! dependency-inversion seam) under `#[cfg(test)]`.
//!
//! Same shape as WO-0070 G3 / WO-0083 G4 / WO-0085 G7.

#![allow(clippy::module_name_repetitions)]
#![deny(rustdoc::broken_intra_doc_links)]

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::task::Poll;
use std::time::Duration;

use serde::{Deserialize, Serialize};

// ── Constants ─────────────────────────────────────────────────────────────

/// Per-source deadline applied to each [`G8Source::execute`] call.
///
/// Held as an unconditional `const`, NOT `min`'d with the caller-
/// supplied `master_deadline`. Capping per-source by master collapses
/// both timeouts on tight masters and the inner per-source wins,
/// hiding the master trip (WO-0068 lessons-learned, mirrored in G7).
/// The 4.5 s / 5 s margin keeps per-source as the primary path under
/// default config; tight-master cases (e.g. 100 ms test masters) let
/// the master fire first deterministically.
///
/// Mirrors [`crate::g7::G7_PER_SOURCE_DEADLINE`] per master-plan §15
/// timeout discipline.
pub const G8_PER_SOURCE_DEADLINE: Duration = Duration::from_millis(4_500);

/// Default master deadline for the G8 (Testing) parallel-execution
/// orchestrator.
///
/// Master-plan §5.8 + §6.1 line 606 prescribe a 5 s overall deadline
/// for any group fan-out so the daemon can return partial results to
/// the host adapter when one discovery source stalls. When this
/// deadline elapses, [`execute_g8`] returns a [`G8Outcome`] with
/// `master_timed_out = true` and per-source [`G8SourceStatus::TimedOut`]
/// placeholders for any source that had not yet completed.
pub const G8_DEFAULT_MASTER_DEADLINE: Duration = Duration::from_millis(5_000);

// ── Public types ──────────────────────────────────────────────────────────

/// G8 test-discovery method per master-plan §5.8 lines 572-574.
///
/// Three variants matching the prescribed methods:
/// * [`TestDiscoveryMethod::Convention`] — walks the corpus for files
///   matching language-specific test conventions
///   (e.g. `tests/test_*.rs`, `*_test.go`, `*.spec.ts`).
/// * [`TestDiscoveryMethod::Import`] — parses import statements to
///   trace which test files import the changed source files.
/// * [`TestDiscoveryMethod::KgRelations`] — queries the WO-0011
///   knowledge-graph `tested_by` relations to recover historical
///   test-to-source linkage.
///
/// `Hash + Eq` bounds are required so the merger can union variants
/// via [`HashSet<TestDiscoveryMethod>`] in the per-test-path
/// aggregation step.
///
/// Serializes as `snake_case` (`"convention"`, `"import"`,
/// `"kg_relations"`) per WO-0067 §6.2 sentinel-row + WO-0085 §5.7
/// severity sentinel-row vocabulary canary patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestDiscoveryMethod {
    /// Convention-based discovery — master-plan §5.8 line 572.
    Convention,
    /// Import-based discovery — master-plan §5.8 line 573.
    Import,
    /// KG `tested_by` relation discovery — master-plan §5.8 line 574.
    KgRelations,
}

/// One test candidate emitted by a G8 source.
///
/// Production impls construct these from real corpus walks / import
/// graphs / KG queries; the test-side impls construct them from
/// literal [`PathBuf`] values per `scope_in` #33.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct G8TestCandidate {
    /// Path to the test file the source associates with the query.
    pub test_path: PathBuf,
    /// Path to the source file the test allegedly covers, when the
    /// method identifies a single originating source.
    /// `None` when the method does NOT identify a single originating
    /// source file (e.g., `KgRelations` may map a test to multiple
    /// sources; convention-based discovery on a `tests/test_foo.rs`
    /// fixture may map back to `src/foo.rs`).
    pub source_path: Option<PathBuf>,
    /// Discovery method that produced this candidate. Cross-checked
    /// against [`G8Source::method`] by the merger when unioning
    /// `methods_found_by` per [`MergedG8TestCandidate`].
    pub method: TestDiscoveryMethod,
    /// Source-supplied confidence score in `[0.0, 1.0]`. Production
    /// impls SHOULD `f.clamp(0.0, 1.0)` before emitting; the trait
    /// surface does NOT enforce.
    pub confidence: f64,
}

/// Disposition of one G8 source on a given fan-out call.
///
/// Each variant is a discriminant with no inner data — the data lives
/// on [`G8SourceOutput`] via `candidates` / `error` / `elapsed_ms`.
/// Master-plan §5.8 + §6.1 prescribes per-source dispositions so
/// partial outcomes remain usable: a single
/// [`G8SourceStatus::Errored`] does not turn the entire fan-out into
/// a failure.
///
/// Mirrors [`crate::g7::G7SourceStatus`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum G8SourceStatus {
    /// The source returned its candidates within the per-source
    /// deadline.
    Available,
    /// The source's per-source `tokio::time::timeout` elapsed before
    /// it returned a response.
    TimedOut,
    /// The source returned an error (transport / decode / internal).
    Errored,
}

/// One source's contribution to a G8 fan-out outcome.
///
/// The additional `method` field over [`crate::g7::G7SourceOutput`]
/// allows the merger to union by method across all sources finding
/// the same test path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct G8SourceOutput {
    /// Identifier of the source that emitted this output. Stable
    /// across the source's lifetime and unique within one
    /// [`execute_g8`] call — matches [`G8Source::source_id`].
    pub source_id: String,
    /// Discovery method this source declares — matches
    /// [`G8Source::method`]. Stamped on every candidate so the merger
    /// can union by method even when the candidate's `method` field
    /// has been rewritten downstream.
    pub method: TestDiscoveryMethod,
    /// Disposition of the source on this fan-out call.
    pub status: G8SourceStatus,
    /// Wall-clock time the source spent before returning, in
    /// milliseconds.
    pub elapsed_ms: u64,
    /// Source-emitted test candidates. Empty when `status` is
    /// `TimedOut` or `Errored`; the merger ignores those statuses.
    pub candidates: Vec<G8TestCandidate>,
    /// Operator-readable error description for any non-`Available`
    /// status. `None` for [`G8SourceStatus::Available`].
    pub error: Option<String>,
}

/// Aggregate outcome of one [`execute_g8`] fan-out call.
///
/// `results` is a `Vec` whose order matches the input `sources`
/// argument so callers can correlate by index. `master_timed_out` is
/// `true` when the outer master deadline elapsed before all
/// per-source futures completed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct G8Outcome {
    /// Per-source outputs, in the same order as the input `sources`.
    pub results: Vec<G8SourceOutput>,
    /// Wall-clock time the orchestrator spent, in milliseconds.
    pub wall_elapsed_ms: u64,
    /// `true` iff the outer master deadline elapsed before all
    /// per-source futures completed.
    pub master_timed_out: bool,
}

/// G8 (Testing) query input — a list of changed source-file paths
/// for which related tests should be discovered.
///
/// Master-plan §3.2 row 13 (`review_changes`) + row 15 (`run_tests`)
/// drive this query input from the host adapter's request. The frozen
/// acceptance test constructs it directly. Empty `changed_files` is
/// valid (returns empty candidates from each source).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct G8Query {
    /// Source-file paths under change. Each source then discovers
    /// tests related to these paths via its own `method`.
    pub changed_files: Vec<PathBuf>,
}

/// One dedup-by-test-path merged candidate emitted by
/// [`merge_g8_test_discoveries`].
///
/// Master-plan §5.8 line 577 prescribes the merge directive
/// `concurrently, then merge`: the winning entry carries the union
/// of every [`TestDiscoveryMethod`] that discovered its `test_path`,
/// the highest `confidence` observed at that path, and the
/// deduplicated alphabetical-sort union of every `Some(source_path)`
/// across contributors.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MergedG8TestCandidate {
    /// Test file path the merged candidate is anchored on.
    pub test_path: PathBuf,
    /// Deduplicated union of every [`TestDiscoveryMethod`] that
    /// discovered this `test_path`. Sorted by enum discriminant order
    /// (`Convention < Import < KgRelations`) for deterministic output.
    pub methods_found_by: Vec<TestDiscoveryMethod>,
    /// Highest confidence observed across all contributors to this
    /// `test_path`'s group.
    pub max_confidence: f64,
    /// Deduplicated alphabetical-sort union of every contributing
    /// candidate's `Some(source_path)` (None values dropped).
    pub source_paths: Vec<PathBuf>,
}

// ── Trait + helpers ───────────────────────────────────────────────────────

/// Dependency-inversion seam for one G8 (Testing) source.
///
/// Per `DEC-0008` §4 this trait is UCIL-owned — it is **not** a
/// re-export or adapter of any external wire format. The frozen
/// acceptance test
/// [`crate::executor::test_g8_test_discovery_all_methods`] supplies
/// local trait impls of [`G8Source`] (UCIL's own abstraction
/// boundary); production wiring of real subprocess clients (e.g.
/// `ConventionG8Source` walking corpus conventions, `ImportG8Source`
/// parsing imports, `KgRelationsG8Source` querying KG `tested_by`
/// relations) is deferred to follow-up production-wiring WOs.
///
/// The trait method returns `Result<Vec<G8TestCandidate>, String>`
/// (NOT a typed `Error` enum) — production impls' richer error
/// contracts will be wrapped/converted to `String` at the trait
/// boundary. Same shape as [`crate::g7::G7Source`] in spirit; the
/// `Result` shape lets [`run_g8_source`] map source errors to
/// [`G8SourceStatus::Errored`] without forcing every production impl
/// to construct a [`G8SourceOutput`] envelope.
///
/// Same shape as the WO-0047 `G1Source`, WO-0070 `G3Source`,
/// WO-0083 `G4Source`, and WO-0085 `G7Source` traits in role.
/// `Send + Sync` bounds are required so trait objects can live in
/// `Vec<Box<dyn G8Source + Send + Sync + 'static>>` inside the
/// daemon's long-lived server state once the production-wiring WO
/// lands.
#[async_trait::async_trait]
pub trait G8Source: Send + Sync {
    /// Identifies this source without runtime introspection so
    /// [`execute_g8`] can label results by source. The returned
    /// string is expected to be stable across the source's lifetime
    /// and unique within one [`execute_g8`] call.
    fn source_id(&self) -> String;

    /// Returns the discovery method this source implements. Stamped
    /// on every emitted [`G8SourceOutput`] so the merger can union
    /// `methods_found_by` even when the candidate's per-row `method`
    /// has been rewritten downstream.
    fn method(&self) -> TestDiscoveryMethod;

    /// Run this source's test-discovery query.
    ///
    /// Returns `Ok(candidates)` on success or `Err(message)` on any
    /// internal failure. The orchestrator (via [`run_g8_source`])
    /// maps these to [`G8SourceStatus::Available`] /
    /// [`G8SourceStatus::Errored`]; per-source-timeout cases are
    /// synthesised as [`G8SourceStatus::TimedOut`] without ever
    /// invoking this method's error path.
    async fn execute(&self, query: &G8Query) -> Result<Vec<G8TestCandidate>, String>;
}

/// Run one source under [`G8_PER_SOURCE_DEADLINE`], converting a
/// per-source timeout into a [`G8SourceStatus::TimedOut`]
/// [`G8SourceOutput`] without ever panicking.
///
/// The helper keeps [`execute_g8`] focused on the fan-out shape —
/// per-source timeout handling lives here so the orchestrator does
/// not need a `match` arm per disposition. Mirrors `run_g7_source` /
/// `run_g4_source` / `run_g3_source`.
#[tracing::instrument(
    name = "ucil.g8.source",
    level = "debug",
    skip_all,
    fields(source_id = %source.source_id(), method = ?source.method()),
)]
async fn run_g8_source(
    source: &(dyn G8Source + Send + Sync),
    query: &G8Query,
    per_source_deadline: Duration,
) -> G8SourceOutput {
    let source_id = source.source_id();
    let method = source.method();
    let start = std::time::Instant::now();
    match tokio::time::timeout(per_source_deadline, source.execute(query)).await {
        Ok(Ok(candidates)) => {
            let elapsed_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
            G8SourceOutput {
                source_id,
                method,
                status: G8SourceStatus::Available,
                elapsed_ms,
                candidates,
                error: None,
            }
        }
        Ok(Err(msg)) => {
            let elapsed_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
            G8SourceOutput {
                source_id,
                method,
                status: G8SourceStatus::Errored,
                elapsed_ms,
                candidates: vec![],
                error: Some(msg),
            }
        }
        Err(_) => G8SourceOutput {
            source_id,
            method,
            status: G8SourceStatus::TimedOut,
            elapsed_ms: u64::try_from(per_source_deadline.as_millis()).unwrap_or(u64::MAX),
            candidates: vec![],
            error: Some(format!(
                "per-source deadline {} ms exceeded",
                per_source_deadline.as_millis()
            )),
        },
    }
}

/// Poll a `Vec` of pinned-boxed futures concurrently and collect
/// every output once all are ready.
///
/// Behaviourally equivalent to `futures::future::join_all` but uses
/// the same `poll_fn` fan-out shape as `crate::g7::join_all_g7` and
/// `crate::g3::join_all_g3` — `tokio` ships everything we need for a
/// 3-to-N-way fan-out without introducing an additional poll-set
/// abstraction.
///
/// `#[allow(dead_code)]` covers the M1 mutation contract: the M1
/// mutation swaps [`run_g8_source`]'s
/// `tokio::time::timeout(per_source_deadline, source.execute(query))`
/// for a direct `source.execute(query).await`, which leaves the
/// timeout-handling closure unreferenced but does NOT orphan this
/// helper — the dead-code guard remains here mirroring the WO-0070
/// G3 / WO-0085 G7 precedent so any future M1-style mutation that
/// orphans the helper does not flip the verifier from SA-tagged
/// panic to compile failure.
#[allow(dead_code)]
async fn join_all_g8<'a, T>(
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
    slots.into_iter().flatten().collect()
}

// ── Orchestrator ──────────────────────────────────────────────────────────

/// G8 (Testing) parallel-execution orchestrator.
///
/// Master-plan §5.8 lines 561-579 prescribes the fan-out shape:
/// `Discover ALL relevant tests via ALL methods: 1. Convention-based
/// / 2. Import-based / 3. KG-based — concurrently, then merge`, with
/// a 5 s overall deadline so partial outcomes stay usable when one
/// discovery source stalls.
///
/// Implementation:
///
/// 1. The per-source deadline is held at [`G8_PER_SOURCE_DEADLINE`]
///    **unconditionally** — it is NOT `min`'d with `master_deadline`.
///    Capping per-source by master collapses both timeouts on tight
///    masters and hides the master trip (G7 precedent).
/// 2. Each per-source future is wrapped in
///    `tokio::time::timeout(G8_PER_SOURCE_DEADLINE, ...)` via
///    [`run_g8_source`] which returns
///    [`G8SourceStatus::TimedOut`] on elapse.
/// 3. Build one boxed future per source and poll them concurrently
///    through [`join_all_g8`].
/// 4. Wrap the whole join in
///    `tokio::time::timeout(master_deadline, ...)`. On `Err(Elapsed)`,
///    return a [`G8Outcome`] with [`G8SourceStatus::TimedOut`]
///    placeholders for every source and `master_timed_out = true` so
///    downstream code never sees an empty result vector when the
///    master deadline fires.
///
/// The orchestrator never `panic!`s and never `?` propagates an error
/// out — partial results are valid output per master-plan §5.8 +
/// §6.1 line 606.
///
/// Per master-plan §15.2 line 1521, this orchestrator emits a
/// `tracing` span `ucil.group.testing` (parallel to
/// `ucil.group.quality` for G7).
///
/// # Examples
///
/// ```no_run
/// use std::path::PathBuf;
/// use std::time::Duration;
/// use ucil_daemon::g8::{
///     execute_g8, G8Query, G8Source, G8_DEFAULT_MASTER_DEADLINE,
/// };
///
/// # async fn demo(sources: Vec<Box<dyn G8Source + Send + Sync + 'static>>) {
/// let q = G8Query {
///     changed_files: vec![PathBuf::from("src/auth.rs")],
/// };
/// let outcome = execute_g8(q, sources, G8_DEFAULT_MASTER_DEADLINE).await;
/// assert!(!outcome.master_timed_out || !outcome.results.is_empty());
/// # }
/// ```
#[tracing::instrument(
    name = "ucil.group.testing",
    level = "debug",
    skip(sources, query),
    fields(source_count = sources.len()),
)]
pub async fn execute_g8(
    query: G8Query,
    sources: Vec<Box<dyn G8Source + Send + Sync + 'static>>,
    master_deadline: Duration,
) -> G8Outcome {
    // Step 1 + Step 2: start time + per-source deadline.
    let start = std::time::Instant::now();
    let per_source_deadline = G8_PER_SOURCE_DEADLINE;

    // Step 3: build one boxed future per source and poll them
    // concurrently. Same shape as `execute_g7` / `execute_g3`.
    let q_ref = &query;
    let mut futures: Vec<Pin<Box<dyn Future<Output = G8SourceOutput> + Send + '_>>> =
        Vec::with_capacity(sources.len());
    for s in &sources {
        futures.push(Box::pin(run_g8_source(
            s.as_ref(),
            q_ref,
            per_source_deadline,
        )));
    }

    // Step 4: outer master-deadline wrap.
    let outer = tokio::time::timeout(master_deadline, join_all_g8(futures)).await;
    let wall_elapsed_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

    // Step 5: preserve input order. On master-deadline trip,
    // synthesise `TimedOut` placeholders in input order so
    // `results[i].source_id == sources[i].source_id()` holds either
    // way.
    outer.map_or_else(
        |_| {
            let results = sources
                .iter()
                .map(|s| G8SourceOutput {
                    source_id: s.source_id(),
                    method: s.method(),
                    status: G8SourceStatus::TimedOut,
                    elapsed_ms: wall_elapsed_ms,
                    candidates: vec![],
                    error: Some(format!(
                        "G8 master deadline {} ms elapsed",
                        master_deadline.as_millis()
                    )),
                })
                .collect();
            G8Outcome {
                results,
                wall_elapsed_ms,
                master_timed_out: true,
            }
        },
        |results| G8Outcome {
            results,
            wall_elapsed_ms,
            master_timed_out: false,
        },
    )
}

// ── Dedup-by-test-path merge fusion ───────────────────────────────────────

/// Dedup-by-test-path merge for G8 (Testing) candidates.
///
/// Master-plan §5.8 lines 561-579 prescribes the merge contract:
///
/// 1. Group every [`G8TestCandidate`] from `Available` sources by
///    `test_path` (use a `BTreeMap` for deterministic ordering).
/// 2. Within each group, union every observed
///    [`TestDiscoveryMethod`] into a [`HashSet`] then project to a
///    sorted-by-discriminant `Vec`
///    (`Convention < Import < KgRelations`).
/// 3. Take `max(confidence)` across all contributors as the merged
///    `max_confidence`.
/// 4. Union every `Some(source_path)` deduplicated alphabetical-sort
///    into the merged `source_paths` vec.
/// 5. The output is sorted alphabetical by `test_path`
///    ([`BTreeMap`] iteration order).
///
/// `Errored` or `TimedOut` sources contribute zero candidates to the
/// merge.
///
/// The merger is pure-deterministic CPU-bound logic — no async, no
/// IO, no logging — so per WO-0067 §`lessons_applied` #5 + WO-0070 G3
/// merger precedent + WO-0085 G7 merger precedent it does NOT carry
/// a `#[tracing::instrument]` span. [`execute_g8`] (which IS
/// async/IO/orchestration) DOES carry the §15.2
/// `ucil.group.testing` span.
///
/// # Examples
///
/// ```
/// use std::path::PathBuf;
/// use ucil_daemon::g8::{
///     merge_g8_test_discoveries, G8Outcome, G8SourceOutput, G8SourceStatus,
///     G8TestCandidate, TestDiscoveryMethod,
/// };
///
/// let outcome = G8Outcome {
///     results: vec![G8SourceOutput {
///         source_id: "convention-1".to_owned(),
///         method: TestDiscoveryMethod::Convention,
///         status: G8SourceStatus::Available,
///         elapsed_ms: 5,
///         candidates: vec![G8TestCandidate {
///             test_path: PathBuf::from("tests/test_foo.rs"),
///             source_path: Some(PathBuf::from("src/foo.rs")),
///             method: TestDiscoveryMethod::Convention,
///             confidence: 0.9,
///         }],
///         error: None,
///     }],
///     wall_elapsed_ms: 5,
///     master_timed_out: false,
/// };
/// let merged = merge_g8_test_discoveries(&outcome);
/// assert_eq!(merged.len(), 1);
/// assert_eq!(merged[0].test_path, PathBuf::from("tests/test_foo.rs"));
/// ```
#[must_use]
pub fn merge_g8_test_discoveries(outcome: &G8Outcome) -> Vec<MergedG8TestCandidate> {
    // Step 1: group by `test_path`. BTreeMap keeps the iteration
    // order deterministic — alphabetical by PathBuf.
    let mut groups: BTreeMap<PathBuf, (HashSet<TestDiscoveryMethod>, f64, BTreeSet<PathBuf>)> =
        BTreeMap::new();

    for result in &outcome.results {
        if result.status != G8SourceStatus::Available {
            continue;
        }
        for candidate in &result.candidates {
            let entry = groups
                .entry(candidate.test_path.clone())
                .or_insert_with(|| (HashSet::new(), f64::NEG_INFINITY, BTreeSet::new()));
            entry.0.insert(candidate.method);
            if candidate.confidence > entry.1 {
                entry.1 = candidate.confidence;
            }
            if let Some(sp) = &candidate.source_path {
                entry.2.insert(sp.clone());
            }
        }
    }

    // Step 2-5: project the BTreeMap into the output vec.
    groups
        .into_iter()
        .map(|(test_path, (method_set, max_confidence, source_paths))| {
            let mut methods_found_by: Vec<TestDiscoveryMethod> = method_set.into_iter().collect();
            methods_found_by.sort_by_key(|m| match m {
                TestDiscoveryMethod::Convention => 0u8,
                TestDiscoveryMethod::Import => 1u8,
                TestDiscoveryMethod::KgRelations => 2u8,
            });
            let source_paths: Vec<PathBuf> = source_paths.into_iter().collect();
            MergedG8TestCandidate {
                test_path,
                methods_found_by,
                max_confidence,
                source_paths,
            }
        })
        .collect()
}
