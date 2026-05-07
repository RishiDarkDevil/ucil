//! Cross-group orchestration brain for UCIL Phase 3 Week 9.
//!
//! Master-plan §4 lines 113-116 freezes the 8 tool groups (G1..G8).
//! Master-plan §6.1 lines 585-641 places the cross-group fan-out in
//! the Parallel Executor step of the query pipeline; this module's
//! [`execute_cross_group`] is the orchestration shell for that step.
//! Master-plan §6.2 lines 643-658 freezes the 10 × 8 query-type
//! weight matrix; this module's [`fuse_cross_group`] consumes
//! [`crate::fusion::group_weights_for`] (P3-W9-F01 data table)
//! unmodified to compute weighted Reciprocal Rank Fusion (`RRF`)
//! scores `Σ_g w_g(query_type) × 1 / (k + rank_g(d))` per master-
//! plan §6.2 line 645 (k = 60 default, tunable).
//!
//! No production wiring of real `GroupExecutor` impls lives here —
//! `G1Adapter` / `G2Adapter` / G3..G8 adapters land in `ucil-daemon`
//! follow-up WOs to avoid an `ucil-core` → `ucil-daemon` cycle.
//! This module is the dependency-inversion seam (`GroupExecutor`
//! trait) plus the fan-out + fusion shell that production wiring
//! plugs into.
//!
//! Feature anchors: P3-W9-F03 (parallel executor + degraded-groups
//! aggregation per §6.1 line 606) and P3-W9-F04 (cross-group
//! weighted RRF fusion per §6.2 lines 643-658).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use crate::fusion::{group_weights_for, QueryType};

// ── Group enum ────────────────────────────────────────────────────────────────

/// One of the 8 UCIL tool groups per master-plan §4 lines 113-116.
///
/// Variant declaration order MUST match master-plan §6.2 column
/// order verbatim (`[G1, G2, G3, G4, G5, G6, G7, G8]`) so that
/// `Group as usize` indexes the §6.2 weight-matrix row correctly
/// (verified by [`fuse_cross_group`] and the frozen test
/// `test_cross_group_rrf_fusion`).
///
/// `Default` is [`Group::G1`] — the most-permissive default per
/// master-plan §3.2 row 1 fallback (`understand_code → G1, G3,
/// G5`). The 8 variants are frozen by master-plan §4 / §6.2;
/// adding a 9th group requires a master-plan amendment and an ADR.
///
/// `serde(rename_all = "snake_case")` produces the JSON wire labels
/// `"g1"`, `"g2"`, … `"g8"`.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    /// Structural: tree-sitter, Serena, ast-grep, LSP diagnostics,
    /// SCIP — master-plan §4 line 113. Default per §3.2 row 1
    /// fallback (`understand_code → G1, G3, G5`).
    #[default]
    G1,
    /// Search: ripgrep, Probe, `LanceDB`, Zoekt, codedb — master-plan
    /// §4 line 113.
    G2,
    /// Knowledge: Codebase-Memory MCP, Mem0, Graphiti — master-plan
    /// §4 line 114.
    G3,
    /// Architecture: `CodeGraphContext`, dependency graphs, blast-
    /// radius — master-plan §4 line 114.
    G4,
    /// Conventions: ucil's own convention learner (rules + counter-
    /// examples) — master-plan §4 line 115.
    G5,
    /// Bonus context: relevant tests, recent diffs, ADRs —
    /// master-plan §4 line 115.
    G6,
    /// Quality: lint, type-check, security scan, complexity —
    /// master-plan §4 line 115.
    G7,
    /// Diff/Review: change-aware analysis (PR review, blast-radius
    /// of diffs) — master-plan §4 line 116.
    G8,
}

// ── Group status ──────────────────────────────────────────────────────────────

/// Disposition of one group's executor on a [`execute_cross_group`]
/// fan-out call.
///
/// Mirrors the existing `G1ToolStatus` shape in
/// `crates/ucil-daemon/src/executor.rs`. `Default` is
/// [`GroupStatus::Unavailable`] — the safest pre-execution status
/// (any code path that constructs a [`GroupResult`] without running
/// an executor MUST surface its degraded state in
/// [`CrossGroupExecution::degraded_groups`]).
///
/// `serde(rename_all = "snake_case")` produces JSON labels
/// `"available"`, `"timed_out"`, `"errored"`, `"unavailable"`.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum GroupStatus {
    /// The executor returned a payload within its per-group
    /// deadline.
    Available,
    /// The executor's per-group `tokio::time::timeout` elapsed
    /// before it returned a response.
    TimedOut,
    /// The executor returned an error (transport / decode /
    /// internal).
    Errored,
    /// The group is degraded or not installed in this deployment
    /// (e.g. plugin absent, MCP server disabled). The safest pre-
    /// execution default per master-plan §6.1 line 606 (degraded-
    /// groups _meta surface).
    #[default]
    Unavailable,
}

// ── Hit / result / query types ────────────────────────────────────────────────

/// A single per-group hit emitted by a [`GroupExecutor`].
///
/// Lines are 1-based; `start_line == end_line` is permitted. The
/// `score` is the per-group raw score (NOT the fused RRF score).
/// Same shape contract as [`crate::fusion::G2Hit`].
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GroupHit {
    /// Path to the file containing the hit.
    pub file_path: PathBuf,
    /// Inclusive 1-based start line.
    pub start_line: u32,
    /// Inclusive 1-based end line.
    pub end_line: u32,
    /// Rendered text excerpt from the originating group.
    pub snippet: String,
    /// Per-group raw score (not the fused RRF score).
    pub score: f64,
}

/// One group's contribution to a [`execute_cross_group`] call.
///
/// `hits[0]` is rank 1, `hits[1]` is rank 2, etc. The
/// [`fuse_cross_group`] consumer treats `idx + 1` as the 1-based
/// rank in the RRF formula.
///
/// Invariant per master-plan §6.1 line 606: when [`Self::status`]
/// is anything other than [`GroupStatus::Available`] the
/// `error.is_some()` MUST hold so operators can read why the group
/// is degraded; conversely, [`GroupStatus::Available`] MUST have
/// `error: None`.
///
/// `Default` produces `{ group: G1, status: Unavailable, hits:
/// vec![], elapsed_ms: 0, error: None }` — useful for empty test
/// inputs.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GroupResult {
    /// Which group produced this result.
    pub group: Group,
    /// Disposition of the group on this fan-out call.
    pub status: GroupStatus,
    /// Already-ranked hits — `hits[0]` is rank 1.
    pub hits: Vec<GroupHit>,
    /// Wall-clock time the group spent before returning, in
    /// milliseconds.
    pub elapsed_ms: u64,
    /// Operator-readable error description for any non-`Available`
    /// status. `None` for [`GroupStatus::Available`].
    pub error: Option<String>,
}

