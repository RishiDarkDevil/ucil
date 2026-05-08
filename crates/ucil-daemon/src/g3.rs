//! G3 (Knowledge) parallel-query orchestrator + entity-keyed
//! temporal-priority merger — feature `P3-W9-F07`, master-plan §5.3
//! lines 469-479.
//!
//! # Pipeline shape
//!
//! Master-plan §5.3 prescribes the G3 (Knowledge) fan-out as
//! "All knowledge stores queried → temporal merge": every installed
//! knowledge plugin (codebase-memory, Mem0, Graphiti, …) runs in
//! parallel under a master deadline; per-source outputs are then
//! merged by `entity` with newer observations winning on conflict and
//! highest-confidence observations winning on agreement.
//!
//! [`execute_g3`] mirrors `executor::execute_g1` (WO-0047) and
//! `cross_group::execute_cross_group` (WO-0068):
//!
//! 1. Each [`G3Source::execute`] runs under a per-source
//!    `tokio::time::timeout` parameterised by [`G3_PER_SOURCE_DEADLINE`].
//! 2. The whole fan-out runs under
//!    `tokio::time::timeout(master_deadline, ...)`.
//! 3. On master-deadline trip, [`G3SourceStatus::TimedOut`] placeholders
//!    are synthesised in input order so `results[i].source_id` matches
//!    `sources[i].source_id()` either way.
//!
//! # Merge contract
//!
//! [`merge_g3_by_entity`] groups every [`G3FactObservation`] from
//! `Available` sources by [`G3FactObservation::entity`].  Within each
//! entity group the algorithm partitions observations into
//! agreement-clusters (same `fact` text, winner = highest confidence,
//! ties broken by newest `observed_ts_ns`, then lexicographic
//! `source_id`) and conflict-clusters (different `fact` text, winner =
//! newest `observed_ts_ns`, ties broken by highest confidence, then
//! lexicographic `source_id`) — distinct from the rank-based G2 / cross-
//! group RRF (WO-0056 / WO-0068).
//!
//! `Errored` or `TimedOut` sources contribute zero observations to
//! the merge.  The output [`G3MergeOutcome`] emits one [`G3MergedFact`] per
//! distinct entity with `conflict_count` = (distinct-fact-strings - 1)
//! and `agreement_count` = (observations supporting the winning fact).
//!
//! # Forbidden-pattern declaration
//!
//! Per master-plan §15.4 + CLAUDE.md "no mocks of critical deps", this
//! module — including its public traits, types, and orchestrator —
//! does NOT contain mock implementations of MCP servers, JSON-RPC
//! transports, or `tokio::process::Command` subprocess runners.  The
//! module ships the trait + orchestrator + merger only; production
//! `G3Source` impls (e.g. `CodebaseMemoryG3Source`, `Mem0G3Source`)
//! are deferred to a follow-up production-wiring WO that bundles G3
//! into the cross-group executor.  The frozen acceptance test
//! [`crate::executor::test_g3_parallel_merge`] supplies UCIL-internal
//! `G3Source` impls (`DEC-0008` §4 dependency-inversion seam) under
//! `#[cfg(test)]`; those test impls are not mocks of any external
//! wire format.

#![allow(clippy::module_name_repetitions)]

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::task::Poll;
use std::time::Duration;

use serde::{Deserialize, Serialize};

// ── Constants ─────────────────────────────────────────────────────────────

/// Master deadline for the G3 (Knowledge) parallel-execution
/// orchestrator.
///
/// Master-plan §5.3 + §6.1 line 606 prescribe a 5 s overall deadline
/// for any group fan-out so the daemon can return partial results to
/// the host adapter when one knowledge store stalls.  When this
/// deadline elapses, [`execute_g3`] returns a [`G3Outcome`] with
/// `master_timed_out = true` and per-source [`G3SourceStatus::TimedOut`]
/// placeholders for any source that had not yet completed.
pub const G3_MASTER_DEADLINE: Duration = Duration::from_millis(5_000);

