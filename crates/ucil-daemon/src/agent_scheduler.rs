//! Warm-tier promotion processors driven by an interval-based
//! [`AgentScheduler`].
//!
//! Feature `P3-W10-F13`, master-plan §10 `[knowledge_tiering]` config
//! block lines 2015-2024 (interval seconds + min-evidence + dedup
//! threshold), §11 hot/warm schema lines 1213-1320 (`hot_observations`
//! / `hot_convention_signals` / `hot_architecture_deltas` /
//! `hot_decision_material` and their warm-tier counterparts), §15.2
//! lines 1518-1522 (`tracing` span `ucil.<layer>.<op>` discipline),
//! §17.2 line 1636 (warm-processor module placement; reinterpreted per
//! `DEC-0008` §4 to live in `ucil-daemon` so the trait and orchestration
//! sit beside the live `KnowledgeGraph` handle), §18 Phase 3 Week 10
//! lines 1810-1815 (warm processors thread).
//!
//! # Pipeline shape
//!
//! Master-plan §10 lines 2016-2024 prescribe four interval-driven
//! processors that promote unpromoted hot-tier rows into the
//! corresponding warm-tier table:
//!
//! 1. `observation_processor_interval_sec = 60` →
//!    [`run_observation_processor`] dedups
//!    `hot_observations` by `related_symbol` similarity ≥
//!    [`OBSERVATION_DEDUP_THRESHOLD`] and inserts one
//!    `warm_observations` row per cluster.
//! 2. `convention_signal_processor_interval_sec = 60` →
//!    [`run_convention_signal_processor`] groups
//!    `hot_convention_signals` by `pattern_hash` and promotes a group
//!    only when its size meets [`CONVENTION_MIN_EVIDENCE`].
//! 3. `architecture_delta_processor_interval_sec = 120` →
//!    [`run_architecture_delta_processor`] aggregates
//!    `hot_architecture_deltas` by `(change_type, file_path)` and
//!    upserts one `warm_architecture_state` row per group.
//! 4. `decision_linker_interval_sec = 60` →
//!    [`run_decision_linker_processor`] selects
//!    `hot_decision_material` rows with non-null `affected_files` and
//!    inserts one `warm_decisions` row per qualifying hot row.
//!
//! Each processor runs under [`WARM_PROCESSOR_OP_DEADLINE`] held as an
//! unconditional `const` (NOT `min`'d with caller-supplied deadlines)
//! per WO-0068 lessons §"per-source deadline UNCONDITIONAL const".
//!
//! [`AgentScheduler::start`] spawns four `tokio::time::interval_at`-
//! driven tasks (one per [`WarmProcessorKind`]) inside a
//! [`tokio::task::JoinSet`]; the matching
//! [`AgentSchedulerHandle::shutdown`] flips a
//! [`tokio::sync::watch`] channel to signal every task to break
//! out of its select-loop, then drains the join-set so no task leaks.
//!
//! # No-substitute-impls policy
//!
//! Per master-plan §15.4 + `CLAUDE.md` "no substitute impls of critical
//! deps": this module ships the [`WarmProcessorSource`] trait, four
//! concrete processor functions, and the [`AgentScheduler`]
//! orchestrator only. NO substitute / placeholder implementations of
//! `SQLite`, `KnowledgeGraph`, or `tokio::process::Command` exist on the
//! production path; the trait is the dependency-inversion seam
//! (`DEC-0008` §4) — production impls MUST own a real
//! [`ucil_core::knowledge_graph::KnowledgeGraph`] handle. The
//! `#[cfg(test)]` `TestWarmProcessorSource` impl is exempt under the
//! WO-0048 `#[cfg(test)]` carve-out — it lives at the bottom of this
//! file beside the frozen test [`test_warm_processors`].
//!
//! Same shape (trait + orchestration + frozen test, production wiring
//! deferred) as WO-0070 G3 / WO-0083 G4 / WO-0085 G7 / WO-0089 G8.

#![allow(clippy::module_name_repetitions)]
#![deny(rustdoc::broken_intra_doc_links)]

use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

// ── Constants ─────────────────────────────────────────────────────────────

/// Cadence of [`run_observation_processor`] ticks.
///
/// Master-plan §10 line 2016 sets
/// `observation_processor_interval_sec = 60`. Held as a `tokio::time::
/// Duration` so [`AgentScheduler::start`] can wire it directly into
/// `tokio::time::interval_at`.
pub const OBSERVATION_PROCESSOR_INTERVAL: Duration = Duration::from_secs(60);

/// Cadence of [`run_convention_signal_processor`] ticks.
///
/// Master-plan §10 line 2017 sets
/// `convention_signal_processor_interval_sec = 60`. Same cadence as
/// the observation processor, but a distinct `const` so the
/// production config loader can override each independently in a
/// future WO without coupling the two values.
pub const CONVENTION_SIGNAL_PROCESSOR_INTERVAL: Duration = Duration::from_secs(60);

/// Cadence of [`run_architecture_delta_processor`] ticks.
///
/// Master-plan §10 line 2018 sets
/// `architecture_delta_processor_interval_sec = 120`. Twice the
/// observation/convention/decision interval — architecture deltas are
/// rarer and aggregating them less often keeps `SQLite` write
/// amplification predictable.
pub const ARCHITECTURE_DELTA_PROCESSOR_INTERVAL: Duration = Duration::from_secs(120);