/// Minimal cross-group input shape consumed by a [`GroupExecutor`].
///
/// Additional fields (e.g. `files_in_context`, `current_task`)
/// land via additive non-breaking changes in follow-up WOs that
/// wire the daemon orchestration loop through the cross-group
/// executor — master-plan §3.2 line 209 (CEQP-universal-params
/// expansion). The 3-field minimal shape is the proof-of-concept
/// fan-out input.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CrossGroupQuery {
    /// MCP tool name (e.g. `"find_definition"`, `"search_code"`).
    pub tool_name: String,
    /// Symbol / pattern / target of the query.
    pub target: String,
    /// Free-form reason keywords lifted from the CEQP `reason`
    /// field — see master-plan §3.2 lines 211-237 and
    /// `crate::ceqp::parse_reason`.
    pub reason_keywords: Vec<String>,
}

// ── GroupExecutor trait ───────────────────────────────────────────────────────

/// Dependency-inversion seam for one of the 8 UCIL tool groups.
///
/// Per `DEC-0008` §4 this trait is UCIL-owned — it is **not** a
/// re-export or adapter of any external wire format. Same pattern
/// as `G1Source` in `crates/ucil-daemon/src/executor.rs`. Live
/// `GroupExecutor` impls (`G1Adapter` wrapping `execute_g1` / `fuse_g1`,
/// `G2Adapter` wrapping `fuse_g2_rrf`, G3..G8 adapters bound to
/// plugin-manager-managed plugins) land in `ucil-daemon` follow-up
/// WOs to avoid an `ucil-core` → `ucil-daemon` cycle.
///
/// `Send + Sync` bounds are required so trait objects can live in
/// `Vec<Box<dyn GroupExecutor + Send + Sync + 'static>>` inside the
/// daemon's long-lived server state once production adapters land.
#[async_trait::async_trait]
pub trait GroupExecutor: Send + Sync {
    /// Identifies this executor's [`Group`] without runtime
    /// introspection so [`execute_cross_group`] can label results
    /// by group.
    fn group(&self) -> Group;

    /// Run this group's query.
    ///
    /// Implementations are responsible for emitting their own
    /// [`GroupResult`] with the appropriate [`GroupStatus`] — the
    /// orchestrator only overrides the status to
    /// [`GroupStatus::TimedOut`] when its per-group
    /// `tokio::time::timeout` elapses.
    async fn execute(&self, query: &CrossGroupQuery) -> GroupResult;
}

// ── Aggregate execution outcome ───────────────────────────────────────────────

/// Aggregate outcome of one [`execute_cross_group`] fan-out call.
///
/// `results` is in input executor order: `results[i].group ==
/// executors[i].group()`. `master_timed_out` is `true` iff the
/// outer `tokio::time::timeout` elapsed before all per-group
/// futures completed; in that case `results` carries
/// [`GroupStatus::TimedOut`] placeholders for every executor that
/// had not yet completed so downstream code never sees an empty-
/// but-non-`master_timed_out` outcome.
///
/// `degraded_groups` contains every [`Group`] whose
/// [`GroupResult::status`] is anything other than
/// [`GroupStatus::Available`], in the same order they appear in
/// `results`. Surfaces in `_meta.degraded_groups` per master-plan
/// §6.1 line 606.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CrossGroupExecution {
    /// Per-group results, in the same order as the input executor
    /// slice.
    pub results: Vec<GroupResult>,
    /// `true` iff the outer master deadline elapsed before all
    /// per-group futures completed.
    pub master_timed_out: bool,
    /// Wall-clock time the orchestrator spent in milliseconds.
    pub wall_elapsed_ms: u64,
    /// Subset of [`Group`]s whose status is not
    /// [`GroupStatus::Available`] — surfaces in
    /// `_meta.degraded_groups`.
    pub degraded_groups: Vec<Group>,
}

// ── Constants ─────────────────────────────────────────────────────────────────

/// Master timeout for the cross-group fan-out per master-plan §6.1
/// line 606 ("Per-group timeout: 5s default").
///
/// When this deadline elapses, [`execute_cross_group`] returns a
/// [`CrossGroupExecution`] with `master_timed_out = true` and per-
/// group [`GroupStatus::TimedOut`] placeholders for any executor
/// that had not yet completed.
pub const CROSS_GROUP_MASTER_DEADLINE: Duration = Duration::from_millis(5_000);

/// Per-group timeout applied to each [`GroupExecutor::execute`]
/// call.
///
/// 4.5 s leaves a 0.5 s margin under [`CROSS_GROUP_MASTER_DEADLINE`]
/// so the per-group timeout always wins on a true global stall —
/// the master deadline is a safety net, not the primary timing
/// path. Mirrors the rationale + timing relationship documented
/// in `crates/ucil-daemon/src/executor.rs` for
/// `G1_PER_SOURCE_DEADLINE`.
pub const CROSS_GROUP_PER_GROUP_DEADLINE: Duration = Duration::from_millis(4_500);

/// `RRF` `k` parameter for cross-group fusion per master-plan §6.2
/// line 645 ("k = 60 (tunable)").
///
/// Named DISTINCTLY from the existing [`crate::fusion::G2_RRF_K`]
/// because §6.2 (cross-group) and §5.2 (intra-G2) are independent
/// contracts — a future tuning change to one MUST NOT silently
/// propagate to the other. Typed as `u32` (not `usize`) so the
/// lossless `f64::from(...)` cast inside the fusion formula is
/// unambiguous on 32-bit and 64-bit targets identically.
pub const CROSS_GROUP_RRF_K: u32 = 60;

// ── Cross-group orchestrator ──────────────────────────────────────────────────

