//! G5 (Context) parallel-query orchestrator + PageRank-ranked, session-
//! deduped context-assembly fusion — feature `P3-W10-F04`, master-plan
//! §5.5 lines 502-522.
//!
//! # Pipeline shape
//!
//! Master-plan §5.5 prescribes the G5 (Context) fan-out as
//! "Query → ALL context sources run in parallel" — Aider-style
//! repo-map (`PageRank`, 50× bias toward relevant files), Context7
//! (library docs), Open Context (platform docs), Repomix (full file
//! packing), `OpenAPI` MCP, `GraphQL` MCP — with the host adapter
//! receiving partial outcomes whenever one context source stalls.
//! After the fan-out, [`assemble_g5_context`] applies two contracts:
//!
//! 1. **Session dedup** — chunks whose `path` appears in the CEQP
//!    `files_in_context` array (master-plan §5.5 line 517 "skip
//!    content the agent already has") are dropped.
//! 2. **`PageRank` ranking** — survivors are sorted by
//!    [`G5ContextChunk::pagerank_score`] *descending* (master-plan
//!    §5.5 line 506 "rank all context by relevance using `PageRank`
//!    scores"); ties broken stably on `(source_id, path)`
//!    lexicographic.
//!
//! [`execute_g5`] mirrors `executor::execute_g1` (WO-0047),
//! [`crate::g3::execute_g3`] (WO-0070), and [`crate::g4::execute_g4`]
//! (WO-0083):
//!
//! 1. Each [`G5Source::execute`] runs under a per-source
//!    `tokio::time::timeout` parameterised by [`G5_PER_SOURCE_DEADLINE`].
//! 2. The whole fan-out runs under
//!    `tokio::time::timeout(master_deadline, ...)`.
//! 3. On master-deadline trip, [`G5SourceStatus::TimedOut`] placeholders
//!    are synthesised in input order so `results[i].source_id` matches
//!    `sources[i].source_id()` either way.
//!
//! # Token-budget non-enforcement
//!
//! [`G5Query::token_budget`] is captured in the type but
//! [`assemble_g5_context`] does NOT trim or truncate based on it.
//! Token-budget enforcement is the responsibility of the consumer
//! (the future `get_context_for_edit` MCP handler will trim the
//! assembled context to the budget); [`G5AssembledContext::
//! total_token_count`] exposes the unconstrained total so the
//! consumer can decide trimming.  See WO-0091 `scope_out` #8.
//!
//! # No-substitute-impls policy
//!
//! Per master-plan §15.4 + CLAUDE.md "no substitute impls of critical
//! deps", this module — its public traits, types, and orchestrator —
//! does NOT contain placeholder implementations of MCP servers,
//! JSON-RPC transports, or `tokio::process::Command` subprocess
//! runners.  The module ships the trait + orchestrator + assembler
//! only; production [`G5Source`] impls (e.g. `AiderRepoMapG5Source`
//! consuming the WO-0087 `PageRank` engine, `Context7G5Source` /
//! `RepomixG5Source` wrapping the WO-0074 plugin runtimes,
//! `OpenContextG5Source` / `OpenApiG5Source` / `GraphQlG5Source`
//! wrapping future plugin manifests) are deferred to a follow-up
//! production-wiring WO that bundles G5 into the cross-group
//! executor.  The frozen acceptance test
//! [`crate::executor::test_g5_context_assembly`] supplies UCIL-
//! internal `G5Source` impls (`DEC-0008` §4 dependency-inversion
//! seam) under `#[cfg(test)]`.

#![allow(clippy::module_name_repetitions)]

use std::collections::BTreeSet;
use std::future::Future;
use std::pin::Pin;
use std::task::Poll;
use std::time::Duration;

use serde::{Deserialize, Serialize};

// ── Constants ─────────────────────────────────────────────────────────────

/// Master deadline for the G5 (Context) parallel-execution
/// orchestrator.
///
/// Master-plan §5.5 + §6.1 line 606 prescribe a 5 s overall deadline
/// for any group fan-out so the daemon can return partial results to
/// the host adapter when one context source stalls.  When this
/// deadline elapses, [`execute_g5`] returns a [`G5Outcome`] with
/// `master_timed_out = true` and per-source [`G5SourceStatus::TimedOut`]
/// placeholders for any source that had not yet completed.
pub const G5_MASTER_DEADLINE: Duration = Duration::from_millis(5_000);