/// Cadence of [`run_decision_linker_processor`] ticks.
///
/// Master-plan §10 line 2019 sets `decision_linker_interval_sec = 60`.
/// Same cadence as observation/convention, but a distinct `const`
/// keeps the four interval values independently tunable.
pub const DECISION_LINKER_INTERVAL: Duration = Duration::from_secs(60);

/// Minimum evidence count required for a `hot_convention_signals`
/// `pattern_hash` group to promote into `warm_conventions`.
///
/// Master-plan §10 line 2020 sets `convention_min_evidence = 3`.
/// Below this threshold the convention candidate is treated as
/// idiosyncratic and held back; the hot rows stay unpromoted so a
/// future tick can re-evaluate once new evidence arrives.
pub const CONVENTION_MIN_EVIDENCE: usize = 3;

/// Token-overlap (Jaccard) similarity threshold above which two
/// `hot_observations` rows under the same `related_symbol` are
/// clustered into a single `warm_observations` row.
///
/// Master-plan §10 line 2024 sets `observation_dedup_threshold = 0.9`.
/// Token-overlap is preferred over a third-party crate (`strsim`,
/// `levenshtein`, ...) per `.claude/rules/rust-style.md` §`Crate
/// layout` "keep `ucil-daemon` lean": a hand-rolled Jaccard suffices
/// for the §10 spec value.
pub const OBSERVATION_DEDUP_THRESHOLD: f64 = 0.9;

/// Maximum number of unpromoted hot rows examined per processor tick.
///
/// Bounds the per-tick wall-clock cost so a back-log of accumulated
/// hot rows cannot block the scheduler. Subsequent ticks drain the
/// remaining rows. A 256-row batch keeps each tick well under
/// [`WARM_PROCESSOR_OP_DEADLINE`] on cold-cache `SQLite` reads.
pub const WARM_PROCESSOR_BATCH_SIZE: usize = 256;

/// Per-operation deadline applied to each
/// [`WarmProcessorSource`] async call inside a processor tick.
///
/// Held as an unconditional `const`, NOT `min`'d with the
/// `AgentScheduler`-level cancellation signal. Capping per-op by an
/// outer signal collapses both timeouts on tight outer cancels and
/// the inner per-op wins — the WO-0068 lessons-learned `Timeout::poll`
/// inner-first race carried into G3 / G7 / G8 mirrors here.
pub const WARM_PROCESSOR_OP_DEADLINE: Duration = Duration::from_secs(30);

// ── Error type ────────────────────────────────────────────────────────────

/// Errors emitted by [`WarmProcessorSource`] methods and the
/// processor functions ([`run_observation_processor`], …).
///
/// `#[non_exhaustive]` per `.claude/rules/rust-style.md` §`Errors`
/// (libraries: `thiserror`; `non_exhaustive`); future variants for
/// new failure shapes (e.g. schema-version mismatch) can be added
/// without breaking downstream `match` arms.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum WarmProcessorError {
    /// Underlying [`WarmProcessorSource`] returned an error message.
    /// Production impls SHOULD enrich the message with row-id /
    /// SQL-statement context; the trait surface accepts an opaque
    /// string so test impls do not have to construct a typed error.
    #[error("warm processor source error: {0}")]
    Source(String),
    /// A [`WarmProcessorSource`] async call exceeded
    /// [`WARM_PROCESSOR_OP_DEADLINE`].
    #[error("warm processor op deadline exceeded")]
    Timeout,
    /// A `SQLite` (or other database) operation failed mid-call.
    /// Currently only constructed from production impls; the test
    /// impl uses [`WarmProcessorError::Source`].
    #[error("database error: {0}")]
    Database(String),
    /// The scheduler's cancellation signal flipped while the
    /// processor was mid-tick.
    #[error("warm processor cancelled")]
    Cancelled,
}

// ── Public types ──────────────────────────────────────────────────────────

/// Discriminator naming each warm-tier promotion processor.
///
/// Master-plan §10 lines 2016-2019 enumerate the four interval
/// values; the variants are 1:1 with those lines:
///
/// * [`WarmProcessorKind::Observation`] — line 2016 →
///   [`OBSERVATION_PROCESSOR_INTERVAL`].
/// * [`WarmProcessorKind::ConventionSignal`] — line 2017 →
///   [`CONVENTION_SIGNAL_PROCESSOR_INTERVAL`].
/// * [`WarmProcessorKind::ArchitectureDelta`] — line 2018 →
///   [`ARCHITECTURE_DELTA_PROCESSOR_INTERVAL`].
/// * [`WarmProcessorKind::DecisionLinker`] — line 2019 →
///   [`DECISION_LINKER_INTERVAL`].
///
/// `Hash + Eq` are required so the scheduler can key the per-kind
/// stats and last-result maps by [`WarmProcessorKind`]; `Copy +
/// Ord` simplifies iteration in `BTreeMap` keys for deterministic
/// test output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarmProcessorKind {
    /// Observation processor — master-plan §10 line 2016.
    Observation,
    /// Convention-signal processor — master-plan §10 line 2017.
    ConventionSignal,
    /// Architecture-delta processor — master-plan §10 line 2018.
    ArchitectureDelta,
    /// Decision-linker processor — master-plan §10 line 2019.
    DecisionLinker,
}