/// Run one [`GroupExecutor::execute`] call under a per-group
/// timeout, converting a per-group timeout into a
/// [`GroupStatus::TimedOut`] [`GroupResult`] without ever
/// panicking.
///
/// Mirrors `run_g1_source` in `crates/ucil-daemon/src/executor.rs`
/// adapted to the cross-group [`GroupResult`] shape.
async fn run_group_executor(
    executor: &(dyn GroupExecutor + Send + Sync),
    query: &CrossGroupQuery,
    per_group_deadline: Duration,
) -> GroupResult {
    let group = executor.group();
    let start = std::time::Instant::now();
    tokio::time::timeout(per_group_deadline, executor.execute(query))
        .await
        .unwrap_or_else(|_| {
            let elapsed_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
            GroupResult {
                group,
                status: GroupStatus::TimedOut,
                hits: vec![],
                elapsed_ms,
                error: Some(format!(
                    "per-group deadline {} ms exceeded",
                    per_group_deadline.as_millis()
                )),
            }
        })
}

/// Cross-group parallel-execution orchestrator.
///
/// Master-plan §6.1 lines 585-641 places this function under the
/// Parallel Executor step of the query pipeline. The 7-step
/// implementation contract (per `scope_in` #12 of WO-0068):
///
/// 1. Record start time via `tokio::time::Instant::now()`.
/// 2. Hold `per_group_deadline = CROSS_GROUP_PER_GROUP_DEADLINE`
///    unconditionally so the master deadline always wins on a
///    tight `master_deadline` (without a cap, the inner per-group
///    timeout would race the master timer and hide the master
///    trip). The 4.5 s/5 s margin keeps per-group as the primary
///    path under default config (master = 5 s).
/// 3. Spawn each executor's `execute(&query)` future under
///    `tokio::time::timeout(per_group_deadline, ...)` via
///    `tokio::task::JoinSet`-equivalent concurrent polling.
/// 4. Wrap the entire collection in
///    `tokio::time::timeout(master_deadline, ...)` so a global
///    stall yields `master_timed_out = true` with
///    [`GroupStatus::TimedOut`] placeholders for every executor.
/// 5. Preserve input executor order in `results` (so
///    `results[i].group == executors[i].group()`).
/// 6. Compute `degraded_groups` after `results` is finalised.
/// 7. `wall_elapsed_ms = start.elapsed().as_millis() as u64`.
///
/// The orchestrator never `panic!`s and never `?` propagates an
/// error out — partial results are valid output per master-plan
/// §6.1 line 606.
///
/// # Examples
///
/// ```no_run
/// use std::time::Duration;
/// use ucil_core::cross_group::{
///     execute_cross_group, CrossGroupQuery, GroupExecutor,
///     CROSS_GROUP_MASTER_DEADLINE,
/// };
///
/// # async fn demo(executors: Vec<Box<dyn GroupExecutor + Send + Sync>>) {
/// let q = CrossGroupQuery {
///     tool_name: "find_definition".to_owned(),
///     target: "TaskManager".to_owned(),
///     reason_keywords: vec![],
/// };
/// let outcome = execute_cross_group(q, executors, CROSS_GROUP_MASTER_DEADLINE).await;
/// assert!(!outcome.master_timed_out || !outcome.results.is_empty());
/// # }
/// ```
#[tracing::instrument(
    name = "ucil.daemon.classify_and_dispatch",
    level = "debug",
    skip(executors),
    fields(
        query_target = %query.target,
        executor_count = executors.len(),
    ),
)]
pub async fn execute_cross_group(
    query: CrossGroupQuery,
    executors: Vec<Box<dyn GroupExecutor + Send + Sync>>,
    master_deadline: Duration,
) -> CrossGroupExecution {
    // Step 1 + Step 2: start time + per-group deadline.
    //
    // The per-group deadline is held at [`CROSS_GROUP_PER_GROUP_DEADLINE`]
    // unconditionally so the master deadline ALWAYS wins on a tight
    // `master_deadline` (e.g. SA4 with 100 ms): when
    // `master_deadline < CROSS_GROUP_PER_GROUP_DEADLINE`, the outer
    // `tokio::time::timeout(master_deadline, ...)` fires first and the
    // master-trip path synthesises in-order [`GroupStatus::TimedOut`]
    // placeholders. Capping `per_group_deadline` by `master_deadline`
    // would race the two timers and let the inner per-group timeout
    // resolve the inner future first, hiding the master trip.
    // Master-plan §6.1 line 606: "Per-group timeout: 5s default; master
    // deadline is the safety net". The 4.5 s/5 s margin (per-group <
    // master under default config) keeps per-group as the primary
    // path; tight-master cases let master win deterministically.
    let start = std::time::Instant::now();
    let per_group_deadline = CROSS_GROUP_PER_GROUP_DEADLINE;

    // Step 3: build one boxed future per executor and poll them
    // concurrently through `join_all_cross_group`. A `JoinSet`
    // would also work but the poll-fn fan-out keeps the cancellation
    // semantics simple — when the outer `master_deadline` fires the
    // outer `timeout` wraps everything together so unfinished
    // futures are dropped, mirroring the `execute_g1` pattern.
    let q_ref = &query;
    let mut futures: Vec<
        std::pin::Pin<Box<dyn std::future::Future<Output = GroupResult> + Send + '_>>,
    > = Vec::with_capacity(executors.len());
    for ex in &executors {
        futures.push(Box::pin(run_group_executor(
            ex.as_ref(),
            q_ref,
            per_group_deadline,
        )));
    }

    // Step 4: outer master-deadline wrap.
    let outer = tokio::time::timeout(master_deadline, join_all_cross_group(futures)).await;
    let wall_elapsed_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

    // Step 5: preserve input order. On master-deadline trip,
    // synthesise `TimedOut` placeholders in input order so
    // `results[i].group == executors[i].group()` holds either way.
    let (results, master_timed_out) = outer.map_or_else(
        |_| {
            let placeholder: Vec<GroupResult> = executors
                .iter()
                .map(|ex| GroupResult {
                    group: ex.group(),
                    status: GroupStatus::TimedOut,
                    hits: vec![],
                    elapsed_ms: wall_elapsed_ms,
                    error: Some(format!(
                        "cross-group master deadline {} ms elapsed",
                        master_deadline.as_millis()
                    )),
                })
                .collect();
            (placeholder, true)
        },
        |results| (results, false),
    );

    // Step 6: compute `degraded_groups` AFTER `results` is final.
    let degraded_groups: Vec<Group> = results
        .iter()
        .filter(|r| r.status != GroupStatus::Available)
        .map(|r| r.group)
        .collect();

    // Step 7: assemble the outcome.
    CrossGroupExecution {
        results,
        master_timed_out,
        wall_elapsed_ms,
        degraded_groups,
    }
}