/// Per-source deadline applied to each [`G3Source::execute`] call.
///
/// **Held as an unconditional `const`, NOT `min`'d with the caller-
/// supplied `master_deadline`.**  WO-0068 lessons-learned (For
/// executor #2 + For planner #3) demonstrated that capping per-source
/// by master collapses both timeouts on tight masters and the inner
/// per-source wins, hiding the master trip.  The 4.5 s / 5 s margin
/// keeps per-source as the primary path under default config; tight-
/// master cases (e.g. 100 ms test masters) let the master fire first
/// deterministically.
///
/// * Per-source wins only on a true global stall (sleeper > 4.5 s).
/// * Master wins only on tight budgets (master < 4.5 s).
pub const G3_PER_SOURCE_DEADLINE: Duration = Duration::from_millis(4_500);

// ── Public types ──────────────────────────────────────────────────────────

/// G3 (Knowledge) query input — entity-anchored fact lookup over every
/// installed knowledge store.
///
/// Live wiring will derive these from the host adapter's
/// `find_definition` / `recall_session_history` request through the
/// master-plan §6.1 query-pipeline classifier; the unit test
/// constructs them directly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct G3Query {
    /// Entities the caller wants facts about.  An empty `entities`
    /// vector is permitted — sources may interpret that as "any
    /// entity" or as a no-op; the merger treats every emitted
    /// observation independently.
    pub entities: Vec<String>,
    /// Maximum total facts the caller wants returned across all
    /// entities.  Sources MAY emit fewer; the merger does not enforce
    /// this cap (it is informational so sources can self-limit).
    pub max_results: usize,
    /// Minimum confidence (in `[0.0, 1.0]`) the caller will accept.
    /// Sources MAY filter their own emissions; the merger does not
    /// re-filter (so a strict caller can post-filter
    /// [`G3MergedFact::winning_confidence`]).
    pub min_confidence: f64,
}

/// One fact observation emitted by a knowledge source.
///
/// Master-plan §5.3 lines 473-477 prescribes the entity-keyed temporal
/// merge: a `fact` string anchored to an `entity` (e.g. a function
/// name, a file path, a session id), tagged with a source-defined
/// `confidence` and an `observed_ts_ns` timestamp the merger uses to
/// resolve conflicts.
///
/// `validity_window` is reserved for future Graphiti (F10) integration
/// — when present, `(start_ns, end_ns)` would override the bare
/// `observed_ts_ns` precedence per master-plan §5.3 line 475 ("Graphiti
/// temporal validity wins").  F07 leaves the field on the struct but
/// does NOT consume it in the merge body — see the sentinel comment in
/// [`merge_g3_by_entity`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct G3FactObservation {
    /// Identifier of the source that emitted this observation; matches
    /// the `source_id` returned by [`G3SourceOutput::source_id`] and
    /// [`G3Source::source_id`].
    pub source_id: String,
    /// Entity this observation is anchored to (e.g. function name,
    /// file path, session id).
    pub entity: String,
    /// Fact text — a free-form string that the merger compares
    /// byte-for-byte (`String::==`) when partitioning observations
    /// into agreement-clusters vs conflict-clusters.
    pub fact: String,
    /// Source-defined confidence in `[0.0, 1.0]`.  The merger uses
    /// this to break ties on agreement-clusters and to attribute the
    /// winning observation on conflict-clusters.
    pub confidence: f64,
    /// Wall-clock timestamp the observation was made, in nanoseconds
    /// since the Unix epoch.  Used by the merger as the temporal-
    /// priority key on conflict-clusters.
    pub observed_ts_ns: u128,
    /// Optional Graphiti-style temporal validity window
    /// `(valid_from_ns, valid_to_ns)`.  Reserved for F10 (Graphiti P1
    /// plugin) — F07 ships the field but does NOT load-bear on it.
    pub validity_window: Option<(u128, u128)>,
}