impl WarmProcessorKind {
    /// Returns the interval for this processor variant — wires the
    /// `pub const`s above to the [`AgentScheduler::start`] task
    /// dispatcher.
    #[must_use]
    pub const fn interval(self) -> Duration {
        match self {
            Self::Observation => OBSERVATION_PROCESSOR_INTERVAL,
            Self::ConventionSignal => CONVENTION_SIGNAL_PROCESSOR_INTERVAL,
            Self::ArchitectureDelta => ARCHITECTURE_DELTA_PROCESSOR_INTERVAL,
            Self::DecisionLinker => DECISION_LINKER_INTERVAL,
        }
    }

    /// Iteration order used by [`AgentScheduler::start`] when spawning
    /// the four per-kind tasks. Stable so test assertions can pin
    /// per-kind ordering.
    #[must_use]
    pub const fn all() -> [Self; 4] {
        [
            Self::Observation,
            Self::ConventionSignal,
            Self::ArchitectureDelta,
            Self::DecisionLinker,
        ]
    }
}

/// Outcome of a single processor tick.
///
/// `error` is `Some(_)` only when the tick failed — successful ticks
/// (including those that examined zero hot rows) carry `None`. The
/// invariant
/// `warm_rows_inserted + dropped_due_to_threshold == hot_rows_examined`
/// holds in spirit but is not enforced at the type level: clusters
/// that fall below [`OBSERVATION_DEDUP_THRESHOLD`] are absorbed into
/// other clusters and the dropped count is implicit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WarmPromotionResult {
    /// Which processor produced this result.
    pub kind: WarmProcessorKind,
    /// Total hot rows the processor selected (before grouping).
    pub hot_rows_examined: u64,
    /// Warm rows inserted (one per qualifying cluster / group).
    pub warm_rows_inserted: u64,
    /// Hot rows the processor flipped `promoted_to_warm = 1` /
    /// `promoted = 1` on (matches `hot_rows_examined` minus any rows
    /// that fell below a per-kind threshold).
    pub hot_rows_marked_promoted: u64,
    /// `Some(error_message)` when the tick failed; `None` otherwise.
    pub error: Option<String>,
}

/// Aggregate per-kind scheduler stats.
///
/// `BTreeMap` (not `HashMap`) for deterministic iteration order in
/// test snapshots. Both maps are keyed by [`WarmProcessorKind`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSchedulerStats {
    /// Number of ticks observed per kind, monotonically increasing.
    pub ticks_observed: BTreeMap<WarmProcessorKind, u64>,
    /// Last result per kind. `None` until the first tick fires.
    pub last_result: BTreeMap<WarmProcessorKind, WarmPromotionResult>,
}

// ── Internal POD row types ────────────────────────────────────────────────

/// Hot-tier `hot_observations` row mirroring master-plan §11
/// lines 1214-1222.
///
/// Field names match the SQL schema verbatim so the production
/// `WarmProcessorSource` adapter is a 1:1 `rusqlite` pluck.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotObservationRow {
    /// `INTEGER PRIMARY KEY AUTOINCREMENT`.
    pub id: i64,
    /// `raw_text TEXT NOT NULL`.
    pub raw_text: String,
    /// `session_id TEXT` (nullable).
    pub session_id: Option<String>,
    /// `related_file TEXT` (nullable).
    pub related_file: Option<String>,
    /// `related_symbol TEXT` (nullable).
    pub related_symbol: Option<String>,
    /// `created_at TEXT NOT NULL DEFAULT (datetime('now'))`.
    pub created_at: String,
}

/// Hot-tier `hot_convention_signals` row mirroring master-plan §11
/// lines 1224-1231.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotConventionSignalRow {
    /// `INTEGER PRIMARY KEY AUTOINCREMENT`.
    pub id: i64,
    /// `pattern_hash TEXT NOT NULL`.
    pub pattern_hash: String,
    /// `file_path TEXT NOT NULL`.
    pub file_path: String,
    /// `example_snippet TEXT` (nullable).
    pub example_snippet: Option<String>,
    /// `created_at TEXT NOT NULL DEFAULT (datetime('now'))`.
    pub created_at: String,
}

/// Hot-tier `hot_architecture_deltas` row mirroring master-plan §11
/// lines 1233-1240.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotArchitectureDeltaRow {
    /// `INTEGER PRIMARY KEY AUTOINCREMENT`.
    pub id: i64,
    /// `change_type TEXT NOT NULL`.
    pub change_type: String,
    /// `file_path TEXT NOT NULL`.
    pub file_path: String,
    /// `details TEXT` (nullable JSON blob).
    pub details: Option<String>,
    /// `created_at TEXT NOT NULL DEFAULT (datetime('now'))`.
    pub created_at: String,
}

