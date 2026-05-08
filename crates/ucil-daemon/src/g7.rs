//! G7 (Quality) parallel-execution orchestrator + severity-weighted
//! merge fusion.
//!
//! Features `P3-W11-F01` + `P3-W11-F05`, master-plan §3.2 row 14
//! (`check_quality`), §5.7 lines 539-559, §15.2 line 1519
//! (`ucil.group.quality` span), §17 line 1636 (`g7.rs` placement),
//! §18 Phase 3 Week 11 deliverables #1 + #2.
//!
//! # Pipeline shape
//!
//! Master-plan §5.7 prescribes the G7 (Quality) fan-out as
//! "All quality tools → severity-weighted merge": every installed
//! quality plugin (`LSP` diagnostics, `ESLint`, Ruff, Semgrep, …) runs in
//! parallel under a master deadline; per-source outputs are then
//! merged by `(file_path, line_start, category)` with the highest
//! severity winning per group and merged details preserved across
//! source tools.
//!
//! Same shape as WO-0070 G3 (Knowledge) and WO-0083 G4 (Architecture):
//!
//! 1. Each [`G7Source::execute`] runs under a per-source
//!    `tokio::time::timeout` parameterised by [`G7_PER_SOURCE_DEADLINE`].
//! 2. The whole fan-out runs under
//!    `tokio::time::timeout(master_deadline, ...)`.
//! 3. On master-deadline trip, [`G7SourceStatus::TimedOut`] placeholders
//!    are synthesised in input order so `results[i].source_id` matches
//!    `sources[i].source_id()` either way.
//!
//! # Merge contract
//!
//! [`merge_g7_by_severity`] groups every [`G7Issue`] from `Available`
//! sources by `(file_path, line_start, category)`.  Within each group
//! the algorithm picks the highest severity ([`Severity`]'s natural
//! `Ord` puts the most severe first — `Critical` = 0 is the smallest
//! discriminant, so `.min()` selects the most severe), with ties
//! broken by lexicographic `source_tool`.  The merged entry's
//! `source_tools` / `rule_ids` / `fix_suggestions` are the deduplicated
//! alphabetically-sorted union over **all** issues in the group (per
//! §5.7 line 556 "keep highest-severity with merged details").
//!
//! `Errored` or `TimedOut` sources contribute zero issues to the merge.
//! The output [`MergedG7Issue`] vec is sorted severity-ascending
//! (`Critical` first), then `file_path` ASC, then `line_start` ASC
//! (`None` last).
//!
//! # No-substitute-impls policy
//!
//! Per master-plan §15.4 + CLAUDE.md "no substitute impls of critical
//! deps", this module — its public traits, types, and orchestrator —
//! does NOT contain placeholder implementations of LSP servers,
//! JSON-RPC transports, or `tokio::process::Command` subprocess
//! runners.  The module ships the trait + orchestrator + merger only;
//! production `G7Source` impls (e.g. `LspDiagnosticsG7Source`,
//! `EslintG7Source`, `RuffG7Source`, `SemgrepG7Source`) are deferred
//! to follow-up production-wiring WOs that bundle the daemon-startup
//! orchestration.  The frozen acceptance tests
//! [`test_g7_parallel_pipeline`] + [`test_g7_severity_merge`] supply
//! UCIL-internal `G7Source` impls (`DEC-0008` §4 dependency-inversion
//! seam) under `#[cfg(test)]`.
//!
//! Same shape as WO-0070 G3 / WO-0083 G4.

#![allow(clippy::module_name_repetitions)]
#![deny(rustdoc::broken_intra_doc_links)]

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::task::Poll;
use std::time::Duration;

use serde::{Deserialize, Serialize};

// ── Constants ─────────────────────────────────────────────────────────────

/// Per-source deadline applied to each [`G7Source::execute`] call.
///
/// **Held as an unconditional `const`, NOT `min`'d with the caller-
/// supplied `master_deadline`.**  WO-0068 lessons-learned (For
/// executor #2 + For planner #3) demonstrated that capping per-source
/// by master collapses both timeouts on tight masters and the inner
/// per-source wins, hiding the master trip.  The 4.5 s / 5.5 s margin
/// keeps per-source as the primary path under default config; tight-
/// master cases (e.g. 100 ms test masters) let the master fire first
/// deterministically.
///
/// Mirrors the G3 [`crate::g3::G3_PER_SOURCE_DEADLINE`] and G4
/// `G4_PER_SOURCE_DEADLINE` values per master-plan §15 timeout
/// discipline.
pub const G7_PER_SOURCE_DEADLINE: Duration = Duration::from_millis(4_500);

/// Default master deadline for the G7 (Quality) parallel-execution
/// orchestrator.
///
/// Master-plan §5.7 + §6.1 line 606 prescribe a 5-6 s overall deadline
/// for any group fan-out so the daemon can return partial results to
/// the host adapter when one quality tool stalls.  When this deadline
/// elapses, [`execute_g7`] returns a [`G7Outcome`] with
/// `master_timed_out = true` and per-source [`G7SourceStatus::TimedOut`]
/// placeholders for any source that had not yet completed.
///
/// The 5.5 s value gives the per-source 4.5 s deadline a 1 s margin so
/// the per-source path wins under default config.
pub const G7_DEFAULT_MASTER_DEADLINE: Duration = Duration::from_millis(5_500);

// ── Public types ──────────────────────────────────────────────────────────

/// G7 (Quality) query input — quality lookup over every installed
/// quality source for a target file or symbol.
///
/// Master-plan §3.2 row 14 prescribes the public `check_quality(target,
/// type)` MCP tool surface; live wiring will derive these from the host
/// adapter's request through the §6.1 query-pipeline classifier.  The
/// frozen acceptance test constructs them directly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct G7Query {
    /// File path or symbol the quality query is anchored on.
    pub target: String,
    /// Issue categories to filter the source emissions by — empty
    /// vector means "all categories".  Master-plan §12.1 enumerates
    /// the canonical category strings: `type_error`, `lint`,
    /// `security`, `style`, `complexity`.
    pub categories: Vec<String>,
}