/// Disposition of one G3 source on a given fan-out call.
///
/// Each variant is a discriminant with no inner data — the data lives
/// on [`G3SourceOutput`] via `observations` / `error` / `elapsed_ms`.
/// Master-plan §5.3 + §6.1 prescribes per-source dispositions so partial
/// outcomes remain usable: a single [`G3SourceStatus::Errored`] does
/// not turn the entire fan-out into a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum G3SourceStatus {
    /// The source returned its observations within the per-source
    /// deadline.
    Available,
    /// The source's per-source `tokio::time::timeout` elapsed before
    /// it returned a response.
    TimedOut,
    /// The source returned an error (transport / decode / internal).
    Errored,
}

/// One source's contribution to a G3 fan-out outcome.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct G3SourceOutput {
    /// Identifier of the source that emitted this output.
    pub source_id: String,
    /// Disposition of the source on this fan-out call.
    pub status: G3SourceStatus,
    /// Wall-clock time the source spent before returning, in
    /// milliseconds.
    pub elapsed_ms: u64,
    /// Source-emitted fact observations.  Empty when `status` is
    /// `TimedOut` or `Errored`; the merger ignores those statuses.
    pub observations: Vec<G3FactObservation>,
    /// Operator-readable error description for any non-`Available`
    /// status.  `None` for [`G3SourceStatus::Available`].
    pub error: Option<String>,
}

/// Aggregate outcome of one [`execute_g3`] fan-out call.
///
/// `results` is a `Vec` whose order matches the input `sources`
/// argument so callers can correlate by index.  `master_timed_out` is
/// `true` when the outer master deadline elapsed before all per-source
/// futures completed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct G3Outcome {
    /// Per-source outputs, in the same order as the input `sources`.
    pub results: Vec<G3SourceOutput>,
    /// Wall-clock time the orchestrator spent, in milliseconds.
    pub wall_elapsed_ms: u64,
    /// `true` iff the outer master deadline elapsed before all
    /// per-source futures completed.
    pub master_timed_out: bool,
}

/// One merged fact emitted by [`merge_g3_by_entity`].
///
/// The fields directly project the master-plan §5.3 line 476-477
/// contract: a winning `fact` per `entity`, plus the metadata needed
/// to surface conflict / agreement counts to the host adapter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct G3MergedFact {
    /// Entity this merged fact is anchored to.
    pub entity: String,
    /// The winning fact text after temporal-priority resolution.
    pub winning_fact: String,
    /// `source_id` of the observation that won the entity's
    /// resolution.
    pub winning_source_id: String,
    /// Confidence of the winning observation.
    pub winning_confidence: f64,
    /// `observed_ts_ns` of the winning observation.
    pub winning_ts_ns: u128,
    /// Number of distinct `fact` strings observed for this entity
    /// minus 1 (zero when every observation agreed).
    pub conflict_count: u32,
    /// Number of observations whose `fact` string equals
    /// `winning_fact` (so an agreement-cluster of size 3 reports
    /// `agreement_count = 3`).
    pub agreement_count: u32,
}

/// Aggregate outcome of one [`merge_g3_by_entity`] call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct G3MergeOutcome {
    /// One [`G3MergedFact`] per distinct entity observed across every
    /// `Available` source.  Sorted by entity name (`BTreeMap`-driven)
    /// for deterministic output.
    pub merged: Vec<G3MergedFact>,
    /// Total observations the merger considered (sum of
    /// `observations.len()` across every `Available` source).
    pub total_observations_in: usize,
    /// Number of distinct entities in `merged` (equal to
    /// `merged.len()` — surfaced separately so consumers do not need
    /// to recompute it).
    pub total_entities_out: usize,
}

// ── Trait + helpers ───────────────────────────────────────────────────────