/// Hot-tier `hot_decision_material` row mirroring master-plan §11
/// lines 1242-1251.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotDecisionMaterialRow {
    /// `INTEGER PRIMARY KEY AUTOINCREMENT`.
    pub id: i64,
    /// `source_type TEXT NOT NULL` ('pr', 'commit', 'issue', 'adr').
    pub source_type: String,
    /// `source_url TEXT` (nullable).
    pub source_url: Option<String>,
    /// `title TEXT` (nullable).
    pub title: Option<String>,
    /// `description TEXT` (nullable).
    pub description: Option<String>,
    /// `affected_files TEXT` (nullable JSON-array blob; `None` rows
    /// are filtered out by [`run_decision_linker_processor`]).
    pub affected_files: Option<String>,
    /// `created_at TEXT NOT NULL DEFAULT (datetime('now'))`.
    pub created_at: String,
}

/// Warm-tier `warm_observations` row mirroring master-plan §11
/// lines 1254-1264.
#[derive(Debug, Clone, PartialEq)]
pub struct WarmObservationRow {
    /// Aggregated text — typically the longest `raw_text` in the
    /// cluster (production impls may rephrase / summarise).
    pub text: String,
    /// `domains TEXT` (nullable JSON blob; production wires from
    /// session domain tags).
    pub domains: Option<String>,
    /// `related_entities TEXT` (nullable JSON blob; production wires
    /// from the cluster's `related_symbol` set).
    pub related_entities: Option<String>,
    /// `severity TEXT` (nullable).
    pub severity: Option<String>,
    /// `evidence_count INTEGER DEFAULT 1` — set to the cluster size.
    pub evidence_count: i64,
    /// `first_seen TEXT` — earliest `created_at` in the cluster.
    pub first_seen: Option<String>,
    /// `last_seen TEXT` — latest `created_at` in the cluster.
    pub last_seen: Option<String>,
    /// `confidence REAL DEFAULT 0.6`.
    pub confidence: f64,
}

/// Warm-tier `warm_conventions` row mirroring master-plan §11
/// lines 1266-1274.
#[derive(Debug, Clone, PartialEq)]
pub struct WarmConventionRow {
    /// `category TEXT NOT NULL` — production wires from the
    /// `pattern_hash` to a higher-level category bucket; the trait
    /// surface accepts the raw value.
    pub category: String,
    /// `pattern_description TEXT NOT NULL`.
    pub pattern_description: String,
    /// `examples TEXT` (nullable JSON-array blob).
    pub examples: Option<String>,
    /// `evidence_count INTEGER DEFAULT 3` — set to the group size.
    pub evidence_count: i64,
    /// `confidence REAL DEFAULT 0.5`.
    pub confidence: f64,
}

/// Warm-tier `warm_architecture_state` row mirroring master-plan §11
/// lines 1276-1283.
#[derive(Debug, Clone, PartialEq)]
pub struct WarmArchitectureStateRow {
    /// `summary TEXT NOT NULL` — production wires from a structured
    /// summary of the `(change_type, file_path)` aggregation; the
    /// trait surface accepts the raw value.
    pub summary: String,
    /// `deltas_incorporated INTEGER` — set to the group size.
    pub deltas_incorporated: i64,
    /// `last_updated TEXT` — latest `created_at` in the group.
    pub last_updated: Option<String>,
    /// `confidence REAL DEFAULT 0.5`.
    pub confidence: f64,
}

/// Warm-tier `warm_decisions` row mirroring master-plan §11
/// lines 1285-1293.
#[derive(Debug, Clone, PartialEq)]
pub struct WarmDecisionRow {
    /// `title TEXT NOT NULL` — first 80 chars of the source
    /// `description` (or the source `title` if non-empty).
    pub title: String,
    /// `key_phrases TEXT` (nullable JSON-array blob).
    pub key_phrases: Option<String>,
    /// `related_entities TEXT` (nullable JSON-array blob).
    pub related_entities: Option<String>,
    /// `source_material_ids TEXT` — JSON-array blob of the
    /// `hot_decision_material.id` values that fed this row.
    pub source_material_ids: Option<String>,
    /// `confidence REAL DEFAULT 0.5`.
    pub confidence: f64,
}

// ── Trait — the dependency-inversion seam ─────────────────────────────────