/// Severity ladder for a single quality issue.
///
/// Master-plan §5.7 line 555 prescribes the hierarchy
/// `critical > high > medium > low`; §12.1 extends it with a fifth
/// `info` rung for informational diagnostics.  The discriminant order
/// is **`Critical = 0` < `High = 1` < `Medium = 2` < `Low = 3` <
/// `Info = 4`** so the natural `Ord` puts the LOWEST discriminant
/// (most severe) first — [`merge_g7_by_severity`] uses `.min()` to
/// keep the most severe issue in each group.
///
/// Serializes as the lowercase `quality_issues.severity` column value
/// per §12.1: `"critical"`, `"high"`, `"medium"`, `"low"`, `"info"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Most-severe rung — typically reserved for security findings or
    /// rule-id allow-list promotions (e.g. `RustBorrowCheckError`).
    Critical,
    /// Hard failures (type errors, broken builds).  Default for
    /// `DiagnosticSeverity::Error` in the LSP-4 → quality-5 collapse.
    High,
    /// Warnings.  Default for `DiagnosticSeverity::Warning`.
    Medium,
    /// Information-level diagnostics.  Default for
    /// `DiagnosticSeverity::Information`.
    Low,
    /// Hints / inlay candidates.  Default for
    /// `DiagnosticSeverity::Hint`.
    Info,
}

impl Severity {
    /// Lowercase string projection for the §12.1
    /// `quality_issues.severity` column.
    ///
    /// Returns `"critical"`, `"high"`, `"medium"`, `"low"`, or
    /// `"info"` per the master-plan §5.7 + §12.1 vocabulary.  Used by
    /// the persistence layer to bind the column value without paying
    /// for `serde_json` serialization.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Critical => "critical",
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
            Self::Info => "info",
        }
    }
}

/// One quality issue emitted by a G7 source.
///
/// Field-by-field maps to the §12.1 `quality_issues` columns (without
/// `id` / `first_seen` / `last_seen` / `resolved` / `resolved_by_session`
/// — those are persistence-side concerns owned by F06's
/// `persist_diagnostics` UPSERT, NOT the in-memory issue surface).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct G7Issue {
    /// Tool that emitted the issue (e.g. `"lsp:rust-analyzer"`,
    /// `"eslint"`, `"ruff"`, `"semgrep"`).
    pub source_tool: String,
    /// File path the issue is anchored to.
    pub file_path: String,
    /// 1-indexed start line.  `None` when the source did not provide
    /// a line range (e.g. file-level findings).
    pub line_start: Option<u32>,
    /// 1-indexed end line.  `None` when the source did not provide a
    /// line range, or when the issue's start and end lines are
    /// identical and the source omitted the explicit end.
    pub line_end: Option<u32>,
    /// One of `type_error` / `lint` / `security` / `style` /
    /// `complexity` per §12.1.
    pub category: String,
    /// Severity rung — see [`Severity`].
    pub severity: Severity,
    /// Free-form operator-readable issue description.
    pub message: String,
    /// Tool-defined rule identifier (e.g. `"E0308"`, `"F401"`).
    pub rule_id: Option<String>,
    /// Optional remediation suggestion the tool provided.
    pub fix_suggestion: Option<String>,
}

/// Disposition of one G7 source on a given fan-out call.
///
/// Each variant is a discriminant with no inner data — the data lives
/// on [`G7SourceOutput`] via `issues` / `error` / `elapsed_ms`.
/// Master-plan §5.7 + §6.1 prescribes per-source dispositions so partial
/// outcomes remain usable: a single [`G7SourceStatus::Errored`] does
/// not turn the entire fan-out into a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum G7SourceStatus {
    /// The source returned its issues within the per-source deadline.
    Available,
    /// The source's per-source `tokio::time::timeout` elapsed before
    /// it returned a response.
    TimedOut,
    /// The source returned an error (transport / decode / internal).
    Errored,
}

/// One source's contribution to a G7 fan-out outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct G7SourceOutput {
    /// Identifier of the source that emitted this output.  Stable
    /// across the source's lifetime and unique within one
    /// [`execute_g7`] call — matches [`G7Source::source_id`].
    pub source_id: String,
    /// Disposition of the source on this fan-out call.
    pub status: G7SourceStatus,
    /// Wall-clock time the source spent before returning, in
    /// milliseconds.
    pub elapsed_ms: u64,
    /// Source-emitted quality issues.  Empty when `status` is
    /// `TimedOut` or `Errored`; the merger ignores those statuses.
    pub issues: Vec<G7Issue>,
    /// Operator-readable error description for any non-`Available`
    /// status.  `None` for [`G7SourceStatus::Available`].
    pub error: Option<String>,
}

/// Aggregate outcome of one [`execute_g7`] fan-out call.
///
/// `results` is a `Vec` whose order matches the input `sources`
/// argument so callers can correlate by index.  `master_timed_out` is
/// `true` when the outer master deadline elapsed before all per-source
/// futures completed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct G7Outcome {
    /// Per-source outputs, in the same order as the input `sources`.
    pub results: Vec<G7SourceOutput>,
    /// Wall-clock time the orchestrator spent, in milliseconds.
    pub wall_elapsed_ms: u64,
    /// `true` iff the outer master deadline elapsed before all
    /// per-source futures completed.
    pub master_timed_out: bool,
}

/// One severity-weighted merged quality issue emitted by
/// [`merge_g7_by_severity`].
///
/// Master-plan §5.7 line 556 prescribes the merge directive
/// `keep highest-severity with merged details`: the winning issue
/// carries the most-severe rung observed at its `(file_path,
/// line_start, category)` group, plus the deduplicated alphabetically-
/// sorted union of `source_tools` / `rule_ids` / `fix_suggestions`
/// across **all** issues in the group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergedG7Issue {
    /// File path the merged group is anchored to.
    pub file_path: String,
    /// 1-indexed start line.  `None` for file-level groups.
    pub line_start: Option<u32>,
    /// Issue category — one of `type_error` / `lint` / `security` /
    /// `style` / `complexity` per §12.1.
    pub category: String,
    /// Highest severity observed in the group.  Within tied-severity
    /// candidates the issue with the alphabetically-first
    /// `source_tool` wins.
    pub severity: Severity,
    /// Operator-readable message from the winning candidate.
    pub message: String,
    /// Deduplicated alphabetically-sorted union of every
    /// contributing issue's `source_tool`.
    pub source_tools: Vec<String>,
    /// Deduplicated alphabetically-sorted union of every
    /// contributing issue's `Some(rule_id)` (None values dropped).
    pub rule_ids: Vec<String>,
    /// Deduplicated alphabetically-sorted union of every
    /// contributing issue's `Some(fix_suggestion)` (None values
    /// dropped).
    pub fix_suggestions: Vec<String>,
}