/// Dependency-inversion seam for one G3 (Knowledge) source.
///
/// Per `DEC-0008` §4 this trait is UCIL-owned — it is **not** a
/// re-export or adapter of any external wire format.  The frozen
/// acceptance test `executor::test_g3_parallel_merge` supplies local
/// trait impls of [`G3Source`] (UCIL's own abstraction boundary);
/// production wiring of real subprocess clients (e.g.
/// `CodebaseMemoryG3Source` calling the F05 plugin via the Mem0 /
/// codebase-memory MCP transport) is deferred to a follow-up
/// production-wiring WO.
///
/// Same shape as the WO-0047 `G1Source` trait and the WO-0068
/// `GroupExecutor` trait.  `Send + Sync` bounds are required so trait
/// objects can live in `Vec<Box<dyn G3Source + Send + Sync + 'static>>`
/// inside the daemon's long-lived server state once the production-
/// wiring WO lands.
#[async_trait::async_trait]
pub trait G3Source: Send + Sync {
    /// Identifies this source without runtime introspection so
    /// [`execute_g3`] can label results by source.  The returned
    /// string is expected to be stable across the source's lifetime
    /// and unique within one [`execute_g3`] call.
    fn source_id(&self) -> &str;

    /// Run this source's knowledge query.
    ///
    /// Implementations are responsible for emitting their own
    /// [`G3SourceOutput`] with the appropriate
    /// [`G3SourceStatus`] — the orchestrator only overrides the status
    /// to [`G3SourceStatus::TimedOut`] when its per-source
    /// `tokio::time::timeout` elapses.
    async fn execute(&self, query: &G3Query) -> G3SourceOutput;
}