/// Dependency-inversion seam between the warm-tier processors and the
/// underlying [`ucil_core::knowledge_graph::KnowledgeGraph`] handle.
///
/// Per `DEC-0008` §4 ("UCIL-owned trait dep-inversion seam") this
/// trait is UCIL-owned — it is NOT a re-export or adapter of any
/// external wire format. Production impls MUST own a real
/// `KnowledgeGraph` handle; the [`WarmProcessorSource`] trait is the
/// boundary the processors talk through. The frozen acceptance test
/// [`test_warm_processors`] supplies a `#[cfg(test)]`
/// `TestWarmProcessorSource` impl living at the bottom of this file
/// per the WO-0048 `#[cfg(test)]` carve-out.
///
/// `Send + Sync + 'static` lets the scheduler hold an
/// `Arc<dyn WarmProcessorSource>` and clone it across the four
/// per-kind tasks spawned by [`AgentScheduler::start`].
#[async_trait::async_trait]
pub trait WarmProcessorSource: Send + Sync + 'static {
    /// Read up to `limit` unpromoted [`HotObservationRow`]s from
    /// `hot_observations WHERE promoted_to_warm = 0`.
    async fn select_unpromoted_observations(
        &self,
        limit: usize,
    ) -> Result<Vec<HotObservationRow>, WarmProcessorError>;

    /// Insert one [`WarmObservationRow`] into `warm_observations`,
    /// returning the new `id`.
    async fn insert_warm_observation(
        &self,
        row: WarmObservationRow,
    ) -> Result<i64, WarmProcessorError>;

    /// Flip `promoted_to_warm = 1` on the given `hot_observations.id`
    /// rows. Empty input is valid (no-op).
    async fn mark_observations_promoted(&self, hot_ids: &[i64]) -> Result<(), WarmProcessorError>;

    /// Read up to `limit` unpromoted [`HotConventionSignalRow`]s from
    /// `hot_convention_signals WHERE promoted = 0`.
    async fn select_unpromoted_convention_signals(
        &self,
        limit: usize,
    ) -> Result<Vec<HotConventionSignalRow>, WarmProcessorError>;

    /// Insert one [`WarmConventionRow`] into `warm_conventions`,
    /// returning the new `id`.
    async fn insert_warm_convention(
        &self,
        row: WarmConventionRow,
    ) -> Result<i64, WarmProcessorError>;

    /// Flip `promoted = 1` on the given
    /// `hot_convention_signals.id` rows.
    async fn mark_convention_signals_promoted(
        &self,
        hot_ids: &[i64],
    ) -> Result<(), WarmProcessorError>;

    /// Read up to `limit` unpromoted [`HotArchitectureDeltaRow`]s
    /// from `hot_architecture_deltas WHERE promoted = 0`.
    async fn select_unpromoted_architecture_deltas(
        &self,
        limit: usize,
    ) -> Result<Vec<HotArchitectureDeltaRow>, WarmProcessorError>;

    /// Upsert one [`WarmArchitectureStateRow`] into
    /// `warm_architecture_state` keyed by an internal natural key
    /// (production impls use `(summary)` collapsed via SHA-1 or a
    /// dedicated key column added in a follow-up migration).
    /// Returns the resulting row's `id`.
    async fn upsert_warm_architecture_state(
        &self,
        row: WarmArchitectureStateRow,
    ) -> Result<i64, WarmProcessorError>;

    /// Flip `promoted = 1` on the given
    /// `hot_architecture_deltas.id` rows.
    async fn mark_architecture_deltas_promoted(
        &self,
        hot_ids: &[i64],
    ) -> Result<(), WarmProcessorError>;

    /// Read up to `limit` unpromoted [`HotDecisionMaterialRow`]s
    /// from `hot_decision_material WHERE promoted = 0`.
    async fn select_unpromoted_decision_material(
        &self,
        limit: usize,
    ) -> Result<Vec<HotDecisionMaterialRow>, WarmProcessorError>;

    /// Insert one [`WarmDecisionRow`] into `warm_decisions`,
    /// returning the new `id`.
    async fn insert_warm_decision(&self, row: WarmDecisionRow) -> Result<i64, WarmProcessorError>;

    /// Flip `promoted = 1` on the given
    /// `hot_decision_material.id` rows.
    async fn mark_decision_material_promoted(
        &self,
        hot_ids: &[i64],
    ) -> Result<(), WarmProcessorError>;
}

// ── Helpers ───────────────────────────────────────────────────────────────

/// Token-overlap (Jaccard) similarity over whitespace-split tokens.
///
/// Used by [`run_observation_processor`] to cluster hot rows whose
/// `raw_text` overlap meets [`OBSERVATION_DEDUP_THRESHOLD`]. Hand-
/// rolled instead of a third-party crate per
/// `.claude/rules/rust-style.md` §`Crate layout` ("keep `ucil-daemon`
/// lean").
///
/// Returns `0.0` for two empty strings. The clamp to `[0.0, 1.0]` is
/// intrinsic — the ratio is bounded by definition.
fn jaccard_similarity(a: &str, b: &str) -> f64 {
    let tokens_a: std::collections::HashSet<&str> = a.split_whitespace().collect();
    let tokens_b: std::collections::HashSet<&str> = b.split_whitespace().collect();
    let inter = tokens_a.intersection(&tokens_b).count();
    let union = tokens_a.union(&tokens_b).count();
    #[allow(clippy::cast_precision_loss)]
    if union == 0 {
        0.0
    } else {
        inter as f64 / union as f64
    }
}

/// Wrap any [`WarmProcessorSource`] async call in
/// [`WARM_PROCESSOR_OP_DEADLINE`].
///
/// Returns [`WarmProcessorError::Timeout`] on elapse so each
/// processor can `?`-propagate the error into the per-tick
/// [`WarmPromotionResult`] without ever forcing the per-op deadline
/// up to the caller. Mirrors the WO-0068 / G3 / G7 / G8 unconditional
/// per-op `const`-deadline pattern.
async fn with_op_deadline<T, F>(fut: F) -> Result<T, WarmProcessorError>
where
    F: std::future::Future<Output = Result<T, WarmProcessorError>>,
{
    tokio::time::timeout(WARM_PROCESSOR_OP_DEADLINE, fut)
        .await
        .unwrap_or_else(|_| Err(WarmProcessorError::Timeout))
}