/// Per-source deadline applied to each [`G5Source::execute`] call.
///
/// **Held as an unconditional `const`, NOT `min`'d with the caller-
/// supplied `master_deadline`.**  WO-0068 + WO-0070 + WO-0083 lessons-
/// learned (For executor #2 / For planner #3) demonstrated that
/// capping per-source by master collapses both timeouts on tight
/// masters and the inner per-source wins, hiding the master trip.
/// The 4.5 s / 5 s margin keeps per-source as the primary path under
/// default config; tight-master cases (e.g. 100 ms test masters) let
/// the master fire first deterministically.
///
/// * Per-source wins only on a true global stall (sleeper > 4.5 s).
/// * Master wins only on tight budgets (master < 4.5 s).
pub const G5_PER_SOURCE_DEADLINE: Duration = Duration::from_millis(4_500);

// ── Public types ──────────────────────────────────────────────────────────

/// G5 (Context) query input — what the host adapter is asking each
/// context source to retrieve.
///
/// Live wiring will derive these from the CEQP-classified host
/// adapter request through master-plan §6.1 query-pipeline; the unit
/// test constructs them directly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct G5Query {
    /// Caller-supplied query text — free-form natural-language plus
    /// symbol-name fragments the source ranks against.
    pub query_text: String,
    /// Files the agent already has in its working context.  Used by
    /// [`assemble_g5_context`] (NOT the source) to drop chunks whose
    /// `path` matches one of these entries — master-plan §5.5 line
    /// 517 "skip content the agent already has".
    pub files_in_context: Vec<String>,
    /// Optional caller-supplied token-budget hint.  Captured in the
    /// type but NOT enforced by [`assemble_g5_context`] —
    /// enforcement is the consumer's responsibility (see module-
    /// rustdoc "Token-budget non-enforcement" + WO-0091 `scope_out`
    /// #8).
    pub token_budget: Option<u32>,
}

/// Kind of context source — master-plan §5.5 lines 504-510.
///
/// Each variant maps 1-to-1 to a future production [`G5Source`]
/// impl (e.g. `AiderRepoMap` → `AiderRepoMapG5Source` consuming the
/// WO-0087 `PageRank` engine, `Context7Docs` → `Context7G5Source`
/// wrapping the WO-0074 plugin runtime).  WO-0091 ships the trait,
/// orchestrator, and assembler only; production impls land in a
/// follow-up production-wiring WO per `DEC-0008` §4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum G5SourceKind {
    /// Aider-style repo-map with `PageRank` scoring (master-plan
    /// §5.5 line 504; consumed by WO-0087's `PageRank` engine).
    AiderRepoMap,
    /// Context7 library docs (master-plan §5.5 line 505).
    Context7Docs,
    /// Repomix full-file packing (master-plan §5.5 line 507).
    RepomixPack,
    /// Open Context platform docs (master-plan §5.5 line 506).
    OpenContextDocs,
    /// `OpenAPI` MCP API specs (master-plan §5.5 line 508).
    OpenApiSpec,
    /// `GraphQL` MCP schema (master-plan §5.5 line 509).
    GraphQlSchema,
}

/// Disposition of one G5 source on a given fan-out call.
///
/// Each variant is a discriminant with no inner data — the data lives
/// on [`G5SourceOutput`] via `chunks` / `error` / `elapsed_ms`.
/// Master-plan §5.5 + §6.1 prescribes per-source dispositions so
/// partial outcomes remain usable: a single
/// [`G5SourceStatus::Errored`] does not turn the entire fan-out into
/// a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum G5SourceStatus {
    /// The source returned its chunks within the per-source deadline.
    Available,
    /// The source's per-source `tokio::time::timeout` elapsed before
    /// it returned a response.
    TimedOut,
    /// The source returned an error (transport / decode / internal).
    Errored,
}