/// Poll a `Vec` of pinned-boxed futures concurrently and collect
/// every output once all are ready.
///
/// Behaviourally equivalent to `futures::future::join_all` but
/// avoids pulling the `futures` crate as a dependency (per
/// `scope_in` #42 — `tokio` ships everything we need for an 8-way
/// fan-out). Same pattern as `join_all_g1` in
/// `crates/ucil-daemon/src/executor.rs`.
async fn join_all_cross_group<'a, T>(
    mut futures: Vec<std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>>,
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
                std::task::Poll::Ready(out) => {
                    slots[i] = Some(out);
                }
                std::task::Poll::Pending => {
                    any_pending = true;
                }
            }
        }
        if any_pending {
            std::task::Poll::Pending
        } else {
            std::task::Poll::Ready(())
        }
    })
    .await;
    slots
        .into_iter()
        .map(|r| r.expect("join_all_cross_group: every slot must be filled before returning"))
        .collect()
}

// ── Cross-group RRF fusion ────────────────────────────────────────────────────

/// A single fused hit emitted by [`fuse_cross_group`].
///
/// `contributing_groups` is sorted by descending §6.2 weight (ties
/// broken by ascending `Group as usize`), so a reader can spot the
/// highest-weight contributor at index 0. `per_group_ranks` is
/// `(group, rank)` pairs preserving the per-group rank that the
/// originating group's [`GroupResult::hits`] index assigned to
/// this location — provenance for downstream `_meta` consumers.
/// Same shape contract as [`crate::fusion::G2FusedHit`].
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CrossGroupFusedHit {
    /// Path to the file.
    pub file_path: PathBuf,
    /// Inclusive 1-based start line.
    pub start_line: u32,
    /// Inclusive 1-based end line.
    pub end_line: u32,
    /// Snippet from the highest-weight contributing group.
    pub snippet: String,
    /// Fused `RRF` score: `Σ_g w_g(query_type) × 1 / (k + rank_g)`.
    pub fused_score: f64,
    /// Groups that contributed to this location, sorted by
    /// descending §6.2 weight (ties broken by ascending
    /// `Group as usize`).
    pub contributing_groups: Vec<Group>,
    /// `(group, rank)` pairs preserving the per-group rank
    /// assigned to this location — provenance for downstream
    /// consumers.
    pub per_group_ranks: Vec<(Group, u32)>,
}

/// Fused output of [`fuse_cross_group`].
///
/// `hits` is sorted descending by `fused_score`, with
/// `(file_path, start_line, end_line)` ascending as the
/// deterministic tie-break. `used_weights` is the snapshot of
/// `group_weights_for(query_type)` so consumers can see exactly
/// which §6.2 row drove the ranking — the canary against matrix-
/// row-shift bugs (the §6.2 sentinel row Remember = `[0, 0, 3.0,
/// 0, 0, 0, 0, 0]` is the most diagnostic). `degraded_groups`
/// passes through verbatim from
/// [`CrossGroupExecution::degraded_groups`] so downstream `_meta`
/// surfaces stay coherent across the executor → fusion boundary.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CrossGroupFusedOutcome {
    /// Fused hits sorted descending by `fused_score`. `hits[0]`
    /// is the top result.
    pub hits: Vec<CrossGroupFusedHit>,
    /// Snapshot of `group_weights_for(query_type)` — the §6.2
    /// row that drove the ranking.
    pub used_weights: [f32; 8],
    /// Groups whose [`GroupResult::status`] is not
    /// [`GroupStatus::Available`] — passes through verbatim from
    /// [`CrossGroupExecution::degraded_groups`].
    pub degraded_groups: Vec<Group>,
}