/// Run one source under [`G3_PER_SOURCE_DEADLINE`], converting a
/// per-source timeout into a [`G3SourceStatus::TimedOut`]
/// [`G3SourceOutput`] without ever panicking.
///
/// The helper keeps [`execute_g3`] focused on the fan-out shape —
/// per-source timeout handling lives here so the orchestrator does not
/// need a `match` arm per disposition.  Mirrors `run_g1_source` in
/// `executor.rs` and `run_group_executor` in `cross_group.rs` adapted
/// to the [`G3SourceOutput`] shape.
async fn run_g3_source(
    source: &(dyn G3Source + Send + Sync),
    query: &G3Query,
    per_source_deadline: Duration,
) -> G3SourceOutput {
    let source_id = source.source_id().to_owned();
    let start = std::time::Instant::now();
    tokio::time::timeout(per_source_deadline, source.execute(query))
        .await
        .unwrap_or_else(|_| {
            let elapsed_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
            G3SourceOutput {
                source_id,
                status: G3SourceStatus::TimedOut,
                elapsed_ms,
                observations: vec![],
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
/// `cross_group::join_all_cross_group` — `tokio` ships everything we
/// need for a 3-to-N-way fan-out without introducing an additional
/// poll-set abstraction.
async fn join_all_g3<'a, T>(
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
        .map(|r| r.expect("join_all_g3: every slot must be filled before returning"))
        .collect()
}

// ── Orchestrator ──────────────────────────────────────────────────────────

/// G3 (Knowledge) parallel-execution orchestrator.
///
/// Master-plan §5.3 lines 469-477 prescribes the fan-out shape:
/// `Query → ALL of {codebase-memory, Mem0, Graphiti, …} run in
/// parallel`, with a 5 s overall deadline so partial outcomes stay
/// usable when one knowledge store stalls.
///
/// Implementation:
///
/// 1. The per-source deadline is held at [`G3_PER_SOURCE_DEADLINE`]
///    **unconditionally** — it is NOT `min`'d with `master_deadline`.
///    Per WO-0068 lessons-learned (For executor #2 + For planner #3),
///    capping per-source by master collapses both timeouts on tight
///    masters and the inner per-source wins, hiding the master trip.
///    The 4.5 s / 5 s margin keeps per-source as the primary path
///    under default config (master = 5 s); tight-master cases (e.g.
///    100 ms test masters) let the master fire first deterministically.
/// 2. Each per-source future is wrapped in
///    `tokio::time::timeout(G3_PER_SOURCE_DEADLINE, ...)` via
///    [`run_g3_source`] which returns
///    [`G3SourceStatus::TimedOut`] on elapse.
/// 3. Build one boxed future per source and poll them concurrently
///    through [`join_all_g3`] (the same `poll_fn` fan-out shape as
///    `execute_g1` / `execute_cross_group`).
/// 4. Wrap the whole join in
///    `tokio::time::timeout(master_deadline, ...)`.  On `Err(Elapsed)`,
///    return a [`G3Outcome`] with [`G3SourceStatus::TimedOut`]
///    placeholders for every source and `master_timed_out = true` so
///    downstream code never sees an empty result vector when the
///    master deadline fires.
///
/// The orchestrator never `panic!`s and never `?` propagates an error
/// out — partial results are valid output per master-plan §5.3 +
/// §6.1 line 606.
///
/// Per master-plan §15.2, this orchestrator emits a `tracing` span
/// `ucil.group.knowledge` (parallel to `ucil.group.structural` for
/// G1).  The instrument decorator is appropriate here because
/// `execute_g3` is async/IO orchestration — unlike the deterministic
/// `ceqp::parse_reason` (WO-0067) which intentionally has no span.
///
/// # Examples
///
/// ```no_run
/// use std::time::Duration;
/// use ucil_daemon::g3::{
///     execute_g3, G3Query, G3Source, G3_MASTER_DEADLINE,
/// };
///
/// # async fn demo(sources: Vec<Box<dyn G3Source + Send + Sync + 'static>>) {
/// let q = G3Query {
///     entities: vec!["TaskManager".to_owned()],
///     max_results: 10,
///     min_confidence: 0.0,
/// };
/// let outcome = execute_g3(q, sources, G3_MASTER_DEADLINE).await;
/// assert!(!outcome.master_timed_out || !outcome.results.is_empty());
/// # }
/// ```
#[tracing::instrument(
    name = "ucil.group.knowledge",
    level = "debug",
    skip(sources),
    fields(source_count = sources.len()),
)]
pub async fn execute_g3(
    query: G3Query,
    sources: Vec<Box<dyn G3Source + Send + Sync + 'static>>,
    master_deadline: Duration,
) -> G3Outcome {
    // Step 1 + Step 2: start time + per-source deadline.
    //
    // The per-source deadline is held at [`G3_PER_SOURCE_DEADLINE`]
    // unconditionally so the master deadline ALWAYS wins on a tight
    // `master_deadline` (e.g. SA7 with 100 ms): when
    // `master_deadline < G3_PER_SOURCE_DEADLINE`, the outer
    // `tokio::time::timeout(master_deadline, ...)` fires first and the
    // master-trip path synthesises in-order [`G3SourceStatus::TimedOut`]
    // placeholders. Capping `per_source_deadline` by `master_deadline`
    // would race the two timers and let the inner per-source timeout
    // resolve the inner future first, hiding the master trip
    // (WO-0068 lessons-learned, For executor #2 + For planner #3).
    let start = std::time::Instant::now();
    let per_source_deadline = G3_PER_SOURCE_DEADLINE;

    // Step 3: build one boxed future per source and poll them
    // concurrently.  A `tokio::task::JoinSet` would also work but the
    // `poll_fn` fan-out keeps the cancellation semantics simple — when
    // the outer `master_deadline` fires the outer `timeout` wraps
    // everything together so unfinished futures are dropped, mirroring
    // the `execute_g1` + `execute_cross_group` patterns.
    let q_ref = &query;
    let mut futures: Vec<Pin<Box<dyn Future<Output = G3SourceOutput> + Send + '_>>> =
        Vec::with_capacity(sources.len());
    for s in &sources {
        futures.push(Box::pin(run_g3_source(
            s.as_ref(),
            q_ref,
            per_source_deadline,
        )));
    }

    // Step 4: outer master-deadline wrap.
    let outer = tokio::time::timeout(master_deadline, join_all_g3(futures)).await;
    let wall_elapsed_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

    // Step 5: preserve input order. On master-deadline trip, synthesise
    // `TimedOut` placeholders in input order so
    // `results[i].source_id == sources[i].source_id()` holds either way.
    outer.map_or_else(
        |_| {
            let results = sources
                .iter()
                .map(|s| G3SourceOutput {
                    source_id: s.source_id().to_owned(),
                    status: G3SourceStatus::TimedOut,
                    elapsed_ms: wall_elapsed_ms,
                    observations: vec![],
                    error: Some(format!(
                        "G3 master deadline {} ms elapsed",
                        master_deadline.as_millis()
                    )),
                })
                .collect();
            G3Outcome {
                results,
                wall_elapsed_ms,
                master_timed_out: true,
            }
        },
        |results| G3Outcome {
            results,
            wall_elapsed_ms,
            master_timed_out: false,
        },
    )
}

// ── Entity-keyed temporal-priority merger ─────────────────────────────────

/// Entity-keyed temporal-priority merger for G3 (Knowledge) outputs.
///
/// Master-plan §5.3 lines 473-477 prescribes the merge contract:
///
/// 1. Collect every [`G3FactObservation`] from sources whose status is
///    [`G3SourceStatus::Available`] (`Errored` / `TimedOut` sources
///    contribute zero observations).
/// 2. Group by `entity` field via a `BTreeMap` so output ordering is
///    deterministic.
/// 3. Within each entity group, partition observations by `fact` text
///    equality:
///    * **Agreement clusters** (same `fact`) — winner is highest
///      [`G3FactObservation::confidence`]; ties broken by newest
///      `observed_ts_ns`; further ties broken by lexicographic
///      `source_id`.
///    * **Conflict clusters** (different `fact`) — winner is newest
///      `observed_ts_ns`; ties broken by highest `confidence`; further
///      ties broken by lexicographic `source_id`.
/// 4. Emit one [`G3MergedFact`] per entity with `conflict_count` =
///    (distinct-fact-strings - 1) and `agreement_count` = (number of
///    observations supporting the winning fact).
///
/// **Distinct from RRF.**  This is entity-keyed temporal merge, not
/// the rank-based reciprocal-rank fusion that backs G2 (`fusion::*`)
/// and the cross-group layer (`cross_group::fuse_cross_group_rrf`).
///
/// **Graphiti temporal validity is NOT yet load-bearing.**  The
/// [`G3FactObservation::validity_window`] field is reserved for the
/// future F10 (Graphiti P1 plugin) integration where master-plan §5.3
/// line 475 ("Graphiti temporal validity wins") would override the
/// bare `observed_ts_ns` precedence.  The merge body retains the
/// observation but does not consume the field — the future hook
/// lands in the F10 production-wiring WO.
///
/// The merger is pure (no IO, no async, no logging).
///
/// # Examples
///
/// ```
/// use ucil_daemon::g3::{
///     merge_g3_by_entity, G3FactObservation, G3SourceOutput, G3SourceStatus,
/// };
///
/// let outputs = vec![G3SourceOutput {
///     source_id: "alpha".to_owned(),
///     status: G3SourceStatus::Available,
///     elapsed_ms: 42,
///     observations: vec![G3FactObservation {
///         source_id: "alpha".to_owned(),
///         entity: "TaskManager".to_owned(),
///         fact: "uses tokio".to_owned(),
///         confidence: 0.9,
///         observed_ts_ns: 100,
///         validity_window: None,
///     }],
///     error: None,
/// }];
/// let merged = merge_g3_by_entity(&outputs);
/// assert_eq!(merged.merged.len(), 1);
/// assert_eq!(merged.merged[0].winning_fact, "uses tokio");
/// ```
#[allow(clippy::too_many_lines)]
#[must_use]
pub fn merge_g3_by_entity(outputs: &[G3SourceOutput]) -> G3MergeOutcome {
    // Step 1: collect every observation from `Available` sources.
    //
    // Errored / TimedOut sources contribute zero observations — the
    // intent of master-plan §5.3 is that a single failing knowledge
    // store does not pollute the merged result.
    let mut total_observations_in: usize = 0;
    let mut by_entity: BTreeMap<String, Vec<G3FactObservation>> = BTreeMap::new();
    for output in outputs {
        if output.status != G3SourceStatus::Available {
            continue;
        }
        for obs in &output.observations {
            total_observations_in += 1;
            by_entity
                .entry(obs.entity.clone())
                .or_default()
                .push(obs.clone());
        }
    }

    // Step 2-4: per-entity resolution.
    let mut merged: Vec<G3MergedFact> = Vec::with_capacity(by_entity.len());
    for (entity, observations) in by_entity {
        // Partition by `fact` text equality.  Using a `BTreeMap` keyed
        // on the fact string keeps cluster iteration deterministic.
        let mut clusters: BTreeMap<String, Vec<G3FactObservation>> = BTreeMap::new();
        for obs in observations {
            clusters.entry(obs.fact.clone()).or_default().push(obs);
        }

        // `conflict_count` = distinct-fact-strings - 1 (zero when every
        // observation agreed on one fact text).
        //
        // The cast through `u32` is bounded by the number of
        // `G3FactObservation` structs in flight — which is bounded in
        // turn by `usize` — so the truncation cannot lose information
        // for any realistic workload.
        let conflict_count = u32::try_from(clusters.len().saturating_sub(1)).unwrap_or(u32::MAX);

        // Pick a winner per cluster — agreement-cluster semantics:
        //   * highest `confidence`
        //   * tie → newest `observed_ts_ns`
        //   * tie → lexicographic `source_id`
        //
        // Future Graphiti hook (master-plan §5.3 line 475 "Graphiti
        // temporal validity wins"): when [`G3FactObservation::
        // validity_window`] is `Some((start, end))` AND the query
        // timestamp falls inside `[start, end)`, that observation
        // would override the bare `observed_ts_ns` precedence.  F07
        // does NOT load-bear on this branch — the field is reserved
        // and the F10 production-wiring WO will land the override
        // pass.
        let mut cluster_winners: Vec<(G3FactObservation, u32)> = Vec::with_capacity(clusters.len());
        for (_fact, cluster) in clusters {
            let cluster_size = u32::try_from(cluster.len()).unwrap_or(u32::MAX);
            let mut winner: Option<G3FactObservation> = None;
            for obs in cluster {
                winner = Some(match winner {
                    None => obs,
                    Some(current) => {
                        // Agreement-cluster comparison:
                        //   1. higher confidence wins
                        //   2. tie → newer observed_ts_ns
                        //   3. tie → lexicographic source_id
                        if obs.confidence > current.confidence {
                            obs
                        } else if obs.confidence < current.confidence {
                            current
                        } else if obs.observed_ts_ns > current.observed_ts_ns {
                            obs
                        } else if obs.observed_ts_ns < current.observed_ts_ns {
                            current
                        } else if obs.source_id < current.source_id {
                            obs
                        } else {
                            current
                        }
                    }
                });
            }
            if let Some(w) = winner {
                cluster_winners.push((w, cluster_size));
            }
        }

        // Resolve cross-cluster (conflict) winner — conflict-cluster
        // semantics:
        //   * newest `observed_ts_ns` wins
        //   * tie → highest `confidence`
        //   * tie → lexicographic `source_id`
        //
        // Single-cluster entities (no conflict) fall through unchanged
        // — the `cluster_winners` vec carries one entry whose `obs`
        // is the agreement-cluster winner.
        let mut overall: Option<(G3FactObservation, u32)> = None;
        for (obs, agreement_count) in cluster_winners {
            overall = Some(match overall {
                None => (obs, agreement_count),
                Some((current, current_count)) => {
                    if obs.observed_ts_ns > current.observed_ts_ns {
                        (obs, agreement_count)
                    } else if obs.observed_ts_ns < current.observed_ts_ns {
                        (current, current_count)
                    } else if obs.confidence > current.confidence {
                        (obs, agreement_count)
                    } else if obs.confidence < current.confidence {
                        (current, current_count)
                    } else if obs.source_id < current.source_id {
                        (obs, agreement_count)
                    } else {
                        (current, current_count)
                    }
                }
            });
        }

        if let Some((winner, agreement_count)) = overall {
            merged.push(G3MergedFact {
                entity,
                winning_fact: winner.fact,
                winning_source_id: winner.source_id,
                winning_confidence: winner.confidence,
                winning_ts_ns: winner.observed_ts_ns,
                conflict_count,
                agreement_count,
            });
        }
    }

    let total_entities_out = merged.len();
    G3MergeOutcome {
        merged,
        total_observations_in,
        total_entities_out,
    }
}