/// One context chunk emitted by a G5 source.
///
/// Master-plan §5.5 lines 506-510 prescribes that every context
/// chunk carries a `pagerank_score` so the assembler can rank across
/// heterogeneous sources (Aider → semantic-symbol scores; Context7 /
/// Repomix / Open Context → synthetic scores from the production
/// adapter at wiring time).  WO-0091 consumes the score as a field;
/// real `PageRank` calculation lives in WO-0087's engine and is NOT
/// in scope here (see WO-0091 `scope_out` #9).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct G5ContextChunk {
    /// Identifier of the source that emitted this chunk; matches
    /// [`G5SourceOutput::source_id`] / [`G5Source::source_id`].
    pub source_id: String,
    /// Kind of source that produced this chunk.  Used by the
    /// assembler only for diagnostics; ranking is `pagerank_score`-
    /// only per master-plan §5.5 line 506.
    pub kind: G5SourceKind,
    /// Path of the file (or virtual path for non-filesystem sources
    /// like `Context7Docs` / `OpenApiSpec`) the chunk represents.
    /// Used by [`assemble_g5_context`] for session-dedup matching
    /// against [`G5Query::files_in_context`].
    pub path: String,
    /// Free-form chunk content the host adapter renders verbatim
    /// (e.g. file body, doc excerpt, schema fragment).
    pub content: String,
    /// `PageRank` score (or synthetic equivalent) the assembler
    /// ranks by — descending per master-plan §5.5 line 506.
    pub pagerank_score: f64,
    /// Optional source-supplied token count for this chunk.  Summed
    /// by [`assemble_g5_context`] into
    /// [`G5AssembledContext::total_token_count`] so the consumer
    /// can decide trimming (see module-rustdoc "Token-budget non-
    /// enforcement").  `None` is treated as zero by the assembler.
    pub token_count: Option<u32>,
}

/// One source's contribution to a G5 fan-out outcome.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct G5SourceOutput {
    /// Identifier of the source that emitted this output.
    pub source_id: String,
    /// Kind of source that produced this output.
    pub kind: G5SourceKind,
    /// Disposition of the source on this fan-out call.
    pub status: G5SourceStatus,
    /// Wall-clock time the source spent before returning, in
    /// milliseconds.
    pub elapsed_ms: u64,
    /// Source-emitted context chunks.  Empty when `status` is
    /// `TimedOut` or `Errored`; the assembler ignores those statuses.
    pub chunks: Vec<G5ContextChunk>,
    /// Operator-readable error description for any non-`Available`
    /// status.  `None` for [`G5SourceStatus::Available`].
    pub error: Option<String>,
}

/// Aggregate outcome of one [`execute_g5`] fan-out call.
///
/// `results` is a `Vec` whose order matches the input `sources`
/// argument so callers can correlate by index.  `master_timed_out` is
/// `true` when the outer master deadline elapsed before all per-source
/// futures completed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct G5Outcome {
    /// Per-source outputs, in the same order as the input `sources`.
    pub results: Vec<G5SourceOutput>,
    /// `true` iff the outer master deadline elapsed before all
    /// per-source futures completed.
    pub master_timed_out: bool,
    /// Wall-clock time the orchestrator spent, in milliseconds.
    pub wall_elapsed_ms: u64,
}

/// Fully assembled G5 context — the output of [`assemble_g5_context`].
///
/// Master-plan §5.5 lines 506-522 prescribes the assembled output
/// shape: chunks ranked by `pagerank_score` descending, session-
/// deduped against `files_in_context`, with `total_token_count` so
/// the host adapter can decide further trimming.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct G5AssembledContext {
    /// Surviving context chunks, sorted by `pagerank_score`
    /// descending (ties broken stably on `(source_id, path)`
    /// lexicographic).
    pub chunks: Vec<G5ContextChunk>,
    /// Total chunks the assembler considered (sum of `chunks.len()`
    /// across every `Available` source).  Pre-dedup count.
    pub total_chunks_in: usize,
    /// Number of chunks dropped because their `path` matched an
    /// entry in the dedup set passed to [`assemble_g5_context`].
    pub deduped_count: usize,
    /// Number of distinct `source_id`s contributing at least one
    /// chunk that survived dedup.
    pub source_count: usize,
    /// Sum of [`G5ContextChunk::token_count`] (treating `None` as
    /// zero) across every surviving chunk.  Surfaced separately so
    /// the consumer can decide trimming without re-summing.
    pub total_token_count: u32,
}