/// Cross-group weighted Reciprocal Rank Fusion (`RRF`) per master-
/// plan §6.2 lines 643-658.
///
/// The 7-step implementation contract (per `scope_in` #16 of
/// WO-0068):
///
/// 1. Snapshot `let weights = group_weights_for(query_type)` —
///    consumed unmodified from the P3-W9-F01 data table.
/// 2. For each [`GroupResult`] with `status == Available`, iterate
///    `hits` with 1-based rank; for each hit, compute the per-
///    `(group, file_path, start_line, end_line)` `RRF`
///    contribution
///    `(weights[group as usize] as f64) /
///    ((CROSS_GROUP_RRF_K as f64) + (rank as f64))`.
/// 3. Accumulate contributions into a `BTreeMap<(PathBuf, u32,
///    u32), CrossGroupFusedHit>` keyed by location.
/// 4. The snippet is taken from the highest-§6.2-weight
///    contributor (ties broken by ascending `Group as usize`).
/// 5. `contributing_groups` and `per_group_ranks` populated as
///    above.
/// 6. Sort the final `Vec<CrossGroupFusedHit>` descending by
///    `fused_score`, ties broken by ascending `(file_path,
///    start_line, end_line)`.
/// 7. `degraded_groups` passes through verbatim.
///
/// Per master-plan §6.3 line 667 ("results below relevance
/// threshold (score < 0.1) excluded") this implementation uses
/// the threshold-of-zero contract: `fused_score == 0.0` hits ARE
/// excluded. This is the documented contract for the §6.2
/// Remember sentinel row — a hit that contributes only to a zero-
/// weight group never appears in `hits`.
///
/// The function is pure deterministic math: no IO, no async, no
/// logging. It never `panic!`s and never returns a `Result` —
/// fusion over an empty execution is just an empty
/// [`CrossGroupFusedOutcome`].
///
/// # Examples
///
/// ```
/// use std::path::PathBuf;
/// use ucil_core::cross_group::{
///     fuse_cross_group, CrossGroupExecution, Group, GroupHit, GroupResult,
///     GroupStatus,
/// };
/// use ucil_core::fusion::QueryType;
///
/// let g1 = GroupResult {
///     group: Group::G1,
///     status: GroupStatus::Available,
///     hits: vec![GroupHit {
///         file_path: PathBuf::from("foo.rs"),
///         start_line: 10,
///         end_line: 20,
///         snippet: "fn foo() // g1".to_owned(),
///         score: 0.8,
///     }],
///     elapsed_ms: 5,
///     error: None,
/// };
/// let exec = CrossGroupExecution {
///     results: vec![g1],
///     master_timed_out: false,
///     wall_elapsed_ms: 5,
///     degraded_groups: vec![],
/// };
///
/// let outcome = fuse_cross_group(&exec, QueryType::FindDefinition);
/// assert_eq!(outcome.hits.len(), 1);
/// // §6.2 line 650: find_definition row, G1 weight = 3.0
/// assert!((outcome.hits[0].fused_score - 3.0_f64 / 61.0).abs() < 1e-9);
/// ```
#[must_use]
pub fn fuse_cross_group(
    execution: &CrossGroupExecution,
    query_type: QueryType,
) -> CrossGroupFusedOutcome {
    // Step 1: snapshot the §6.2 weight row for `query_type`.
    let weights = group_weights_for(query_type);

    // Step 2 + 3: accumulate per-(file, start, end) contributions.
    // `BTreeMap` (not `HashMap`) so iteration order is the
    // deterministic location-key ordering, eliminating an end-of-
    // pass sort on the location key.
    #[allow(clippy::type_complexity)]
    let mut groups: BTreeMap<(PathBuf, u32, u32), Vec<(Group, u32, String)>> = BTreeMap::new();
    for result in &execution.results {
        if result.status != GroupStatus::Available {
            continue;
        }
        for (idx, hit) in result.hits.iter().enumerate() {
            // 1-based rank. `try_from` defends against the
            // (unreachable in practice) case of more than
            // `u32::MAX` hits.
            let rank: u32 = u32::try_from(idx + 1).unwrap_or(u32::MAX);
            let key = (hit.file_path.clone(), hit.start_line, hit.end_line);
            groups
                .entry(key)
                .or_default()
                .push((result.group, rank, hit.snippet.clone()));
        }
    }

    // Step 2 (contribution sum) + 4 (snippet pick) + 5 (provenance
    // populate). Per-location fusion.
    let mut hits: Vec<CrossGroupFusedHit> = Vec::with_capacity(groups.len());
    for ((file_path, start_line, end_line), contributors) in groups {
        // Step 2: per-group RRF contribution sum. The cast `Group
        // as usize` indexes the §6.2 row directly — adding a 9th
        // group requires a master-plan amendment + ADR per the
        // doc-comment on `Group`.
        let fused_score: f64 = contributors
            .iter()
            .map(|(g, rank, _)| {
                let w = f64::from(weights[*g as usize]);
                w * (1.0_f64 / (f64::from(CROSS_GROUP_RRF_K) + f64::from(*rank)))
            })
            .sum();

        // Per master-plan §6.3 line 667 + scope_in #32 contract:
        // `fused_score == 0.0` hits are excluded. The Remember
        // sentinel row (G1 weight 0.0) is the canonical case.
        if fused_score == 0.0 {
            continue;
        }

        // `per_group_ranks` in input encounter order — preserves
        // the per-group order for downstream consumers that want
        // to know "which group ranked this where".
        let per_group_ranks: Vec<(Group, u32)> = contributors
            .iter()
            .map(|(g, rank, _)| (*g, *rank))
            .collect();

        // Step 4 (preparation): `contributing_groups` sorted by
        // descending §6.2 weight, ascending `Group as usize` for
        // ties. `partial_cmp` over the {0.0, 0.5, 1.0, 1.5, 2.0,
        // 2.5, 3.0} weight set is total (no NaN possible from
        // `group_weights_for`); the `unwrap_or(Equal)` is
        // defensive.
        let mut contributing_groups: Vec<Group> = contributors.iter().map(|(g, _, _)| *g).collect();
        contributing_groups.sort_by(|a, b| {
            let weight_a = weights[*a as usize];
            let weight_b = weights[*b as usize];
            weight_b
                .partial_cmp(&weight_a)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.cmp(b))
        });

        // Step 4: snippet from the highest-weight contributing
        // group. `contributing_groups` is non-empty because
        // `contributors` is non-empty by `BTreeMap` construction
        // (we only created a group when we pushed into it).
        let top_group = contributing_groups[0];
        let snippet = contributors
            .iter()
            .find(|(g, _, _)| *g == top_group)
            .map_or_else(String::new, |(_, _, s)| s.clone());

        hits.push(CrossGroupFusedHit {
            file_path,
            start_line,
            end_line,
            snippet,
            fused_score,
            contributing_groups,
            per_group_ranks,
        });
    }

    // Step 6: sort hits descending by `fused_score`, ties broken
    // by ascending `(file_path, start_line, end_line)`. NaN-safe
    // — the formula yields only non-negative-finite values over
    // the {0.0..3.0} weight set.
    hits.sort_by(|a, b| {
        b.fused_score
            .partial_cmp(&a.fused_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.file_path.cmp(&b.file_path))
            .then_with(|| a.start_line.cmp(&b.start_line))
            .then_with(|| a.end_line.cmp(&b.end_line))
    });

    // Step 7: assemble the outcome.
    CrossGroupFusedOutcome {
        hits,
        used_weights: weights,
        degraded_groups: execution.degraded_groups.clone(),
    }
}

// ── Frozen tests ──────────────────────────────────────────────────────────────
//
// Tests live at module root per DEC-0007 (frozen-test-at-module-root) so
// the verifier selector `cargo test -p ucil-core
// cross_group::test_cross_group_parallel_execution` substring-matches the
// full test path `cross_group::test_cross_group_parallel_execution`
// (NOT `cross_group::tests::test_...`). Same shape as
// `executor::test_g1_parallel_execution` in `crates/ucil-daemon`.
// Test helpers (per-impl doubles, value builders) carry
// `#[cfg(test)]` so they vanish from the production build per
// scope_in #37 word-ban exemption.

#[cfg(test)]
struct AvailableExec {
    group: Group,
    hit: GroupHit,
}

#[cfg(test)]
#[async_trait::async_trait]
impl GroupExecutor for AvailableExec {
    fn group(&self) -> Group {
        self.group
    }
    async fn execute(&self, _query: &CrossGroupQuery) -> GroupResult {
        GroupResult {
            group: self.group,
            status: GroupStatus::Available,
            hits: vec![self.hit.clone()],
            elapsed_ms: 5,
            error: None,
        }
    }
}

#[cfg(test)]
struct SleepingExec {
    group: Group,
    sleep: Duration,
}

#[cfg(test)]
#[async_trait::async_trait]
impl GroupExecutor for SleepingExec {
    fn group(&self) -> Group {
        self.group
    }
    async fn execute(&self, _query: &CrossGroupQuery) -> GroupResult {
        tokio::time::sleep(self.sleep).await;
        GroupResult {
            group: self.group,
            status: GroupStatus::Available,
            hits: vec![],
            elapsed_ms: u64::try_from(self.sleep.as_millis()).unwrap_or(u64::MAX),
            error: None,
        }
    }
}