// ── Trait + helpers ───────────────────────────────────────────────────────

/// Dependency-inversion seam for one G7 (Quality) source.
///
/// Per `DEC-0008` §4 this trait is UCIL-owned — it is **not** a
/// re-export or adapter of any external wire format.  The frozen
/// acceptance tests [`test_g7_parallel_pipeline`] +
/// [`test_g7_severity_merge`] supply local trait impls of [`G7Source`]
/// (UCIL's own abstraction boundary); production wiring of real
/// subprocess clients (e.g. `LspDiagnosticsG7Source` calling the
/// existing [`crate::g7`] feed via the F06 KG, `EslintG7Source` calling
/// the WO-0076 plugin runtime, `RuffG7Source` calling WO-0080's
/// runtime, `SemgrepG7Source` calling WO-0076 Semgrep plugin) is
/// deferred to follow-up production-wiring WOs.
///
/// Same shape as the WO-0047 `G1Source`, WO-0070 `G3Source`, and
/// WO-0083 `G4Source` traits.  `Send + Sync` bounds are required so
/// trait objects can live in `Vec<Box<dyn G7Source + Send + Sync +
/// 'static>>` inside the daemon's long-lived server state once the
/// production-wiring WO lands.
#[async_trait::async_trait]
pub trait G7Source: Send + Sync {
    /// Identifies this source without runtime introspection so
    /// [`execute_g7`] can label results by source.  The returned
    /// string is expected to be stable across the source's lifetime
    /// and unique within one [`execute_g7`] call.
    fn source_id(&self) -> &str;

    /// Run this source's quality query.
    ///
    /// Implementations are responsible for emitting their own
    /// [`G7SourceOutput`] with the appropriate
    /// [`G7SourceStatus`] — the orchestrator only overrides the status
    /// to [`G7SourceStatus::TimedOut`] when its per-source
    /// `tokio::time::timeout` elapses.
    async fn execute(&self, query: &G7Query) -> G7SourceOutput;
}