/// Derive the `warm_decisions.title` for a hot-decision-material row.
///
/// Returns the source `title` when non-empty, the first 80 chars of
/// the source `description` when `title` is `None` / empty, or an
/// empty string when both are absent. Extracted into a helper so the
/// decision-linker tick body avoids the nested `if let / else` shape
/// that triggers `clippy::option_if_let_else`.
fn derive_decision_title(title: Option<&String>, description: Option<&String>) -> String {
    title.filter(|t| !t.is_empty()).map_or_else(
        || {
            description
                .map(|d| d.chars().take(80).collect::<String>())
                .unwrap_or_default()
        },
        Clone::clone,
    )
}

/// Cluster a `hot_observations` slice by `(related_symbol,
/// raw_text-similarity)` ≥ [`OBSERVATION_DEDUP_THRESHOLD`].
///
/// Two rows belong to the same cluster iff:
///
/// * they share the same `related_symbol` (with `None` treated as a
///   distinct bucket equal only to other `None` rows), AND
/// * their `raw_text` Jaccard similarity is ≥
///   [`OBSERVATION_DEDUP_THRESHOLD`].
///
/// Single-pass disjoint-union: each row is added to the first
/// cluster whose representative satisfies both conditions, otherwise
/// it starts a new cluster. The resulting cluster vec is ordered by
/// first appearance — deterministic for any given input order.
fn cluster_observations(rows: &[HotObservationRow], threshold: f64) -> Vec<Vec<HotObservationRow>> {
    let mut clusters: Vec<Vec<HotObservationRow>> = Vec::new();
    'outer: for row in rows {
        for cluster in &mut clusters {
            let rep = &cluster[0];
            if rep.related_symbol == row.related_symbol
                && jaccard_similarity(&rep.raw_text, &row.raw_text) >= threshold
            {
                cluster.push(row.clone());
                continue 'outer;
            }
        }
        clusters.push(vec![row.clone()]);
    }
    clusters
}

// ── Per-kind processor functions ──────────────────────────────────────────

/// Run one observation-processor tick.
///
/// Reads up to [`WARM_PROCESSOR_BATCH_SIZE`] unpromoted
/// `hot_observations` rows, clusters them via
/// [`cluster_observations`] under
/// [`OBSERVATION_DEDUP_THRESHOLD`], inserts one
/// `warm_observations` row per cluster
/// (`evidence_count = cluster.len()`), and marks every contributing
/// hot row as promoted.
///
/// Per master-plan §15.2 line 1521 the function carries a
/// `tracing::instrument` span named `ucil.agent.warm_processor` with
/// `kind` field. The `kind` field is supplied as a literal so the
/// span value matches across tick boundaries (the surrounding
/// scheduler-level tick span carries the same `kind` field for
/// correlation).
///
/// # Errors
///
/// Returns [`WarmProcessorError::Source`] /
/// [`WarmProcessorError::Database`] when the underlying
/// [`WarmProcessorSource`] call fails, or
/// [`WarmProcessorError::Timeout`] when any single source call
/// exceeds [`WARM_PROCESSOR_OP_DEADLINE`].
#[tracing::instrument(
    name = "ucil.agent.warm_processor",
    level = "debug",
    skip(source),
    fields(kind = "observation")
)]
pub async fn run_observation_processor<S>(
    source: &S,
) -> Result<WarmPromotionResult, WarmProcessorError>
where
    S: WarmProcessorSource + ?Sized,
{
    let kind = WarmProcessorKind::Observation;
    let hot_rows =
        with_op_deadline(source.select_unpromoted_observations(WARM_PROCESSOR_BATCH_SIZE)).await?;
    let hot_rows_examined = hot_rows.len() as u64;
    if hot_rows.is_empty() {
        return Ok(WarmPromotionResult {
            kind,
            hot_rows_examined: 0,
            warm_rows_inserted: 0,
            hot_rows_marked_promoted: 0,
            error: None,
        });
    }
    let clusters = cluster_observations(&hot_rows, OBSERVATION_DEDUP_THRESHOLD);
    let mut warm_rows_inserted: u64 = 0;
    let mut hot_ids_to_promote: Vec<i64> = Vec::with_capacity(hot_rows.len());
    for cluster in &clusters {
        // Pick the longest raw_text as the representative summary.
        let representative = cluster
            .iter()
            .max_by_key(|r| r.raw_text.len())
            .map_or_else(String::new, |r| r.raw_text.clone());
        let first_seen = cluster
            .iter()
            .map(|r| r.created_at.clone())
            .min()
            .unwrap_or_default();
        let last_seen = cluster
            .iter()
            .map(|r| r.created_at.clone())
            .max()
            .unwrap_or_default();
        let related_symbols: Vec<String> = {
            let mut set = std::collections::BTreeSet::new();
            for row in cluster {
                if let Some(sym) = &row.related_symbol {
                    set.insert(sym.clone());
                }
            }
            set.into_iter().collect()
        };
        let related_entities = if related_symbols.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&related_symbols).unwrap_or_default())
        };
        let warm = WarmObservationRow {
            text: representative,
            domains: None,
            related_entities,
            severity: None,
            evidence_count: i64::try_from(cluster.len()).unwrap_or(i64::MAX),
            first_seen: if first_seen.is_empty() {
                None
            } else {
                Some(first_seen)
            },
            last_seen: if last_seen.is_empty() {
                None
            } else {
                Some(last_seen)
            },
            confidence: 0.6,
        };
        with_op_deadline(source.insert_warm_observation(warm)).await?;
        warm_rows_inserted += 1;
        for row in cluster {
            hot_ids_to_promote.push(row.id);
        }
    }
    with_op_deadline(source.mark_observations_promoted(&hot_ids_to_promote)).await?;
    let hot_rows_marked_promoted = hot_ids_to_promote.len() as u64;
    Ok(WarmPromotionResult {
        kind,
        hot_rows_examined,
        warm_rows_inserted,
        hot_rows_marked_promoted,
        error: None,
    })
}