#[cfg(test)]
struct ErroringExec {
    group: Group,
}

#[cfg(test)]
#[async_trait::async_trait]
impl GroupExecutor for ErroringExec {
    fn group(&self) -> Group {
        self.group
    }
    async fn execute(&self, _query: &CrossGroupQuery) -> GroupResult {
        GroupResult {
            group: self.group,
            status: GroupStatus::Errored,
            hits: vec![],
            elapsed_ms: 1,
            error: Some("boom".to_owned()),
        }
    }
}

#[cfg(test)]
fn dummy_query() -> CrossGroupQuery {
    CrossGroupQuery {
        tool_name: "find_definition".to_owned(),
        target: "TaskManager".to_owned(),
        reason_keywords: vec![],
    }
}

#[cfg(test)]
fn dummy_hit(path: &str, line: u32) -> GroupHit {
    GroupHit {
        file_path: PathBuf::from(path),
        start_line: line,
        end_line: line,
        snippet: format!("snippet_{path}_{line}"),
        score: 0.5,
    }
}

// ── F03 frozen test ──────────────────────────────────────────────

/// Frozen acceptance test for P3-W9-F03 (cross-group parallel
/// executor). Master-plan §6.1 lines 585-641 + line 606.
///
/// Sub-assertions (DEC-0007 SA-numbered panic messages):
/// - SA1: All-available fan-out — order, count, master flag,
///   `degraded_groups` empty.
/// - SA2: One executor times out — `GroupStatus::TimedOut`,
///   `degraded_groups` includes the slow group, master flag
///   stays false (per-group cuts in first).
/// - SA3: One executor errors — `GroupStatus::Errored`,
///   `degraded_groups` includes it.
/// - SA4: Master-deadline trip — `master_timed_out == true`,
///   wall time < 2 s.
/// - SA5: Empty executors — empty results, no hang.
/// - SA6: Order preservation — `results[i].group ==
///   executors[i].group()` for non-canonical input order.
/// - SA7: JSON round-trip — `serde_json::from_str ==
///   to_string` on a `CrossGroupExecution`.
#[cfg(test)]
#[tokio::test]
#[allow(clippy::too_many_lines, clippy::missing_panics_doc)] // DEC-0005 module-coherence: SA1..SA7 are one cohesive unit.
pub async fn test_cross_group_parallel_execution() {
    // ── SA1: all-available fan-out ──────────────────────
    let executors: Vec<Box<dyn GroupExecutor + Send + Sync>> = vec![
        Box::new(AvailableExec {
            group: Group::G1,
            hit: dummy_hit("a.rs", 1),
        }),
        Box::new(AvailableExec {
            group: Group::G2,
            hit: dummy_hit("b.rs", 2),
        }),
        Box::new(AvailableExec {
            group: Group::G3,
            hit: dummy_hit("c.rs", 3),
        }),
        Box::new(AvailableExec {
            group: Group::G4,
            hit: dummy_hit("d.rs", 4),
        }),
    ];
    let outcome = execute_cross_group(dummy_query(), executors, CROSS_GROUP_MASTER_DEADLINE).await;
    assert_eq!(
        outcome.results.len(),
        4,
        "(SA1) all-available results.len; left: {}; right: 4",
        outcome.results.len()
    );
    assert!(
        !outcome.master_timed_out,
        "(SA1) all-available master_timed_out; expected false, got true"
    );
    assert!(
        outcome.degraded_groups.is_empty(),
        "(SA1) all-available degraded_groups; expected empty, got {:?}",
        outcome.degraded_groups
    );
    assert_eq!(
        outcome.results.iter().map(|r| r.group).collect::<Vec<_>>(),
        vec![Group::G1, Group::G2, Group::G3, Group::G4],
        "(SA1) all-available result order"
    );

    let sa1_outcome = outcome;

    // ── SA2: one executor times out (per-group deadline) ──
    let slow_executors: Vec<Box<dyn GroupExecutor + Send + Sync>> = vec![
        Box::new(AvailableExec {
            group: Group::G1,
            hit: dummy_hit("a.rs", 1),
        }),
        Box::new(SleepingExec {
            group: Group::G2,
            sleep: Duration::from_secs(6),
        }),
        Box::new(AvailableExec {
            group: Group::G3,
            hit: dummy_hit("c.rs", 3),
        }),
    ];
    let start = tokio::time::Instant::now();
    let outcome =
        execute_cross_group(dummy_query(), slow_executors, CROSS_GROUP_MASTER_DEADLINE).await;
    let elapsed = start.elapsed();
    assert_eq!(
        outcome.results[1].status,
        GroupStatus::TimedOut,
        "(SA2) timeout status; left: {:?}; right: TimedOut",
        outcome.results[1].status
    );
    assert_eq!(
        outcome.results[1].group,
        Group::G2,
        "(SA2) timeout group ordering; left: {:?}; right: G2",
        outcome.results[1].group
    );
    assert!(
        outcome.results[1].error.is_some(),
        "(SA2) timeout error message present"
    );
    assert!(
        outcome.degraded_groups.contains(&Group::G2),
        "(SA2) degraded_groups contains G2; got {:?}",
        outcome.degraded_groups
    );
    assert!(
        !outcome.master_timed_out,
        "(SA2) master_timed_out stays false (per-group wins); got true"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "(SA2) per-group cut-in time bound; elapsed {elapsed:?} >= 5 s"
    );

    // ── SA3: one executor errors ────────────────────────
    let erroring_executors: Vec<Box<dyn GroupExecutor + Send + Sync>> = vec![
        Box::new(AvailableExec {
            group: Group::G1,
            hit: dummy_hit("a.rs", 1),
        }),
        Box::new(ErroringExec { group: Group::G2 }),
    ];
    let outcome = execute_cross_group(
        dummy_query(),
        erroring_executors,
        CROSS_GROUP_MASTER_DEADLINE,
    )
    .await;
    assert_eq!(
        outcome.results[1].status,
        GroupStatus::Errored,
        "(SA3) error status; left: {:?}; right: Errored",
        outcome.results[1].status
    );
    assert!(
        outcome.degraded_groups.contains(&Group::G2),
        "(SA3) degraded_groups contains G2; got {:?}",
        outcome.degraded_groups
    );
    assert!(
        !outcome.master_timed_out,
        "(SA3) master_timed_out stays false on error path; got true"
    );

    // ── SA4: master-deadline trip ───────────────────────
    let bad_executors: Vec<Box<dyn GroupExecutor + Send + Sync>> = vec![Box::new(SleepingExec {
        group: Group::G1,
        sleep: Duration::from_secs(7),
    })];
    let start = tokio::time::Instant::now();
    let outcome =
        execute_cross_group(dummy_query(), bad_executors, Duration::from_millis(100)).await;
    let elapsed = start.elapsed();
    assert!(
        outcome.master_timed_out,
        "(SA4) master_timed_out; expected true, got false"
    );
    assert_eq!(
        outcome.results[0].status,
        GroupStatus::TimedOut,
        "(SA4) master-trip placeholder status; left: {:?}; right: TimedOut",
        outcome.results[0].status
    );
    assert!(
        outcome.degraded_groups.contains(&outcome.results[0].group),
        "(SA4) master-trip degraded_groups contains placeholder group"
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "(SA4) master-trip wall time bound; elapsed {elapsed:?} >= 2 s"
    );

    // ── SA5: empty executors ────────────────────────────
    let outcome = execute_cross_group(dummy_query(), vec![], CROSS_GROUP_MASTER_DEADLINE).await;
    assert!(
        outcome.results.is_empty(),
        "(SA5) empty executors results; got {:?}",
        outcome.results
    );
    assert!(
        outcome.degraded_groups.is_empty(),
        "(SA5) empty executors degraded_groups; got {:?}",
        outcome.degraded_groups
    );
    assert!(
        !outcome.master_timed_out,
        "(SA5) empty executors master_timed_out; got true"
    );

    // ── SA6: order preservation, non-canonical input ────
    let unordered_executors: Vec<Box<dyn GroupExecutor + Send + Sync>> = vec![
        Box::new(AvailableExec {
            group: Group::G3,
            hit: dummy_hit("c.rs", 3),
        }),
        Box::new(AvailableExec {
            group: Group::G1,
            hit: dummy_hit("a.rs", 1),
        }),
        Box::new(AvailableExec {
            group: Group::G4,
            hit: dummy_hit("d.rs", 4),
        }),
        Box::new(AvailableExec {
            group: Group::G2,
            hit: dummy_hit("b.rs", 2),
        }),
    ];
    let outcome = execute_cross_group(
        dummy_query(),
        unordered_executors,
        CROSS_GROUP_MASTER_DEADLINE,
    )
    .await;
    let order: Vec<Group> = outcome.results.iter().map(|r| r.group).collect();
    assert_eq!(
        order,
        vec![Group::G3, Group::G1, Group::G4, Group::G2],
        "(SA6) result ordering preserves input executor order"
    );

    // ── SA7: JSON round-trip on the SA1 outcome ─────────
    let json = serde_json::to_string(&sa1_outcome).expect("(SA7) serialize");
    let round: CrossGroupExecution = serde_json::from_str(&json).expect("(SA7) deserialize");
    assert_eq!(
        round, sa1_outcome,
        "(SA7) round-trip equality; left: {round:?}; right: {sa1_outcome:?}"
    );
}