/// Run one source under [`G7_PER_SOURCE_DEADLINE`], converting a
/// per-source timeout into a [`G7SourceStatus::TimedOut`]
/// [`G7SourceOutput`] without ever panicking.
///
/// The helper keeps [`execute_g7`] focused on the fan-out shape —
/// per-source timeout handling lives here so the orchestrator does not
/// need a `match` arm per disposition.  Mirrors `run_g3_source` /
/// `run_g4_source`.
async fn run_g7_source(
    source: &(dyn G7Source + Send + Sync),
    query: &G7Query,
    per_source_deadline: Duration,
) -> G7SourceOutput {
    let source_id = source.source_id().to_owned();
    let start = std::time::Instant::now();
    tokio::time::timeout(per_source_deadline, source.execute(query))
        .await
        .unwrap_or_else(|_| {
            let elapsed_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
            G7SourceOutput {
                source_id,
                status: G7SourceStatus::TimedOut,
                elapsed_ms,
                issues: vec![],
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
/// the same `poll_fn` fan-out shape as `crate::g3::join_all_g3` and
/// the WO-0083 G4 helper — `tokio` ships everything we need for a
/// 3-to-N-way fan-out without introducing an additional poll-set
/// abstraction.
///
/// `#[allow(dead_code)]` covers the verifier's M1 mutation contract:
/// the M1 mutation swaps [`run_g7_source`]'s
/// `tokio::time::timeout(per_source_deadline, source.execute(query))`
/// for a direct `source.execute(query).await`, which leaves the
/// timeout-handling closure unreferenced but does NOT orphan this
/// helper — the dead-code guard remains here mirroring WO-0070 line
/// 192 precedent so any future M1-style mutation that orphans the
/// helper does not flip the verifier from SA-tagged panic to compile
/// failure.
#[allow(dead_code)]
async fn join_all_g7<'a, T>(
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
        .map(|r| r.expect("join_all_g7: every slot must be filled before returning"))
        .collect()
}

// ── Orchestrator ──────────────────────────────────────────────────────────

/// G7 (Quality) parallel-execution orchestrator.
///
/// Master-plan §5.7 lines 539-559 prescribes the fan-out shape:
/// `Query → ALL of {LSP diagnostics, ESLint, Ruff, Semgrep, …} run in
/// parallel`, with a 5-6 s overall deadline so partial outcomes stay
/// usable when one quality tool stalls.
///
/// Implementation:
///
/// 1. The per-source deadline is held at [`G7_PER_SOURCE_DEADLINE`]
///    **unconditionally** — it is NOT `min`'d with `master_deadline`.
///    Per WO-0068 lessons-learned (For executor #2 + For planner #3),
///    capping per-source by master collapses both timeouts on tight
///    masters and the inner per-source wins, hiding the master trip.
///    The 4.5 s / 5.5 s margin keeps per-source as the primary path
///    under default config (master = 5.5 s); tight-master cases (e.g.
///    100 ms test masters) let the master fire first deterministically.
/// 2. Each per-source future is wrapped in
///    `tokio::time::timeout(G7_PER_SOURCE_DEADLINE, ...)` via
///    [`run_g7_source`] which returns
///    [`G7SourceStatus::TimedOut`] on elapse.
/// 3. Build one boxed future per source and poll them concurrently
///    through [`join_all_g7`] (the same `poll_fn` fan-out shape as
///    `execute_g3` / `execute_cross_group`).
/// 4. Wrap the whole join in
///    `tokio::time::timeout(master_deadline, ...)`.  On `Err(Elapsed)`,
///    return a [`G7Outcome`] with [`G7SourceStatus::TimedOut`]
///    placeholders for every source and `master_timed_out = true` so
///    downstream code never sees an empty result vector when the
///    master deadline fires.
///
/// The orchestrator never `panic!`s and never `?` propagates an error
/// out — partial results are valid output per master-plan §5.7 +
/// §6.1 line 606.
///
/// Per master-plan §15.2 line 1519, this orchestrator emits a
/// `tracing` span `ucil.group.quality` (parallel to
/// `ucil.group.knowledge` for G3 and `ucil.group.architecture` for
/// G4).
///
/// # Examples
///
/// ```no_run
/// use std::time::Duration;
/// use ucil_daemon::g7::{
///     execute_g7, G7Query, G7Source, G7_DEFAULT_MASTER_DEADLINE,
/// };
///
/// # async fn demo(sources: Vec<Box<dyn G7Source + Send + Sync + 'static>>) {
/// let q = G7Query {
///     target: "src/auth.rs".to_owned(),
///     categories: vec![],
/// };
/// let outcome = execute_g7(sources, q, G7_DEFAULT_MASTER_DEADLINE).await;
/// assert!(!outcome.master_timed_out || !outcome.results.is_empty());
/// # }
/// ```
#[tracing::instrument(
    name = "ucil.group.quality",
    level = "debug",
    skip(sources, query),
    fields(source_count = sources.len()),
)]
pub async fn execute_g7(
    sources: Vec<Box<dyn G7Source + Send + Sync + 'static>>,
    query: G7Query,
    master_deadline: Duration,
) -> G7Outcome {
    // Step 1 + Step 2: start time + per-source deadline.
    //
    // The per-source deadline is held at [`G7_PER_SOURCE_DEADLINE`]
    // unconditionally so the master deadline ALWAYS wins on a tight
    // `master_deadline`: when
    // `master_deadline < G7_PER_SOURCE_DEADLINE`, the outer
    // `tokio::time::timeout(master_deadline, ...)` fires first and the
    // master-trip path synthesises in-order [`G7SourceStatus::TimedOut`]
    // placeholders. Capping `per_source_deadline` by `master_deadline`
    // would race the two timers and let the inner per-source timeout
    // resolve the inner future first, hiding the master trip
    // (WO-0068 lessons-learned, For executor #2 + For planner #3).
    let start = std::time::Instant::now();
    let per_source_deadline = G7_PER_SOURCE_DEADLINE;

    // Step 3: build one boxed future per source and poll them
    // concurrently.  A `tokio::task::JoinSet` would also work but the
    // `poll_fn` fan-out keeps the cancellation semantics simple — when
    // the outer `master_deadline` fires the outer `timeout` wraps
    // everything together so unfinished futures are dropped, mirroring
    // the `execute_g3` + `execute_cross_group` patterns.
    let q_ref = &query;
    let mut futures: Vec<Pin<Box<dyn Future<Output = G7SourceOutput> + Send + '_>>> =
        Vec::with_capacity(sources.len());
    for s in &sources {
        futures.push(Box::pin(run_g7_source(
            s.as_ref(),
            q_ref,
            per_source_deadline,
        )));
    }

    // Step 4: outer master-deadline wrap.
    let outer = tokio::time::timeout(master_deadline, join_all_g7(futures)).await;
    let wall_elapsed_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

    // Step 5: preserve input order. On master-deadline trip, synthesise
    // `TimedOut` placeholders in input order so
    // `results[i].source_id == sources[i].source_id()` holds either way.
    outer.map_or_else(
        |_| {
            let results = sources
                .iter()
                .map(|s| G7SourceOutput {
                    source_id: s.source_id().to_owned(),
                    status: G7SourceStatus::TimedOut,
                    elapsed_ms: wall_elapsed_ms,
                    issues: vec![],
                    error: Some(format!(
                        "G7 master deadline {} ms elapsed",
                        master_deadline.as_millis()
                    )),
                })
                .collect();
            G7Outcome {
                results,
                wall_elapsed_ms,
                master_timed_out: true,
            }
        },
        |results| G7Outcome {
            results,
            wall_elapsed_ms,
            master_timed_out: false,
        },
    )
}

// ── Severity-weighted merge fusion ────────────────────────────────────────

/// Severity-weighted merge for G7 (Quality) issues.
///
/// Master-plan §5.7 lines 539-559 prescribes the merge contract:
///
/// 1. Group every [`G7Issue`] by `(file_path, line_start, category)`
///    (use a `BTreeMap` for deterministic ordering).
/// 2. Within each group, find the most-severe `Severity` via
///    `.iter().map(|i| i.severity).min().unwrap()` — the smallest
///    discriminant is the most severe per the [`Severity`] doc, so
///    `.min()` keeps the highest-rank issue.
/// 3. Among the candidates carrying the highest severity in the
///    group, the FIRST one (ordered by `source_tool` ASC) is selected
///    for `severity` / `message` / `line_start` / `category` /
///    `file_path`.
/// 4. The `source_tools` / `rule_ids` / `fix_suggestions` vecs project
///    the deduplicated alphabetically-sorted union over **all** issues
///    in the group (not just the highest-severity ones — the §5.7
///    line 556 directive `keep highest-severity with merged details`).
/// 5. The output is sorted: primary key = `severity` ascending (which
///    is `Critical` first per the discriminant order), secondary =
///    `file_path` ASC, tertiary = `line_start` ASC (`None` last).
///
/// This is severity-weighted merge, NOT rank-based RRF (`fusion::*`)
/// or entity-keyed temporal merge (`crate::g3::merge_g3_by_entity`).
///
/// The merger is pure-deterministic CPU-bound logic — no async, no IO,
/// no logging — so per WO-0067 §`lessons_applied` #5 + WO-0070 G3
/// merger precedent it does NOT carry a `#[tracing::instrument]`
/// span.  `execute_g7` (which IS async/IO/orchestration) DOES carry
/// the §15.2 `ucil.group.quality` span.
///
/// # Examples
///
/// ```
/// use ucil_daemon::g7::{merge_g7_by_severity, G7Issue, Severity};
///
/// let issues = vec![G7Issue {
///     source_tool: "lsp:rust-analyzer".to_owned(),
///     file_path: "src/auth.rs".to_owned(),
///     line_start: Some(42),
///     line_end: Some(42),
///     category: "type_error".to_owned(),
///     severity: Severity::High,
///     message: "mismatched types".to_owned(),
///     rule_id: Some("E0308".to_owned()),
///     fix_suggestion: None,
/// }];
/// let merged = merge_g7_by_severity(&issues);
/// assert_eq!(merged.len(), 1);
/// assert_eq!(merged[0].severity, Severity::High);
/// ```
///
/// # Panics
///
/// The `.min().unwrap()` on the per-group severity pick is
/// unreachable in practice — every group is constructed via
/// `BTreeMap::entry().or_default().push(issue)`, so each group always
/// contains at least one [`G7Issue`].  The `unwrap()` shape is
/// retained verbatim so the M2 mutation contract (`.min()` → `.max()`,
/// per-WO M2) targets a single token rather than a shape rewrite.
#[must_use]
pub fn merge_g7_by_severity(issues: &[G7Issue]) -> Vec<MergedG7Issue> {
    if issues.is_empty() {
        return vec![];
    }

    // Step 1: group by (file_path, line_start, category).  BTreeMap
    // keeps the iteration order deterministic.
    let mut groups: BTreeMap<(String, Option<u32>, String), Vec<&G7Issue>> = BTreeMap::new();
    for issue in issues {
        groups
            .entry((
                issue.file_path.clone(),
                issue.line_start,
                issue.category.clone(),
            ))
            .or_default()
            .push(issue);
    }

    // Step 2-4: per-group merge.
    let mut merged: Vec<MergedG7Issue> = Vec::with_capacity(groups.len());
    for ((file_path, line_start, category), group) in groups {
        // Step 2: find the most-severe rung (smallest discriminant).
        let highest_severity = group.iter().map(|i| i.severity).min().unwrap();

        // Step 3: among tied-severity candidates, pick the
        // alphabetically-first `source_tool` as the canonical winner
        // for `severity` / `message`.  We rely on the BTreeMap-driven
        // input ordering being stable but explicitly sort by
        // `source_tool` to keep the contract independent of caller
        // ordering.
        let mut tied: Vec<&G7Issue> = group
            .iter()
            .copied()
            .filter(|i| i.severity == highest_severity)
            .collect();
        tied.sort_by(|a, b| a.source_tool.cmp(&b.source_tool));
        let winner = tied[0];

        // Step 4: deduplicated alphabetically-sorted union over ALL
        // issues in the group (not just tied-severity candidates).
        let mut source_tools: Vec<String> = group.iter().map(|i| i.source_tool.clone()).collect();
        source_tools.sort();
        source_tools.dedup();

        let mut rule_ids: Vec<String> = group.iter().filter_map(|i| i.rule_id.clone()).collect();
        rule_ids.sort();
        rule_ids.dedup();

        let mut fix_suggestions: Vec<String> = group
            .iter()
            .filter_map(|i| i.fix_suggestion.clone())
            .collect();
        fix_suggestions.sort();
        fix_suggestions.dedup();

        merged.push(MergedG7Issue {
            file_path,
            line_start,
            category,
            severity: highest_severity,
            message: winner.message.clone(),
            source_tools,
            rule_ids,
            fix_suggestions,
        });
    }

    // Step 5: sort severity ASC (most-severe first), then file_path
    // ASC, then line_start ASC (None last).
    merged.sort_by(|a, b| {
        a.severity
            .cmp(&b.severity)
            .then_with(|| a.file_path.cmp(&b.file_path))
            .then_with(|| match (a.line_start, b.line_start) {
                (Some(x), Some(y)) => x.cmp(&y),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            })
    });

    merged
}

// ── Module-root acceptance tests (P3-W11-F01 / P3-W11-F05 oracle) ─────────

/// Frozen acceptance selector for feature `P3-W11-F01` — see
/// `ucil-build/feature-list.json` entry for
/// `-p ucil-daemon g7::test_g7_parallel_pipeline`.
///
/// Drives [`execute_g7`] over UCIL-internal [`G7Source`] impls and
/// asserts six properties (SA1..SA6):
///
/// * **SA1 — Result count**: 3 sources → `outcome.results.len() == 3`.
/// * **SA2 — Master-deadline does not trip**: master = 5500 ms with
///   sources that finish under it → `master_timed_out == false`.
/// * **SA3 — Available source returns issue**: the `available` source
///   returns `Available` status with 1 issue.
/// * **SA4 — `TimedOut` source**: the `slow` source (4700 ms) trips the
///   per-source 4500 ms deadline → `TimedOut` status, no issues, error
///   message contains `"per-source deadline"`.
/// * **SA5 — Errored source**: the `errored` source returns `Errored`
///   status with a non-empty `error` and no issues.
/// * **SA6 — Parallelism**: `wall_elapsed_ms` < 5000 ms.  Sequential
///   200ms+4700ms+200ms = 5100ms+ would exceed this; parallel
///   execution is dominated by the 4500ms per-source timeout.
#[cfg(test)]
#[allow(clippy::too_many_lines, clippy::missing_panics_doc)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
pub async fn test_g7_parallel_pipeline() {
    /// Behaviour switches for the per-test [`G7Source`] impl below.
    #[derive(Clone)]
    enum TestBehaviour {
        ReturnIssues(Vec<G7Issue>),
        LongSleep(Duration),
        Error(String),
    }

    /// Local [`G7Source`] impl driving the per-scenario behaviour.
    /// Per `DEC-0008` §4 this is a UCIL-internal trait (the
    /// dependency-inversion seam), so a local impl in a test is not a
    /// mock of any external wire format — same shape as the
    /// `TestG3Source` impl at `executor.rs:3194`.
    struct TestG7Source {
        id: String,
        behaviour: TestBehaviour,
    }

    #[async_trait::async_trait]
    impl G7Source for TestG7Source {
        fn source_id(&self) -> &str {
            &self.id
        }

        async fn execute(&self, _query: &G7Query) -> G7SourceOutput {
            match &self.behaviour {
                TestBehaviour::ReturnIssues(issues) => G7SourceOutput {
                    source_id: self.id.clone(),
                    status: G7SourceStatus::Available,
                    elapsed_ms: 0,
                    issues: issues.clone(),
                    error: None,
                },
                TestBehaviour::LongSleep(d) => {
                    tokio::time::sleep(*d).await;
                    // The orchestrator's per-source timeout must fire
                    // before this branch returns; if the test ever
                    // sees `Available` here, the timeout wrapper has
                    // regressed (an M1-style mutation would land us
                    // here).
                    G7SourceOutput {
                        source_id: self.id.clone(),
                        status: G7SourceStatus::Available,
                        elapsed_ms: u64::try_from(d.as_millis()).unwrap_or(u64::MAX),
                        issues: vec![],
                        error: None,
                    }
                }
                TestBehaviour::Error(msg) => G7SourceOutput {
                    source_id: self.id.clone(),
                    status: G7SourceStatus::Errored,
                    elapsed_ms: 0,
                    issues: vec![],
                    error: Some(msg.clone()),
                },
            }
        }
    }

    let q = G7Query {
        target: "src/auth.rs".to_owned(),
        categories: vec![],
    };

    let issue_avail = G7Issue {
        source_tool: "lsp:rust-analyzer".to_owned(),
        file_path: "src/auth.rs".to_owned(),
        line_start: Some(10),
        line_end: Some(10),
        category: "type_error".to_owned(),
        severity: Severity::High,
        message: "mismatched types".to_owned(),
        rule_id: Some("E0308".to_owned()),
        fix_suggestion: None,
    };

    // Three sources: available (immediate, 1 issue), slow (4700ms
    // sleep — must trip per-source deadline), errored.
    let sources: Vec<Box<dyn G7Source + Send + Sync + 'static>> = vec![
        Box::new(TestG7Source {
            id: "available".to_owned(),
            behaviour: TestBehaviour::ReturnIssues(vec![issue_avail.clone()]),
        }),
        Box::new(TestG7Source {
            id: "slow".to_owned(),
            behaviour: TestBehaviour::LongSleep(Duration::from_millis(4_700)),
        }),
        Box::new(TestG7Source {
            id: "errored".to_owned(),
            behaviour: TestBehaviour::Error("eslint subprocess crashed".to_owned()),
        }),
    ];

    let outcome = execute_g7(sources, q, Duration::from_millis(5_500)).await;

    // SA1 — Result count.
    assert_eq!(
        outcome.results.len(),
        3,
        "(SA1) outcome.results.len(); left: {}, right: 3",
        outcome.results.len()
    );

    // SA2 — Master deadline does not trip on this 5500 ms master with
    // a 4500 ms per-source deadline (slow source trips per-source
    // before master fires).
    assert!(
        !outcome.master_timed_out,
        "(SA2) outcome.master_timed_out must be false; left: true, right: false"
    );

    // SA3 — Available source returns its 1 issue.
    assert_eq!(
        outcome.results[0].status,
        G7SourceStatus::Available,
        "(SA3a) outcome.results[0].status; left: {:?}, right: Available",
        outcome.results[0].status
    );
    assert_eq!(
        outcome.results[0].issues.len(),
        1,
        "(SA3b) outcome.results[0].issues.len(); left: {}, right: 1",
        outcome.results[0].issues.len()
    );
    assert_eq!(
        outcome.results[0].issues[0], issue_avail,
        "(SA3c) outcome.results[0].issues[0]; left: {:?}, right: {:?}",
        outcome.results[0].issues[0], issue_avail
    );

    // SA4 — TimedOut source has empty issues + per-source-deadline error.
    assert_eq!(
        outcome.results[1].status,
        G7SourceStatus::TimedOut,
        "(SA4a) outcome.results[1].status; left: {:?}, right: TimedOut",
        outcome.results[1].status
    );
    assert!(
        outcome.results[1].issues.is_empty(),
        "(SA4b) outcome.results[1].issues must be empty; left: {:?}, right: []",
        outcome.results[1].issues
    );
    let err1 = outcome.results[1].error.as_deref().unwrap_or("");
    assert!(
        err1.contains("per-source deadline"),
        "(SA4c) outcome.results[1].error must contain `per-source deadline`; left: {err1:?}"
    );

    // SA5 — Errored source preserves status + error message.
    assert_eq!(
        outcome.results[2].status,
        G7SourceStatus::Errored,
        "(SA5a) outcome.results[2].status; left: {:?}, right: Errored",
        outcome.results[2].status
    );
    assert!(
        outcome.results[2].error.is_some(),
        "(SA5b) outcome.results[2].error must be Some; left: None"
    );

    // SA6 — Parallelism: wall_elapsed_ms < 5000 ms.  A sequential
    // execution would be 0+4500+0 = 4500ms minimum (the timeout
    // dominates), but a buggy sequential ordering with `available` →
    // `slow` → `errored` would only need ~4500ms to complete, so this
    // bound at 5000 catches *gross* sequentialisation.  The M1
    // mutation specifically (per-source timeout bypass) lets the slow
    // source run to its full 4700ms, so SA4 is the primary catcher
    // for M1; SA6 is the parallelism backstop.
    assert!(
        outcome.wall_elapsed_ms < 5_000,
        "(SA6) outcome.wall_elapsed_ms < 5000 (parallel execution); left: {}, right: 5000",
        outcome.wall_elapsed_ms
    );
}