// ── Trait + helpers ───────────────────────────────────────────────────────

/// Dependency-inversion seam for one G5 (Context) source.
///
/// Per `DEC-0008` §4 this trait is UCIL-owned — it is **not** a
/// re-export or adapter of any external wire format.  The frozen
/// acceptance test [`crate::executor::test_g5_context_assembly`]
/// supplies local trait impls of [`G5Source`] (UCIL's own
/// abstraction boundary); production wiring of real subprocess
/// clients (e.g. `AiderRepoMapG5Source` consuming WO-0087's
/// `PageRank` engine, `Context7G5Source` / `RepomixG5Source`
/// wrapping WO-0074's plugin runtimes) is deferred to a follow-up
/// production-wiring WO.
///
/// Same shape as the WO-0047 [`crate::executor::G1Source`] trait,
/// the WO-0070 [`crate::g3::G3Source`] trait, and the WO-0083
/// [`crate::g4::G4Source`] trait.  `Send + Sync` bounds are
/// required so trait objects can live in
/// `Vec<Box<dyn G5Source + Send + Sync + 'static>>` inside the
/// daemon's long-lived server state once the production-wiring WO
/// lands.
#[async_trait::async_trait]
pub trait G5Source: Send + Sync {
    /// Identifies this source without runtime introspection so
    /// [`execute_g5`] can label results by source.  The returned
    /// string is expected to be stable across the source's lifetime
    /// and unique within one [`execute_g5`] call.
    fn source_id(&self) -> &str;

    /// Kind of the source (one of the master-plan §5.5
    /// vocabulary slots).  Consumed by the orchestrator for the
    /// `TimedOut` / master-trip placeholder synthesis path so the
    /// downstream caller can still tell which kind stalled without
    /// also reaching for [`G5Source::source_id`].
    fn kind(&self) -> G5SourceKind;

    /// Run this source's context query.
    ///
    /// Implementations are responsible for emitting their own
    /// [`G5SourceOutput`] with the appropriate
    /// [`G5SourceStatus`] — the orchestrator only overrides the
    /// status to [`G5SourceStatus::TimedOut`] when its per-source
    /// `tokio::time::timeout` elapses.
    async fn execute(&self, query: &G5Query) -> G5SourceOutput;
}

/// Run one source under [`G5_PER_SOURCE_DEADLINE`], converting a
/// per-source timeout into a [`G5SourceStatus::TimedOut`]
/// [`G5SourceOutput`] without ever panicking.
///
/// The helper keeps [`execute_g5`] focused on the fan-out shape —
/// per-source timeout handling lives here so the orchestrator does
/// not need a `match` arm per disposition.  Mirrors `run_g1_source`
/// in `executor.rs`, `run_g3_source` in `g3.rs`, and `run_g4_source`
/// in `g4.rs` adapted to the [`G5SourceOutput`] shape.
async fn run_g5_source(
    source: &(dyn G5Source + Send + Sync),
    query: &G5Query,
    per_source_deadline: Duration,
) -> G5SourceOutput {
    let source_id = source.source_id().to_owned();
    let kind = source.kind();
    let start = std::time::Instant::now();
    tokio::time::timeout(per_source_deadline, source.execute(query))
        .await
        .unwrap_or_else(|_| {
            let elapsed_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
            G5SourceOutput {
                source_id,
                kind,
                status: G5SourceStatus::TimedOut,
                elapsed_ms,
                chunks: vec![],
                error: Some(format!(
                    "per-source deadline {} ms exceeded",
                    per_source_deadline.as_millis()
                )),
            }
        })
}