// ── F04 frozen test ──────────────────────────────────────

/// Frozen acceptance test for P3-W9-F04 (cross-group RRF
/// fusion). Master-plan §6.2 lines 643-658 + line 645
/// (k = 60).
///
/// Sub-assertions (DEC-0007 SA-numbered panic messages):
/// - SA1: Basic RRF math — same location at rank 1 in both
///   G1 and G2 with `FindReferences` (G1=3.0, G2=2.0).
/// - SA2: Weighted RRF cross-location — different files at
///   rank 1 with `FindDefinition` (G1=3.0, G2=1.5).
/// - SA3: Same hit at different ranks across groups —
///   ranks (1, 2) with `FindReferences`.
/// - SA4: `degraded_groups` passthrough.
/// - SA5: Zero-weight skip — `Remember` row, G1 hit excluded.
/// - SA6: `used_weights` snapshot — Remember row sentinel.
/// - SA7: JSON round-trip on `CrossGroupFusedOutcome`.
#[cfg(test)]
#[test]
#[allow(clippy::too_many_lines, clippy::float_cmp, clippy::missing_panics_doc)] // DEC-0005 module-coherence: SA1..SA7 cohesive; SA6 sentinel needs bit-exact float compare.
pub fn test_cross_group_rrf_fusion() {
    // ── SA1: basic RRF math, same location ──────────────
    let g1 = GroupResult {
        group: Group::G1,
        status: GroupStatus::Available,
        hits: vec![GroupHit {
            file_path: PathBuf::from("src/lib.rs"),
            start_line: 10,
            end_line: 15,
            snippet: "g1_snippet".to_owned(),
            score: 0.9,
        }],
        elapsed_ms: 5,
        error: None,
    };
    let g2 = GroupResult {
        group: Group::G2,
        status: GroupStatus::Available,
        hits: vec![GroupHit {
            file_path: PathBuf::from("src/lib.rs"),
            start_line: 10,
            end_line: 15,
            snippet: "g2_snippet".to_owned(),
            score: 0.7,
        }],
        elapsed_ms: 5,
        error: None,
    };
    let exec = CrossGroupExecution {
        results: vec![g1, g2],
        master_timed_out: false,
        wall_elapsed_ms: 5,
        degraded_groups: vec![],
    };
    let outcome = fuse_cross_group(&exec, QueryType::FindReferences);
    assert_eq!(
        outcome.hits.len(),
        1,
        "(SA1) basic RRF outcome.hits.len; left: {}; right: 1",
        outcome.hits.len()
    );
    // §6.2 line 651: find_references → G1 = 3.0, G2 = 2.0
    let expected = 3.0_f64 / 61.0 + 2.0_f64 / 61.0;
    assert!(
        (outcome.hits[0].fused_score - expected).abs() < 1e-9,
        "(SA1) fused_score; left: {}; right: {expected}",
        outcome.hits[0].fused_score
    );
    assert_eq!(
        outcome.hits[0].contributing_groups,
        vec![Group::G1, Group::G2],
        "(SA1) contributing_groups order (descending weight)"
    );
    assert!(
        outcome.hits[0].per_group_ranks.contains(&(Group::G1, 1)),
        "(SA1) per_group_ranks contains (G1, 1)"
    );
    assert!(
        outcome.hits[0].per_group_ranks.contains(&(Group::G2, 1)),
        "(SA1) per_group_ranks contains (G2, 1)"
    );

    let sa1_outcome = outcome;

    // ── SA2: weighted RRF, cross-location ───────────────
    let file1 = PathBuf::from("file1.rs");
    let file2 = PathBuf::from("file2.rs");
    let g1 = GroupResult {
        group: Group::G1,
        status: GroupStatus::Available,
        hits: vec![GroupHit {
            file_path: file1.clone(),
            start_line: 1,
            end_line: 1,
            snippet: "f1".to_owned(),
            score: 0.9,
        }],
        elapsed_ms: 5,
        error: None,
    };
    let g2 = GroupResult {
        group: Group::G2,
        status: GroupStatus::Available,
        hits: vec![GroupHit {
            file_path: file2.clone(),
            start_line: 1,
            end_line: 1,
            snippet: "f2".to_owned(),
            score: 0.7,
        }],
        elapsed_ms: 5,
        error: None,
    };
    let exec = CrossGroupExecution {
        results: vec![g1, g2],
        master_timed_out: false,
        wall_elapsed_ms: 5,
        degraded_groups: vec![],
    };
    let outcome = fuse_cross_group(&exec, QueryType::FindDefinition);
    assert_eq!(
        outcome.hits.len(),
        2,
        "(SA2) cross-location outcome.hits.len; left: {}; right: 2",
        outcome.hits.len()
    );
    // §6.2 line 650: find_definition → G1 = 3.0, G2 = 1.5
    assert_eq!(
        outcome.hits[0].file_path, file1,
        "(SA2) higher-weight hit comes first; left: {:?}; right: {:?}",
        outcome.hits[0].file_path, file1
    );
    assert!(
        (outcome.hits[0].fused_score - 3.0_f64 / 61.0).abs() < 1e-9,
        "(SA2) hits[0] fused_score; left: {}; right: {}",
        outcome.hits[0].fused_score,
        3.0_f64 / 61.0
    );
    assert_eq!(
        outcome.hits[1].file_path, file2,
        "(SA2) lower-weight hit comes second"
    );

    // ── SA3: same hit at different ranks across groups ──
    let file_x = PathBuf::from("x.rs");
    let g1 = GroupResult {
        group: Group::G1,
        status: GroupStatus::Available,
        hits: vec![GroupHit {
            file_path: file_x.clone(),
            start_line: 1,
            end_line: 1,
            snippet: "x_g1".to_owned(),
            score: 0.9,
        }],
        elapsed_ms: 5,
        error: None,
    };
    let g2 = GroupResult {
        group: Group::G2,
        status: GroupStatus::Available,
        hits: vec![
            GroupHit {
                file_path: PathBuf::from("decoy.rs"),
                start_line: 1,
                end_line: 1,
                snippet: "decoy".to_owned(),
                score: 0.99,
            },
            GroupHit {
                file_path: file_x.clone(),
                start_line: 1,
                end_line: 1,
                snippet: "x_g2".to_owned(),
                score: 0.7,
            },
        ],
        elapsed_ms: 5,
        error: None,
    };
    let exec = CrossGroupExecution {
        results: vec![g1, g2],
        master_timed_out: false,
        wall_elapsed_ms: 5,
        degraded_groups: vec![],
    };
    let outcome = fuse_cross_group(&exec, QueryType::FindReferences);
    // §6.2 line 651: G1 = 3.0, G2 = 2.0
    let expected = 3.0_f64 / 61.0 + 2.0_f64 / 62.0;
    // Find the file_x hit (it might not be at index 0 because
    // the decoy at G2 rank 1 may outscore it depending on
    // weights — we deliberately check by file_path).
    let x_hit = outcome
        .hits
        .iter()
        .find(|h| h.file_path == file_x)
        .expect("(SA3) file_x must be present");
    assert!(
        (x_hit.fused_score - expected).abs() < 1e-9,
        "(SA3) fused_score; left: {}; right: {expected}",
        x_hit.fused_score
    );
    assert!(
        x_hit.per_group_ranks.contains(&(Group::G1, 1)),
        "(SA3) per_group_ranks contains (G1, 1)"
    );
    assert!(
        x_hit.per_group_ranks.contains(&(Group::G2, 2)),
        "(SA3) per_group_ranks contains (G2, 2)"
    );

    // ── SA4: degraded_groups passthrough ────────────────
    let exec = CrossGroupExecution {
        results: vec![],
        master_timed_out: false,
        wall_elapsed_ms: 0,
        degraded_groups: vec![Group::G3, Group::G7],
    };
    let outcome = fuse_cross_group(&exec, QueryType::UnderstandCode);
    assert_eq!(
        outcome.degraded_groups,
        vec![Group::G3, Group::G7],
        "(SA4) degraded_groups passthrough verbatim"
    );

    // ── SA5: zero-weight skip (Remember sentinel row) ───
    // §6.2 line 658: Remember = [0, 0, 3.0, 0, 0, 0, 0, 0]
    // — G1 weight is 0.0, so the contribution is 0 and the
    // hit is excluded per §6.3 line 667 threshold-of-zero
    // contract.
    let g1 = GroupResult {
        group: Group::G1,
        status: GroupStatus::Available,
        hits: vec![GroupHit {
            file_path: PathBuf::from("zero.rs"),
            start_line: 1,
            end_line: 1,
            snippet: "z".to_owned(),
            score: 0.5,
        }],
        elapsed_ms: 1,
        error: None,
    };
    let exec = CrossGroupExecution {
        results: vec![g1],
        master_timed_out: false,
        wall_elapsed_ms: 1,
        degraded_groups: vec![],
    };
    let outcome = fuse_cross_group(&exec, QueryType::Remember);
    assert!(
        outcome.hits.is_empty(),
        "(SA5) zero-weight hit must be excluded; got {:?}",
        outcome.hits
    );

    // ── SA6: used_weights snapshot (Remember sentinel) ──
    assert_eq!(
        outcome.used_weights,
        [0.0, 0.0, 3.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        "(SA6) used_weights Remember sentinel; left: {:?}",
        outcome.used_weights
    );

    // ── SA7: JSON round-trip on the SA1 outcome ─────────
    let json = serde_json::to_string(&sa1_outcome).expect("(SA7) serialize");
    let round: CrossGroupFusedOutcome = serde_json::from_str(&json).expect("(SA7) deserialize");
    assert_eq!(
        round, sa1_outcome,
        "(SA7) round-trip equality; left: {round:?}; right: {sa1_outcome:?}"
    );
}