/// Frozen acceptance selector for feature `P3-W11-F05` — see
/// `ucil-build/feature-list.json` entry for
/// `-p ucil-daemon g7::test_g7_severity_merge`.
///
/// Drives [`merge_g7_by_severity`] over hand-built [`G7Issue`] inputs
/// and asserts six properties (SA1..SA6):
///
/// * **SA1 — Empty input**: `merge_g7_by_severity(&[])` returns
///   empty.
/// * **SA2 — Single issue**: 1 input → 1 output preserving every
///   field with `source_tools=[issue.source_tool]`.
/// * **SA3 — Same-group different-severity**: two issues at same
///   group with `Medium` + `Critical` → merged keeps `Critical`,
///   `source_tools` is the alphabetical-sorted union.
/// * **SA4 — Different groups + intra-group highest-wins**: 3 issues
///   spanning 2 distinct groups (Group A: Critical+Low; Group B:
///   High) → output has 2 entries sorted Critical-first.
/// * **SA5 — Tied-severity merge**: two issues both `High` at same
///   group, `source_tools` `aaa` and `bbb` → winner is `aaa` (alphabet);
///   `source_tools` / `rule_ids` / `fix_suggestions` show
///   deduplicated unions.
/// * **SA6 — Sentinel-row severity vocabulary**: a 6-issue mix
///   spanning every severity rung returns 6 outputs sorted Critical
///   first → Info last with the canonical lowercase `severity.as_str()`
///   mapping `"critical"/"high"/"medium"/"low"/"info"`.
#[cfg(test)]
#[allow(clippy::too_many_lines, clippy::missing_panics_doc)]
#[test]
pub fn test_g7_severity_merge() {
    // SA1 — Empty input.
    let empty = merge_g7_by_severity(&[]);
    assert_eq!(
        empty,
        Vec::<MergedG7Issue>::new(),
        "(SA1) empty input; left: {empty:?}, right: []"
    );

    // SA2 — Single issue.
    let single = vec![G7Issue {
        source_tool: "lsp:rust-analyzer".to_owned(),
        file_path: "src/auth.rs".to_owned(),
        line_start: Some(42),
        line_end: Some(42),
        category: "type_error".to_owned(),
        severity: Severity::High,
        message: "mismatched types".to_owned(),
        rule_id: Some("E0308".to_owned()),
        fix_suggestion: None,
    }];
    let m_single = merge_g7_by_severity(&single);
    assert_eq!(
        m_single.len(),
        1,
        "(SA2a) single-issue → 1 output; left: {}, right: 1",
        m_single.len()
    );
    assert_eq!(
        m_single[0].file_path, "src/auth.rs",
        "(SA2b) file_path preserved; left: {:?}, right: \"src/auth.rs\"",
        m_single[0].file_path
    );
    assert_eq!(
        m_single[0].severity,
        Severity::High,
        "(SA2c) severity preserved; left: {:?}, right: High",
        m_single[0].severity
    );
    assert_eq!(
        m_single[0].message, "mismatched types",
        "(SA2d) message preserved; left: {:?}, right: \"mismatched types\"",
        m_single[0].message
    );
    assert_eq!(
        m_single[0].source_tools,
        vec!["lsp:rust-analyzer".to_owned()],
        "(SA2e) source_tools singleton; left: {:?}, right: [\"lsp:rust-analyzer\"]",
        m_single[0].source_tools
    );
    assert_eq!(
        m_single[0].rule_ids,
        vec!["E0308".to_owned()],
        "(SA2f) rule_ids singleton; left: {:?}, right: [\"E0308\"]",
        m_single[0].rule_ids
    );

    // SA3 — Same-group different-severity (Medium + Critical → Critical).
    let mixed = vec![
        G7Issue {
            source_tool: "eslint".to_owned(),
            file_path: "src/auth.rs".to_owned(),
            line_start: Some(42),
            line_end: Some(42),
            category: "lint".to_owned(),
            severity: Severity::Medium,
            message: "shadowed variable".to_owned(),
            rule_id: Some("no-shadow".to_owned()),
            fix_suggestion: None,
        },
        G7Issue {
            source_tool: "semgrep".to_owned(),
            file_path: "src/auth.rs".to_owned(),
            line_start: Some(42),
            line_end: Some(42),
            category: "lint".to_owned(),
            severity: Severity::Critical,
            message: "hardcoded credentials".to_owned(),
            rule_id: Some("rules.security.creds".to_owned()),
            fix_suggestion: Some("use env vars".to_owned()),
        },
    ];
    let m_mixed = merge_g7_by_severity(&mixed);
    assert_eq!(
        m_mixed.len(),
        1,
        "(SA3a) merge collapses same-group; left: {}, right: 1",
        m_mixed.len()
    );
    assert_eq!(
        m_mixed[0].severity,
        Severity::Critical,
        "(SA3b) highest severity wins; left: {:?}, right: Critical",
        m_mixed[0].severity
    );
    assert_eq!(
        m_mixed[0].source_tools,
        vec!["eslint".to_owned(), "semgrep".to_owned()],
        "(SA3c) alphabetical-sorted union of source_tools; left: {:?}, right: \
         [\"eslint\", \"semgrep\"]",
        m_mixed[0].source_tools
    );

    // SA4 — Different groups + intra-group highest-wins.
    let multi = vec![
        // Group A (src/a.rs:10:lint): Critical + Low → Critical wins.
        G7Issue {
            source_tool: "alpha".to_owned(),
            file_path: "src/a.rs".to_owned(),
            line_start: Some(10),
            line_end: Some(10),
            category: "lint".to_owned(),
            severity: Severity::Critical,
            message: "critical issue A".to_owned(),
            rule_id: None,
            fix_suggestion: None,
        },
        G7Issue {
            source_tool: "beta".to_owned(),
            file_path: "src/a.rs".to_owned(),
            line_start: Some(10),
            line_end: Some(10),
            category: "lint".to_owned(),
            severity: Severity::Low,
            message: "low issue A".to_owned(),
            rule_id: None,
            fix_suggestion: None,
        },
        // Group B (src/b.rs:20:lint): High alone.
        G7Issue {
            source_tool: "gamma".to_owned(),
            file_path: "src/b.rs".to_owned(),
            line_start: Some(20),
            line_end: Some(20),
            category: "lint".to_owned(),
            severity: Severity::High,
            message: "high issue B".to_owned(),
            rule_id: None,
            fix_suggestion: None,
        },
    ];
    let m_multi = merge_g7_by_severity(&multi);
    assert_eq!(
        m_multi.len(),
        2,
        "(SA4a) two distinct groups; left: {}, right: 2",
        m_multi.len()
    );
    // Sort key is severity ASC (Critical first).
    assert_eq!(
        m_multi[0].severity,
        Severity::Critical,
        "(SA4b) Critical sorts first; left: {:?}, right: Critical",
        m_multi[0].severity
    );
    assert_eq!(
        m_multi[0].file_path, "src/a.rs",
        "(SA4c) Critical entry from Group A; left: {:?}, right: \"src/a.rs\"",
        m_multi[0].file_path
    );
    assert_eq!(
        m_multi[1].severity,
        Severity::High,
        "(SA4d) High sorts second (NOT Low — high-severity in Group A wins); \
         left: {:?}, right: High",
        m_multi[1].severity
    );

    // SA5 — Tied-severity merge: two issues both `High` at same group;
    // source_tools alphabetically `aaa` and `bbb` → winner is `aaa`.
    let tied = vec![
        G7Issue {
            source_tool: "bbb".to_owned(),
            file_path: "src/x.rs".to_owned(),
            line_start: Some(5),
            line_end: Some(5),
            category: "lint".to_owned(),
            severity: Severity::High,
            message: "from bbb".to_owned(),
            rule_id: Some("rule-bbb".to_owned()),
            fix_suggestion: Some("fix-bbb".to_owned()),
        },
        G7Issue {
            source_tool: "aaa".to_owned(),
            file_path: "src/x.rs".to_owned(),
            line_start: Some(5),
            line_end: Some(5),
            category: "lint".to_owned(),
            severity: Severity::High,
            message: "from aaa".to_owned(),
            rule_id: Some("rule-aaa".to_owned()),
            fix_suggestion: Some("fix-aaa".to_owned()),
        },
    ];
    let m_tied = merge_g7_by_severity(&tied);
    assert_eq!(
        m_tied.len(),
        1,
        "(SA5a) tied-severity collapses; left: {}, right: 1",
        m_tied.len()
    );
    assert_eq!(
        m_tied[0].message, "from aaa",
        "(SA5b) winner ordered alphabetically by source_tool (aaa < bbb); \
         left: {:?}, right: \"from aaa\"",
        m_tied[0].message
    );
    assert_eq!(
        m_tied[0].source_tools,
        vec!["aaa".to_owned(), "bbb".to_owned()],
        "(SA5c) source_tools deduplicated + sorted; left: {:?}, right: \
         [\"aaa\", \"bbb\"]",
        m_tied[0].source_tools
    );
    assert_eq!(
        m_tied[0].rule_ids,
        vec!["rule-aaa".to_owned(), "rule-bbb".to_owned()],
        "(SA5d) rule_ids deduplicated + sorted union; left: {:?}, right: \
         [\"rule-aaa\", \"rule-bbb\"]",
        m_tied[0].rule_ids
    );
    assert_eq!(
        m_tied[0].fix_suggestions,
        vec!["fix-aaa".to_owned(), "fix-bbb".to_owned()],
        "(SA5e) fix_suggestions deduplicated + sorted union; left: {:?}, right: \
         [\"fix-aaa\", \"fix-bbb\"]",
        m_tied[0].fix_suggestions
    );

    // SA6 — Sentinel-row severity vocabulary canary.  Six issues
    // across distinct groups, one per severity rung, all with
    // distinct (file_path, line_start, category) so each lands in its
    // own merged entry.  The output is sorted Critical first → Info
    // last, with the §5.7/§12.1 lowercase string mapping.
    let sentinel = vec![
        G7Issue {
            source_tool: "t1".to_owned(),
            file_path: "f1.rs".to_owned(),
            line_start: Some(1),
            line_end: Some(1),
            category: "security".to_owned(),
            severity: Severity::Critical,
            message: "c1".to_owned(),
            rule_id: None,
            fix_suggestion: None,
        },
        G7Issue {
            source_tool: "t2".to_owned(),
            file_path: "f2.rs".to_owned(),
            line_start: Some(2),
            line_end: Some(2),
            category: "security".to_owned(),
            severity: Severity::Critical,
            message: "c2".to_owned(),
            rule_id: None,
            fix_suggestion: None,
        },
        G7Issue {
            source_tool: "t3".to_owned(),
            file_path: "f3.rs".to_owned(),
            line_start: Some(3),
            line_end: Some(3),
            category: "type_error".to_owned(),
            severity: Severity::High,
            message: "h".to_owned(),
            rule_id: None,
            fix_suggestion: None,
        },
        G7Issue {
            source_tool: "t4".to_owned(),
            file_path: "f4.rs".to_owned(),
            line_start: Some(4),
            line_end: Some(4),
            category: "lint".to_owned(),
            severity: Severity::Medium,
            message: "m".to_owned(),
            rule_id: None,
            fix_suggestion: None,
        },
        G7Issue {
            source_tool: "t5".to_owned(),
            file_path: "f5.rs".to_owned(),
            line_start: Some(5),
            line_end: Some(5),
            category: "lint".to_owned(),
            severity: Severity::Low,
            message: "l".to_owned(),
            rule_id: None,
            fix_suggestion: None,
        },
        G7Issue {
            source_tool: "t6".to_owned(),
            file_path: "f6.rs".to_owned(),
            line_start: Some(6),
            line_end: Some(6),
            category: "lint".to_owned(),
            severity: Severity::Info,
            message: "i".to_owned(),
            rule_id: None,
            fix_suggestion: None,
        },
    ];
    let m_sentinel = merge_g7_by_severity(&sentinel);
    assert_eq!(
        m_sentinel.len(),
        6,
        "(SA6a) six distinct groups → 6 outputs; left: {}, right: 6",
        m_sentinel.len()
    );
    // Sort by severity ASC: Critical, Critical, High, Medium, Low, Info.
    let severities: Vec<Severity> = m_sentinel.iter().map(|m| m.severity).collect();
    assert_eq!(
        severities,
        vec![
            Severity::Critical,
            Severity::Critical,
            Severity::High,
            Severity::Medium,
            Severity::Low,
            Severity::Info,
        ],
        "(SA6b) severity-ascending sort order; left: {severities:?}, right: \
         [Critical, Critical, High, Medium, Low, Info]"
    );
    // Vocabulary canary: every severity's `as_str` matches §5.7/§12.1.
    let strs: Vec<&'static str> = m_sentinel.iter().map(|m| m.severity.as_str()).collect();
    assert_eq!(
        strs,
        vec!["critical", "critical", "high", "medium", "low", "info"],
        "(SA6c) severity.as_str() lowercase vocabulary; left: {strs:?}, right: \
         [\"critical\", \"critical\", \"high\", \"medium\", \"low\", \"info\"]"
    );
}