/// Poll a `Vec` of pinned-boxed futures concurrently and collect
/// every output once all are ready.
///
/// Behaviourally equivalent to `futures::future::join_all` but uses
/// the same `poll_fn` fan-out shape as `executor::join_all_g1`,
/// `g3::join_all_g3`, and `g4::join_all_g4` — `tokio` ships
/// everything we need for a 3-to-N-way fan-out without introducing
/// an additional poll-set abstraction.
///
/// `#[allow(dead_code)]` covers the verifier's M1 mutation
/// contract: the M1 mutation swaps [`execute_g5`]'s parallel join
/// site (the per-source `tokio::time::timeout`) for a bare
/// `source.execute(query).await`, leaving this helper unreferenced
/// only on the per-source-timeout-bypass mutation.  Without the
/// allow, `#![deny(warnings)]` (set at the crate root in `lib.rs`)
/// would convert the dead-code lint into a compile error and the
/// verifier would observe a compile failure instead of the SA4
/// panic the contract expects (WO-0070 §executor lesson "atomic M1
/// mutation contract via `#[allow(dead_code)]` on private helper").
#[allow(dead_code)]
async fn join_all_g5<'a, T>(
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
        .map(|r| r.expect("join_all_g5: every slot must be filled before returning"))
        .collect()
}

// ── Orchestrator ──────────────────────────────────────────────────────────

/// G5 (Context) parallel-execution orchestrator.
///
/// Master-plan §5.5 lines 502-522 prescribes the fan-out shape:
/// `Query → ALL context sources run in parallel` (Aider repo-map,
/// Context7, Open Context, Repomix, `OpenAPI`, `GraphQL`), with a
/// 5 s overall deadline so partial outcomes stay usable when one
/// context source stalls.
///
/// Implementation:
///
/// 1. The per-source deadline is held at [`G5_PER_SOURCE_DEADLINE`]
///    **unconditionally** — it is NOT `min`'d with `master_deadline`.
///    Per WO-0068 + WO-0070 + WO-0083 lessons-learned (For executor
///    #2 / For planner #3), capping per-source by master collapses
///    both timeouts on tight masters and the inner per-source wins,
///    hiding the master trip.  The 4.5 s / 5 s margin keeps
///    per-source as the primary path under default config (master =
///    5 s); tight-master cases (e.g. 100 ms test masters) let the
///    master fire first deterministically.
/// 2. Each per-source future is wrapped in
///    `tokio::time::timeout(G5_PER_SOURCE_DEADLINE, ...)` via
///    [`run_g5_source`] which returns
///    [`G5SourceStatus::TimedOut`] on elapse.
/// 3. Build one boxed future per source and poll them concurrently
///    through [`join_all_g5`] (the same `poll_fn` fan-out shape as
///    `execute_g1` / `execute_g3` / `execute_g4`).
/// 4. Wrap the whole join in
///    `tokio::time::timeout(master_deadline, ...)`.  On
///    `Err(Elapsed)`, return a [`G5Outcome`] with
///    [`G5SourceStatus::TimedOut`] placeholders for every source
///    and `master_timed_out = true` so downstream code never sees
///    an empty result vector when the master deadline fires.
///
/// The orchestrator never `panic!`s and never `?` propagates an
/// error out — partial results are valid output per master-plan
/// §5.5 + §6.1 line 606.
///
/// Per master-plan §15.2 line 1519, this orchestrator emits a
/// `tracing` span `ucil.group.context` (parallel to
/// `ucil.group.architecture` for G4 and `ucil.group.knowledge` for
/// G3).  The instrument decorator is appropriate here because
/// `execute_g5` is async/IO orchestration — unlike the
/// deterministic [`assemble_g5_context`] (and the WO-0067
/// `ceqp::parse_reason`) which intentionally has no span.
///
/// # Examples
///
/// ```no_run
/// use std::time::Duration;
/// use ucil_daemon::g5::{
///     execute_g5, G5Query, G5Source, G5_MASTER_DEADLINE,
/// };
///
/// # async fn demo(sources: Vec<Box<dyn G5Source + Send + Sync + 'static>>) {
/// let q = G5Query {
///     query_text: "rate limiter".to_owned(),
///     files_in_context: vec!["src/auth.rs".to_owned()],
///     token_budget: Some(8_000),
/// };
/// let outcome = execute_g5(&sources, &q, G5_MASTER_DEADLINE).await;
/// assert!(!outcome.master_timed_out || !outcome.results.is_empty());
/// # }
/// ```
#[tracing::instrument(name = "ucil.group.context", skip_all, fields(source_count = sources.len()))]
pub async fn execute_g5(
    sources: &[Box<dyn G5Source>],
    query: &G5Query,
    master_deadline: Duration,
) -> G5Outcome {
    // Step 1 + Step 2: start time + per-source deadline.
    //
    // The per-source deadline is held at [`G5_PER_SOURCE_DEADLINE`]
    // unconditionally so the master deadline ALWAYS wins on a tight
    // `master_deadline` (e.g. SA6 with 100 ms): when
    // `master_deadline < G5_PER_SOURCE_DEADLINE`, the outer
    // `tokio::time::timeout(master_deadline, ...)` fires first and
    // the master-trip path synthesises in-order
    // [`G5SourceStatus::TimedOut`] placeholders.  Capping
    // `per_source_deadline` by `master_deadline` would race the two
    // timers and let the inner per-source timeout resolve the inner
    // future first, hiding the master trip (WO-0068 + WO-0070 +
    // WO-0083 lessons-learned, For executor #2 / For planner #3).
    let start = std::time::Instant::now();
    let per_source_deadline = G5_PER_SOURCE_DEADLINE;

    // Step 3: build one boxed future per source and poll them
    // concurrently.
    let mut futures: Vec<Pin<Box<dyn Future<Output = G5SourceOutput> + Send + '_>>> =
        Vec::with_capacity(sources.len());
    for s in sources {
        futures.push(Box::pin(run_g5_source(
            s.as_ref(),
            query,
            per_source_deadline,
        )));
    }

    // Step 4: outer master-deadline wrap.
    let outer = tokio::time::timeout(master_deadline, join_all_g5(futures)).await;
    let wall_elapsed_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

    // Step 5: preserve input order. On master-deadline trip,
    // synthesise `TimedOut` placeholders in input order so
    // `results[i].source_id == sources[i].source_id()` holds either
    // way.
    outer.map_or_else(
        |_| {
            let results = sources
                .iter()
                .map(|s| G5SourceOutput {
                    source_id: s.source_id().to_owned(),
                    kind: s.kind(),
                    status: G5SourceStatus::TimedOut,
                    elapsed_ms: wall_elapsed_ms,
                    chunks: vec![],
                    error: Some(format!(
                        "G5 master deadline {} ms elapsed",
                        master_deadline.as_millis()
                    )),
                })
                .collect();
            G5Outcome {
                results,
                wall_elapsed_ms,
                master_timed_out: true,
            }
        },
        |results| G5Outcome {
            results,
            wall_elapsed_ms,
            master_timed_out: false,
        },
    )
}