/// Run one convention-signal-processor tick.
///
/// Reads up to [`WARM_PROCESSOR_BATCH_SIZE`] unpromoted
/// `hot_convention_signals` rows, groups them by `pattern_hash`, and
/// promotes a group only when its size meets
/// [`CONVENTION_MIN_EVIDENCE`]. For each qualifying group, inserts
/// one `warm_conventions` row (`evidence_count` = group size) and
/// marks every contributing hot row as promoted. Groups under the
/// threshold are left unpromoted so a future tick can re-evaluate.
///
/// # Errors
///
/// Returns [`WarmProcessorError::Source`] /
/// [`WarmProcessorError::Database`] when the underlying
/// [`WarmProcessorSource`] call fails, or
/// [`WarmProcessorError::Timeout`] when any single source call
/// exceeds [`WARM_PROCESSOR_OP_DEADLINE`].
#[tracing::instrument(
    name = "ucil.agent.warm_processor",
    level = "debug",
    skip(source),
    fields(kind = "convention_signal")
)]
pub async fn run_convention_signal_processor<S>(
    source: &S,
) -> Result<WarmPromotionResult, WarmProcessorError>
where
    S: WarmProcessorSource + ?Sized,
{
    let kind = WarmProcessorKind::ConventionSignal;
    let hot_rows =
        with_op_deadline(source.select_unpromoted_convention_signals(WARM_PROCESSOR_BATCH_SIZE))
            .await?;
    let hot_rows_examined = hot_rows.len() as u64;
    if hot_rows.is_empty() {
        return Ok(WarmPromotionResult {
            kind,
            hot_rows_examined: 0,
            warm_rows_inserted: 0,
            hot_rows_marked_promoted: 0,
            error: None,
        });
    }
    let mut groups: BTreeMap<String, Vec<HotConventionSignalRow>> = BTreeMap::new();
    for row in &hot_rows {
        groups
            .entry(row.pattern_hash.clone())
            .or_default()
            .push(row.clone());
    }
    let mut warm_rows_inserted: u64 = 0;
    let mut hot_ids_to_promote: Vec<i64> = Vec::new();
    for (pattern_hash, group) in &groups {
        if group.len() >= CONVENTION_MIN_EVIDENCE {
            let examples: Vec<String> = group
                .iter()
                .filter_map(|r| r.example_snippet.clone())
                .collect();
            let examples_json = if examples.is_empty() {
                None
            } else {
                Some(serde_json::to_string(&examples).unwrap_or_default())
            };
            let warm = WarmConventionRow {
                category: pattern_hash.clone(),
                pattern_description: format!(
                    "convention pattern {} observed in {} files",
                    pattern_hash,
                    group.len()
                ),
                examples: examples_json,
                evidence_count: i64::try_from(group.len()).unwrap_or(i64::MAX),
                confidence: 0.5,
            };
            with_op_deadline(source.insert_warm_convention(warm)).await?;
            warm_rows_inserted += 1;
            for row in group {
                hot_ids_to_promote.push(row.id);
            }
        }
    }
    with_op_deadline(source.mark_convention_signals_promoted(&hot_ids_to_promote)).await?;
    let hot_rows_marked_promoted = hot_ids_to_promote.len() as u64;
    Ok(WarmPromotionResult {
        kind,
        hot_rows_examined,
        warm_rows_inserted,
        hot_rows_marked_promoted,
        error: None,
    })
}

/// Run one architecture-delta-processor tick.
///
/// Reads up to [`WARM_PROCESSOR_BATCH_SIZE`] unpromoted
/// `hot_architecture_deltas` rows, aggregates by
/// `(change_type, file_path)`, and upserts one
/// `warm_architecture_state` row per group. The summary text mentions
/// every contributing delta's `change_type` and `file_path` so a
/// follow-up reader can correlate the warm row back to its hot
/// origin without re-querying.
///
/// # Errors
///
/// Returns [`WarmProcessorError::Source`] /
/// [`WarmProcessorError::Database`] when the underlying
/// [`WarmProcessorSource`] call fails, or
/// [`WarmProcessorError::Timeout`] when any single source call
/// exceeds [`WARM_PROCESSOR_OP_DEADLINE`].
#[tracing::instrument(
    name = "ucil.agent.warm_processor",
    level = "debug",
    skip(source),
    fields(kind = "architecture_delta")
)]
pub async fn run_architecture_delta_processor<S>(
    source: &S,
) -> Result<WarmPromotionResult, WarmProcessorError>
where
    S: WarmProcessorSource + ?Sized,
{
    let kind = WarmProcessorKind::ArchitectureDelta;
    let hot_rows =
        with_op_deadline(source.select_unpromoted_architecture_deltas(WARM_PROCESSOR_BATCH_SIZE))
            .await?;
    let hot_rows_examined = hot_rows.len() as u64;
    if hot_rows.is_empty() {
        return Ok(WarmPromotionResult {
            kind,
            hot_rows_examined: 0,
            warm_rows_inserted: 0,
            hot_rows_marked_promoted: 0,
            error: None,
        });
    }
    let mut groups: BTreeMap<(String, String), Vec<HotArchitectureDeltaRow>> = BTreeMap::new();
    for row in &hot_rows {
        groups
            .entry((row.change_type.clone(), row.file_path.clone()))
            .or_default()
            .push(row.clone());
    }
    let mut warm_rows_inserted: u64 = 0;
    let mut hot_ids_to_promote: Vec<i64> = Vec::with_capacity(hot_rows.len());
    for ((change_type, file_path), group) in &groups {
        let last_updated = group.iter().map(|r| r.created_at.clone()).max();
        let summary = format!(
            "{} delta(s) of type {} on {}",
            group.len(),
            change_type,
            file_path
        );
        let warm = WarmArchitectureStateRow {
            summary,
            deltas_incorporated: i64::try_from(group.len()).unwrap_or(i64::MAX),
            last_updated,
            confidence: 0.5,
        };
        with_op_deadline(source.upsert_warm_architecture_state(warm)).await?;
        warm_rows_inserted += 1;
        for row in group {
            hot_ids_to_promote.push(row.id);
        }
    }
    with_op_deadline(source.mark_architecture_deltas_promoted(&hot_ids_to_promote)).await?;
    let hot_rows_marked_promoted = hot_ids_to_promote.len() as u64;
    Ok(WarmPromotionResult {
        kind,
        hot_rows_examined,
        warm_rows_inserted,
        hot_rows_marked_promoted,
        error: None,
    })
}

/// Run one decision-linker-processor tick.
///
/// Reads up to [`WARM_PROCESSOR_BATCH_SIZE`] unpromoted
/// `hot_decision_material` rows, filters to rows with non-null
/// `affected_files`, and inserts one `warm_decisions` row per
/// qualifying hot row. The warm row's `title` is the source `title`
/// when present, otherwise the first 80 chars of the source
/// `description` (or empty if both are null).
///
/// Hot rows with `affected_files = NULL` are NOT promoted — they
/// stay unpromoted so a future tick can re-evaluate once the source
/// material has been enriched (e.g., a PR diff fills the field
/// later).
///
/// # Errors
///
/// Returns [`WarmProcessorError::Source`] /
/// [`WarmProcessorError::Database`] when the underlying
/// [`WarmProcessorSource`] call fails, or
/// [`WarmProcessorError::Timeout`] when any single source call
/// exceeds [`WARM_PROCESSOR_OP_DEADLINE`].
#[tracing::instrument(
    name = "ucil.agent.warm_processor",
    level = "debug",
    skip(source),
    fields(kind = "decision_linker")
)]
pub async fn run_decision_linker_processor<S>(
    source: &S,
) -> Result<WarmPromotionResult, WarmProcessorError>
where
    S: WarmProcessorSource + ?Sized,
{
    let kind = WarmProcessorKind::DecisionLinker;
    let hot_rows =
        with_op_deadline(source.select_unpromoted_decision_material(WARM_PROCESSOR_BATCH_SIZE))
            .await?;
    let hot_rows_examined = hot_rows.len() as u64;
    if hot_rows.is_empty() {
        return Ok(WarmPromotionResult {
            kind,
            hot_rows_examined: 0,
            warm_rows_inserted: 0,
            hot_rows_marked_promoted: 0,
            error: None,
        });
    }
    let mut warm_rows_inserted: u64 = 0;
    let mut hot_ids_to_promote: Vec<i64> = Vec::new();
    for row in &hot_rows {
        if row.affected_files.is_none() {
            continue;
        }
        let title = derive_decision_title(row.title.as_ref(), row.description.as_ref());
        let warm = WarmDecisionRow {
            title,
            key_phrases: None,
            related_entities: None,
            source_material_ids: Some(serde_json::to_string(&[row.id]).unwrap_or_default()),
            confidence: 0.5,
        };
        with_op_deadline(source.insert_warm_decision(warm)).await?;
        warm_rows_inserted += 1;
        hot_ids_to_promote.push(row.id);
    }
    with_op_deadline(source.mark_decision_material_promoted(&hot_ids_to_promote)).await?;
    let hot_rows_marked_promoted = hot_ids_to_promote.len() as u64;
    Ok(WarmPromotionResult {
        kind,
        hot_rows_examined,
        warm_rows_inserted,
        hot_rows_marked_promoted,
        error: None,
    })
}