// ── PageRank-ranked, session-deduped assembler ────────────────────────────

/// Assemble the G5 fan-out [`G5Outcome`] into a deduped + ranked
/// [`G5AssembledContext`].
///
/// Master-plan §5.5 lines 506-522 prescribes the assembly contract:
///
/// 1. Collect every [`G5ContextChunk`] from sources whose status is
///    [`G5SourceStatus::Available`] (`Errored` / `TimedOut` sources
///    contribute zero chunks — master-plan §5.5 line 517 requires
///    "Exclude irrelevant content entirely").
/// 2. **Session dedup** (master-plan §5.5 line 517 "skip content
///    the agent already has, `files_in_context`"): drop chunks
///    whose `path` field appears in `files_in_context`.  Membership
///    is checked through a `BTreeSet<&str>` for O(log n) lookup.
/// 3. **`PageRank` ranking** (master-plan §5.5 line 506 "rank all
///    context by relevance using `PageRank` scores"): sort
///    survivors by [`G5ContextChunk::pagerank_score`] **descending**
///    with stable lexicographic tiebreak on `(source_id, path)` so
///    deterministic output survives across runs.
/// 4. Sum [`G5ContextChunk::token_count`] (treating `None` as
///    zero) into [`G5AssembledContext::total_token_count`] so the
///    consumer can trim if needed (no enforcement at this layer —
///    see module-rustdoc "Token-budget non-enforcement").
///
/// The assembler is pure (no IO, no async, no logging) and
/// therefore **does NOT carry `#[tracing::instrument]`** — per
/// master-plan §15.2 line 1519 `ucil.<layer>.<op>` span naming
/// only applies to async/IO/orchestration paths.  WO-0067 lessons-
/// learned ("pure CPU-bound merge functions are exempt from §15.2
/// tracing", `ceqp::parse_reason` precedent) carries the rationale.
///
/// # Examples
///
/// ```
/// use ucil_daemon::g5::{
///     assemble_g5_context, G5ContextChunk, G5Outcome, G5SourceKind,
///     G5SourceOutput, G5SourceStatus,
/// };
///
/// let outcome = G5Outcome {
///     results: vec![G5SourceOutput {
///         source_id: "aider".to_owned(),
///         kind: G5SourceKind::AiderRepoMap,
///         status: G5SourceStatus::Available,
///         elapsed_ms: 42,
///         chunks: vec![G5ContextChunk {
///             source_id: "aider".to_owned(),
///             kind: G5SourceKind::AiderRepoMap,
///             path: "src/lib.rs".to_owned(),
///             content: "// lib body".to_owned(),
///             pagerank_score: 0.9,
///             token_count: Some(64),
///         }],
///         error: None,
///     }],
///     master_timed_out: false,
///     wall_elapsed_ms: 42,
/// };
/// let assembled = assemble_g5_context(outcome, &[]);
/// assert_eq!(assembled.chunks.len(), 1);
/// assert_eq!(assembled.total_token_count, 64);
/// ```
#[must_use]
pub fn assemble_g5_context(outcome: G5Outcome, files_in_context: &[String]) -> G5AssembledContext {
    // Step 1: flatten chunks from `Available`-status results only.
    //
    // `Errored` / `TimedOut` sources contribute zero chunks per
    // master-plan §5.5 line 517 ("Exclude irrelevant content
    // entirely") and the upstream contract that
    // [`G5SourceOutput::chunks`] is empty when status is non-
    // `Available`.
    let mut flattened: Vec<G5ContextChunk> = Vec::new();
    for output in outcome.results {
        if output.status != G5SourceStatus::Available {
            continue;
        }
        flattened.extend(output.chunks);
    }
    let total_chunks_in = flattened.len();

    // Step 2: session dedup against `files_in_context`.
    //
    // BTreeSet<&str> for O(log n) membership lookup keyed on the
    // borrowed path — no allocation per check.  The .filter()
    // shape is the M2 mutation target (replacing the predicate
    // with `.filter(|_| true)` bypasses the dedup contract — see
    // WO-0091 acceptance AC18).
    let dedup_set: BTreeSet<&str> = files_in_context.iter().map(String::as_str).collect();
    let mut chunks: Vec<G5ContextChunk> = flattened
        .into_iter()
        .filter(|c| !dedup_set.contains(c.path.as_str()))
        .collect();
    let deduped_count = total_chunks_in - chunks.len();

    // Step 3: PageRank ranking, descending, with stable
    // lexicographic tiebreak on (source_id, path).  The
    // `b.partial_cmp(&a)` direction is the M3 mutation target
    // (swapping a/b inverts the sort to ascending — see WO-0091
    // acceptance AC19).  partial_cmp -> unwrap_or(Equal) treats
    // NaN scores as equal so we never panic on a degenerate
    // source-supplied float.
    chunks.sort_by(|a, b| {
        b.pagerank_score
            .partial_cmp(&a.pagerank_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.source_id.cmp(&b.source_id))
            .then_with(|| a.path.cmp(&b.path))
    });

    // Step 4: sum token_count (None treated as zero) and count
    // distinct source_ids in survivors.
    let total_token_count: u32 = chunks
        .iter()
        .map(|c| c.token_count.unwrap_or(0))
        .fold(0_u32, u32::saturating_add);
    let distinct_sources: BTreeSet<&str> = chunks.iter().map(|c| c.source_id.as_str()).collect();
    let source_count = distinct_sources.len();

    G5AssembledContext {
        chunks,
        total_chunks_in,
        deduped_count,
        source_count,
        total_token_count,
    }
}
