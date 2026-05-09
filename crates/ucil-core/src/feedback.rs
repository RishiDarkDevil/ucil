//! Post-hoc feedback-loop analyser — feature `P3-W11-F12`.
//!
//! Master-plan citations driving this module:
//!
//! * §6.3 lines 626-639 — response-assembly pipeline `[Feedback
//!   Analyzer]`: "Compare with previous response's bonus context //
//!   Track whether agent used pitfalls, followed conventions, read
//!   related code // Boost/decay importance scores accordingly".
//! * §8.7 lines 824-844 — the 4-signal taxonomy: pitfall→Used+0.1,
//!   convention→Followed+0.05, quality_issue→Fixed/lint-priority-boost,
//!   related_code→Used+relevance-boost.
//! * §12.1 lines 1295-1303 — `feedback_signals` `SQLite` schema
//!   (`id`, `session_id`, `bonus_type`, `bonus_id`, `signal`,
//!   `timestamp`).  The
//!   table is already present in `INIT_SQL` at
//!   `crates/ucil-core/src/knowledge_graph.rs:575` per the WO-0007
//!   storage-test selector + WO-0024 KG CRUD seed; this analyser
//!   persists into it via the [`FeedbackPersistence`]
//!   dependency-inversion seam.
//! * §12.4 line 1370 — decay/aggregation policy: "Aggregate
//!   per-bonus-type monthly, delete raw signals >30 days".
//! * §17 line 1637 — `feedback.rs` joins the canonical
//!   `crates/ucil-core/src/` directory layout as a sibling of
//!   `tier_merger.rs`, `bonus_selector.rs`, and `warm_processors.rs`.
//! * §18 Phase 3 Week 11 deliverable #7 line 1823 — "Feedback loop:
//!   post-hoc analyzer tracking bonus context usage".
//!
//! Implementation issued by `WO-0096`.  This module ships the
//! pure-deterministic analyser surface only — production-wired
//! persistence + KG importance updates (a `KgFeedbackPersistence`
//! impl reading/writing the §12.1 `feedback_signals` table AND
//! mutating `entities.importance` / `conventions.confidence` etc.)
//! are deferred to a Phase-4 daemon-side wiring WO.  Same shape as
//! the `G1Source` / `G2Source` / `G3Source` / `G4Source` /
//! `G7Source` / `G8Source` / `BonusContextSource` /
//! `WarmProcessorSource` production-wiring deferrals (WO-0047 /
//! 0050+0051 / 0070 / 0073+0083 / 0085+0090 / 0089+0090 /
//! 0088 / 0093).
//!
//! # Design — UCIL-owned dependency-inversion seam
//!
//! Per `DEC-0008` §4 the dependency-inversion seam between this
//! analyser and the production back-ends (KG `feedback_signals`
//! writes; KG importance/confidence mutations) IS a UCIL-owned
//! trait — [`FeedbackPersistence`].  Production impls live in
//! `crates/ucil-daemon/` and own a real `KnowledgeGraph` handle.
//! The `#[cfg(test)]` `TestFeedbackPersistence` impl in this
//! module is the in-process trait-instantiation site, NOT an
//! external-wire-format substitute.
//!
//! The trait surface is intentionally synchronous — production
//! impls that need IO MAY internally use
//! [`tokio::task::block_in_place`] or buffer into eagerly-computed
//! [`Vec<FeedbackSignalRecord>`] / [`Vec<ImportanceAdjustment>`].
//! Keeping the trait sync lets [`analyze_post_hoc`] be called from
//! any execution context (sync or async) without `Pin<Box<...>>`
//! orchestration overhead.  Same precedent: the
//! `BonusContextSource` (WO-0088), `G7Source` (WO-0085),
//! `WarmProcessorSource` (WO-0093) traits are also sync.
//!
//! # Determinism
//!
//! [`analyze_post_hoc`] is pure-deterministic: same
//! `(session_id, prior_bonuses, next_call, now_iso, options)` ⇒
//! same [`FeedbackAnalysisOutcome`].  The caller supplies
//! `now_iso` so the function carries no IO / clock side-effects
//! (mirrors the WO-0084 `tier_merger.rs` `observed_at: SystemTime`
//! injection pattern + the WO-0095 single-arg `now_iso` style).
//! No `HashMap` / `HashSet`; no `chrono::Utc::now()`; no logging;
//! no async; no instrumentation spans; no `regex`.  Membership
//! tests use linear scans over `Vec` (master-plan §6.3 line 666
//! sub-100-element session-dedup-files volume).
//!
//! # Tracing
//!
//! Master-plan §15.2 tracing does NOT apply (pure-deterministic
//! CPU-bound projection — no async, no IO, no spawn).  Production
//! impls of [`FeedbackPersistence`] in `ucil-daemon` carry tracing
//! span annotations at the IO boundary.  See `WO-0067`
//! §`lessons_applied #5` + `WO-0084` §`scope_in #12` + `WO-0088`
//! §`scope_in §15.2 carve-out` for the deterministic-fallback
//! module carve-out precedent.

#![allow(clippy::too_long_first_doc_paragraph)]
#![deny(rustdoc::broken_intra_doc_links)]

use std::ops::RangeInclusive;
use std::path::PathBuf;

// ── BonusType: the 5 master-plan §12.1 categories ────────────────────────────

/// One of the 5 §12.1 `feedback_signals.bonus_type` allowed values.
///
/// Variants match the §12.1 line 1299 schema-comment verbatim
/// (`'pitfall'|'convention'|'related_code'|'quality_issue'|'test'`).
/// Production impls of [`FeedbackPersistence`] translate the enum to
/// lowercase strings before the `INSERT INTO feedback_signals`.
///
/// NB: This enum does NOT mirror the 8 [`crate::BonusEntries`]
/// field names from `bonus_selector.rs` (`conventions`, `pitfalls`,
/// `quality_issues`, `related_code`, `tests_to_update`,
/// `blast_radius`, `history`, `security`).  The analyser tracks
/// ONLY the 5 §12.1-schema-supported categories per master-plan
/// §8.7's explicit signal taxonomy.  `blast_radius`, `history`,
/// `security` are tracked by other §8 mechanisms; feedback for
/// those flows lands in a follow-up WO if/when the §12.1 schema
/// is extended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BonusType {
    /// Pitfall bonus (e.g., `"PaymentGateway not idempotent"`).
    /// Master-plan §8.7 line 826-832.
    Pitfall,
    /// Convention bonus (e.g., `"Use thiserror + ModuleError"`).
    /// Master-plan §8.7 line 838-841.
    Convention,
    /// Related-code bonus (e.g., `"Retry utility in
    /// src/utils/retry.rs"`).  Master-plan §8.7 line 843-844.
    RelatedCode,
    /// Quality-issue bonus (e.g., `"Type error on line 42"`).
    /// Master-plan §8.7 line 833-836.
    QualityIssue,
    /// Test bonus (a test that exercises the hit's code, surfaced
    /// from the §6.5 G7 test-impact subgraph).  Master-plan §12.1
    /// line 1299 lists `'test'` as the fifth category alongside
    /// the four §8.7 named flows.
    Test,
}

// ── FeedbackSignal: the 4 master-plan §12.1 signal values ────────────────────

/// One of the 4 §12.1 `feedback_signals.signal` allowed values.
///
/// Variants match the §12.1 line 1301 schema-comment verbatim
/// (`'used'|'followed'|'ignored'|'fixed'`).  The §8.7 4-signal
/// taxonomy maps as follows:
///
/// * `Pitfall`/`RelatedCode`/`Test` ⇒ [`Used`](Self::Used) on match.
/// * `Convention` ⇒ [`Followed`](Self::Followed) on match.
/// * `QualityIssue` ⇒ [`Fixed`](Self::Fixed) on match.
/// * Any unmatched bonus ⇒ [`Ignored`](Self::Ignored).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FeedbackSignal {
    /// The agent used the bonus (pitfall keyword in reason,
    /// related-code file in `files_in_context`, test file in
    /// either `files_in_context` or an edit overlap).
    Used,
    /// The agent followed the bonus (convention keyword in some
    /// edit's `content_after`).
    Followed,
    /// The agent did not engage with the bonus (no rule matched).
    /// Subject to the §12.4 line 1370 30-day decay policy.
    Ignored,
    /// The agent fixed the bonus (quality-issue overlap with an
    /// edit's `edited_lines`).
    Fixed,
}

// ── BonusReference: per-bonus record UCIL sent in response N-1 ───────────────

/// A single bonus payload UCIL emitted in response N-1 — the unit
/// the analyser compares against the agent's NEXT call.
///
/// Production wiring derives this from `HitWithBonus` +
/// `AssembledResponse` + the §12.1 `feedback_signals.bonus_id`
/// autoincrement sequence (production wiring is OUT OF SCOPE per
/// `WO-0096` `scope_out` #1 — see Phase-4 daemon-side wiring WO).
///
/// Field semantics:
///
/// * `bonus_id` is `Option<i64>` because not every bonus payload
///   has a stable `feedback_signals.bonus_id` row yet (the
///   persistence layer assigns it on first persist).
/// * `file_path` / `line_range` / `keyword` carry the ANCHORS
///   needed to compare against the agent's next call:
///   - `file_path` for [`BonusType::RelatedCode`] /
///     [`BonusType::QualityIssue`] / [`BonusType::Test`].
///   - `line_range` for [`BonusType::QualityIssue`] overlap
///     detection.
///   - `keyword` for [`BonusType::Pitfall`] (substring in
///     `next_call.reason`) / [`BonusType::Convention`]
///     (substring in `edit.content_after`).
/// * `RangeInclusive<u32>` is intentional (NOT `Range<u32>`) so
///   single-line issues are expressible as `42..=42`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BonusReference {
    /// MCP session ID under which UCIL emitted the bonus.
    pub session_id: String,
    /// One of the 5 §12.1 categories this bonus belongs to.
    pub bonus_type: BonusType,
    /// `Some(_)` after the persistence layer has assigned an
    /// autoincrement row id; `None` for first-emission references.
    pub bonus_id: Option<i64>,
    /// File the bonus anchors at (relevant for `RelatedCode`,
    /// `QualityIssue`, `Test`).
    pub file_path: Option<PathBuf>,
    /// Line range the bonus anchors at (relevant for
    /// `QualityIssue` overlap detection).
    pub line_range: Option<RangeInclusive<u32>>,
    /// Keyword the bonus anchors at (relevant for `Pitfall`
    /// reason-substring match + `Convention`
    /// content-after-substring match).
    pub keyword: Option<String>,
}

// ── EditObservation: a single post-call edit the agent performed ─────────────

/// A single edit observation extracted from the agent's NEXT call.
///
/// `content_after` carries the post-edit text snippet that the
/// analyser scans for convention keyword matches per master-plan
/// §8.7 line 837-839 (`UCIL returns convention: "Use thiserror +
/// ModuleError" → Agent's next edit creates a ModuleError enum
/// with thiserror → SIGNAL: convention was FOLLOWED`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditObservation {
    /// File the edit applied to.
    pub file_path: PathBuf,
    /// Inclusive line range affected by the edit (post-edit
    /// numbering — i.e., the range the agent said it edited).
    pub edited_lines: RangeInclusive<u32>,
    /// Post-edit text snippet covering the edited region.
    pub content_after: String,
}

// ── AgentNextCall: the input-side aggregate ──────────────────────────────────

/// Aggregate of the agent's NEXT call after UCIL response N-1.
///
/// `files_in_context` uses [`Vec<PathBuf>`] (NOT `HashSet<PathBuf>`)
/// to keep determinism without adding a `Hash` bound at the
/// type-surface level.  Membership tests inside [`analyze_post_hoc`]
/// use a linear scan over the typical sub-100-element list per
/// master-plan §6.3 line 666 session-dedup-files volume.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AgentNextCall {
    /// Free-form reason the agent supplied for the call (scanned
    /// for `Pitfall` keyword substring matches).
    pub reason: Option<String>,
    /// Files the agent has in active context for this call
    /// (scanned for `RelatedCode` / `Test` file matches).
    pub files_in_context: Vec<PathBuf>,
    /// Edits the agent performed since UCIL response N-1
    /// (scanned for `Convention` content-after substring matches
    /// + `QualityIssue` line-range overlaps + `Test` overlap).
    pub edits: Vec<EditObservation>,
}

// ── FeedbackSignalRecord: 1:1 with the §12.1 schema columns ──────────────────

/// One row destined for the §12.1 `feedback_signals` table.
///
/// Field order + names mirror the §12.1 schema columns 1:1
/// (excluding the autoincrement `id` PRIMARY KEY which the
/// persistence layer assigns).  `timestamp_iso` is an RFC-3339
/// string (e.g., `"2026-05-09T08:42:51Z"`) per the schema's
/// `TEXT NOT NULL DEFAULT (datetime('now'))` — production impls
/// of [`FeedbackPersistence::persist`] write this directly into
/// the `timestamp` column without further parsing.
///
/// Using [`String`] (NOT `chrono::DateTime<Utc>`) at the surface
/// keeps the trait serde-friendly without forcing a `chrono` dep
/// on consumers (chrono IS in `crates/ucil-core/Cargo.toml` direct
/// deps but the analyser surface DOES NOT introduce it — caller
/// passes the string).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedbackSignalRecord {
    /// MCP session ID from the originating [`BonusReference`].
    pub session_id: String,
    /// One of the 5 §12.1 categories.
    pub bonus_type: BonusType,
    /// Autoincrement row id from the source table (`Option<i64>`
    /// for first-emission references where the persistence layer
    /// has not yet assigned a row id).
    pub bonus_id: Option<i64>,
    /// One of the 4 §12.1 signal values.
    pub signal: FeedbackSignal,
    /// RFC-3339 timestamp at which the analyser observed the
    /// signal.  Caller-supplied via the [`analyze_post_hoc`]
    /// `now_iso` argument.
    pub timestamp_iso: String,
}

// ── ImportanceAdjustment: a delta to a bonus row's importance ────────────────

/// A signed importance delta for a bonus row.
///
/// `delta` is signed: positive = boost, negative = decay (per
/// master-plan §8.7 lines 824-844 + §12.4 line 1370).  Production
/// impls of [`FeedbackPersistence::apply_adjustments`] mutate the
/// underlying row's `importance` / `confidence` / `priority`
/// column in the KG — Phase-4 daemon-side wiring WO ships that
/// layer.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportanceAdjustment {
    /// One of the 5 §12.1 categories.
    pub bonus_type: BonusType,
    /// Autoincrement row id from the source table.
    pub bonus_id: Option<i64>,
    /// Signed delta to apply to the underlying row's importance /
    /// confidence / priority column.
    pub delta: f64,
}

// ── FeedbackAnalysisOutcome: the [`analyze_post_hoc`] return type ────────────

/// Aggregate output of one [`analyze_post_hoc`] call.
///
/// Both vectors are emitted in `prior_bonuses` input order (no
/// sorting — strict input-preserving projection).  `signals.len()`
/// always equals `adjustments.len()` always equals
/// `prior_bonuses.len()` (the per-bonus dispatch always emits
/// exactly ONE signal + ONE adjustment).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FeedbackAnalysisOutcome {
    /// One [`FeedbackSignalRecord`] per input bonus, input order.
    pub signals: Vec<FeedbackSignalRecord>,
    /// One [`ImportanceAdjustment`] per input bonus, input order.
    pub adjustments: Vec<ImportanceAdjustment>,
}

// ── FeedbackAnalysisOptions: tuning knobs for [`analyze_post_hoc`] ───────────

/// Tuning knobs for [`analyze_post_hoc`].
///
/// Defaults are pinned to master-plan §8.7 lines 824-844 verbatim:
///
/// * `pitfall_used_boost = 0.1` (§8.7 line 831).
/// * `convention_followed_boost = 0.05` (§8.7 line 839).
/// * `related_code_used_boost = 0.05` (§8.7 line 843 — relevance
///   boost mirrored to the convention magnitude).
/// * `quality_issue_fixed_boost = 0.1` (§8.7 line 835 — lint
///   priority boost mirrored to the pitfall magnitude).
/// * `test_used_boost = 0.05` (mirrored to the related-code
///   magnitude — the §12.1 fifth category).
/// * `ignored_decay = -0.01` (§12.4 line 1370 30-day-decay
///   baseline divided by 30 days = -0.01 per signal).
///
/// # Examples
///
/// ```
/// use ucil_core::FeedbackAnalysisOptions;
///
/// let defaults = FeedbackAnalysisOptions::default();
/// assert!((defaults.pitfall_used_boost - 0.1).abs() < f64::EPSILON);
/// assert!((defaults.convention_followed_boost - 0.05).abs() < f64::EPSILON);
/// assert!((defaults.ignored_decay - -0.01).abs() < f64::EPSILON);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FeedbackAnalysisOptions {
    /// Boost for a `Pitfall` whose keyword matched in
    /// `next_call.reason`.  Master-plan §8.7 line 831 pins this
    /// to `+0.1`.
    pub pitfall_used_boost: f64,
    /// Boost for a `Convention` whose keyword matched in some
    /// `edit.content_after`.  Master-plan §8.7 line 839 pins
    /// this to `+0.05`.
    pub convention_followed_boost: f64,
    /// Boost for a `RelatedCode` whose `file_path` is in
    /// `next_call.files_in_context`.  Master-plan §8.7 line 843
    /// pins this to `+0.05`.
    pub related_code_used_boost: f64,
    /// Boost for a `QualityIssue` whose `(file_path, line_range)`
    /// overlaps an edit's `(file_path, edited_lines)`.
    /// Master-plan §8.7 line 835 pins this to `+0.1`.
    pub quality_issue_fixed_boost: f64,
    /// Boost for a `Test` whose `file_path` is in
    /// `next_call.files_in_context` OR overlaps an edit.
    /// Mirrored to the related-code magnitude `+0.05`.
    pub test_used_boost: f64,
    /// Decay applied to any bonus that did not match any of the
    /// per-type rules.  Master-plan §12.4 line 1370 30-day-decay
    /// baseline divided by 30 days = `-0.01`.
    pub ignored_decay: f64,
}

impl Default for FeedbackAnalysisOptions {
    fn default() -> Self {
        Self {
            pitfall_used_boost: 0.1,
            convention_followed_boost: 0.05,
            related_code_used_boost: 0.05,
            quality_issue_fixed_boost: 0.1,
            test_used_boost: 0.05,
            ignored_decay: -0.01,
        }
    }
}

// ── FeedbackError ────────────────────────────────────────────────────────────

/// Errors surfaced by production impls of [`FeedbackPersistence`].
///
/// The pure-deterministic [`analyze_post_hoc`] entry point does
/// NOT return [`Result`] — it cannot fail.  The error surface is
/// defined here so production impls of the persistence trait have
/// a typed error to return; future WOs MAY extend the surface via
/// `#[non_exhaustive]`.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FeedbackError {
    /// Underlying `SQLite` write to the `feedback_signals` table or
    /// underlying KG-mutation call failed.
    #[error("persistence failure: {0}")]
    Persistence(String),
    /// `timestamp_iso` field on a [`FeedbackSignalRecord`] was not
    /// RFC-3339-parseable.  Reserved for malformed test fixtures
    /// — the analyser produces RFC-3339 strings via the caller-
    /// supplied `now_iso` so this error is never returned for
    /// outputs of [`analyze_post_hoc`] when the caller supplies
    /// a well-formed timestamp.
    #[error("invalid timestamp: {0}")]
    InvalidTimestamp(String),
}

// ── FeedbackPersistence: the dependency-inversion seam ───────────────────────

/// Synchronous feedback-persistence sink — the UCIL-owned
/// dependency-inversion seam (per `DEC-0008` §4).
///
/// The trait IS the dependency-inversion seam — production impls
/// live in `crates/ucil-daemon/` and own a real `KnowledgeGraph`
/// handle.  The `#[cfg(test)]` `TestFeedbackPersistence` impl in
/// this module is the in-process trait-instantiation site, NOT
/// an external-wire-format substitute.
///
/// Production impls (e.g. `KgFeedbackPersistence`) wire the
/// signals to the §12.1 `feedback_signals` `SQLite` table writes
/// AND the importance-adjustment delta-applies to the underlying
/// KG row mutations; they are deferred to a Phase-4 daemon-side
/// wiring WO (same shape as `BonusContextSource` (WO-0088),
/// `WarmProcessorSource` (WO-0093) production-wiring deferrals).
///
/// The trait surface is intentionally synchronous — production
/// impls that need IO MAY internally use
/// [`tokio::task::block_in_place`] or buffer into eagerly-computed
/// [`Vec<FeedbackSignalRecord>`] / [`Vec<ImportanceAdjustment>`].
/// Keeping the trait sync lets [`analyze_post_hoc`] be called
/// from any execution context (sync or async).  Same precedent:
/// `BonusContextSource` (WO-0088), `G7Source` (WO-0085),
/// `WarmProcessorSource` (WO-0093) traits are also sync.
pub trait FeedbackPersistence {
    /// Persist a slice of [`FeedbackSignalRecord`]s to the §12.1
    /// `feedback_signals` table.
    ///
    /// # Errors
    ///
    /// Production impls return [`FeedbackError::Persistence`] when
    /// the underlying `SQLite` write fails AND
    /// [`FeedbackError::InvalidTimestamp`] when `timestamp_iso`
    /// is not RFC-3339-parseable.  The analyser produces RFC-3339
    /// strings via the caller-supplied `now_iso`, so the latter
    /// is reserved for malformed test fixtures.
    fn persist(&self, signals: &[FeedbackSignalRecord]) -> Result<(), FeedbackError>;

    /// Apply a slice of [`ImportanceAdjustment`]s to the
    /// underlying KG rows (`entities.importance` /
    /// `conventions.confidence` / `quality_issues.priority` etc.).
    ///
    /// # Errors
    ///
    /// Production impls return [`FeedbackError::Persistence`] when
    /// the underlying KG mutation fails.
    fn apply_adjustments(&self, adjustments: &[ImportanceAdjustment]) -> Result<(), FeedbackError>;
}

// ── analyze_post_hoc: the pure-deterministic entry point ─────────────────────

/// Compare prior-response bonuses against the agent's NEXT call,
/// emitting one [`FeedbackSignalRecord`] + one
/// [`ImportanceAdjustment`] per input bonus.
///
/// Pure-deterministic projection per `WO-0096` (P3-W11-F12) and
/// master-plan §8.7 + §6.3 line 626-639.  The caller supplies
/// `now_iso` so the function carries no IO / clock side-effects
/// (mirrors WO-0084 `tier_merger.rs` `observed_at: SystemTime`
/// injection pattern + WO-0095 single-arg `now_iso` style).
///
/// §15.2 tracing carve-out applies (pure-deterministic CPU-bound
/// projection) — production impls of [`FeedbackPersistence`] in
/// `ucil-daemon` carry tracing span annotations at the IO boundary.
///
/// Algorithm — per master-plan §8.7 lines 824-844 verbatim:
///
/// For each [`BonusReference`] `b` in input order, dispatch on
/// `b.bonus_type`:
///
/// * [`BonusType::Pitfall`]: if `b.keyword` is set AND its
///   case-insensitive substring is in `next_call.reason` ⇒ emit
///   [`FeedbackSignal::Used`] + `delta = options.pitfall_used_boost`.
/// * [`BonusType::Convention`]: if `b.keyword` is set AND its
///   case-insensitive substring is in some `edit.content_after`
///   ⇒ emit [`FeedbackSignal::Followed`] +
///   `delta = options.convention_followed_boost`.
/// * [`BonusType::RelatedCode`]: if `b.file_path` is set AND it
///   appears in `next_call.files_in_context` ⇒ emit
///   [`FeedbackSignal::Used`] +
///   `delta = options.related_code_used_boost`.
/// * [`BonusType::QualityIssue`]: if `b.file_path` AND
///   `b.line_range` are set AND some `edit.file_path == b.file_path`
///   AND `b.line_range` overlaps `edit.edited_lines` ⇒ emit
///   [`FeedbackSignal::Fixed`] +
///   `delta = options.quality_issue_fixed_boost`.
/// * [`BonusType::Test`]: if `b.file_path` is set AND it appears
///   in `next_call.files_in_context` OR overlaps an edit ⇒ emit
///   [`FeedbackSignal::Used`] +
///   `delta = options.test_used_boost`.
/// * Otherwise (no rule matched): emit
///   [`FeedbackSignal::Ignored`] +
///   `delta = options.ignored_decay`.
///
/// Output ordering preserves `prior_bonuses` input order — no
/// sorting.  Empty `prior_bonuses` ⇒
/// [`FeedbackAnalysisOutcome::default`].
///
/// # Examples
///
/// ```
/// use ucil_core::{
///     analyze_post_hoc, AgentNextCall, BonusReference, BonusType,
///     FeedbackAnalysisOptions, FeedbackSignal,
/// };
///
/// let bonuses = vec![BonusReference {
///     session_id: "sess-42".to_owned(),
///     bonus_type: BonusType::Pitfall,
///     bonus_id: Some(1),
///     file_path: None,
///     line_range: None,
///     keyword: Some("idempotency_key".to_owned()),
/// }];
/// let next_call = AgentNextCall {
///     reason: Some("guard the call with an idempotency_key".to_owned()),
///     files_in_context: vec![],
///     edits: vec![],
/// };
/// let outcome = analyze_post_hoc(
///     "sess-42",
///     &bonuses,
///     &next_call,
///     "2026-05-09T08:42:51Z",
///     &FeedbackAnalysisOptions::default(),
/// );
/// assert_eq!(outcome.signals.len(), 1);
/// assert_eq!(outcome.signals[0].signal, FeedbackSignal::Used);
/// assert!((outcome.adjustments[0].delta - 0.1).abs() < f64::EPSILON);
/// ```
#[must_use]
pub fn analyze_post_hoc(
    session_id: &str,
    prior_bonuses: &[BonusReference],
    next_call: &AgentNextCall,
    now_iso: &str,
    options: &FeedbackAnalysisOptions,
) -> FeedbackAnalysisOutcome {
    let mut signals: Vec<FeedbackSignalRecord> = Vec::with_capacity(prior_bonuses.len());
    let mut adjustments: Vec<ImportanceAdjustment> = Vec::with_capacity(prior_bonuses.len());

    for b in prior_bonuses {
        let (signal, delta) = classify(b, next_call, options);
        signals.push(FeedbackSignalRecord {
            session_id: session_id.to_owned(),
            bonus_type: b.bonus_type,
            bonus_id: b.bonus_id,
            signal,
            timestamp_iso: now_iso.to_owned(),
        });
        adjustments.push(ImportanceAdjustment {
            bonus_type: b.bonus_type,
            bonus_id: b.bonus_id,
            delta,
        });
    }

    FeedbackAnalysisOutcome {
        signals,
        adjustments,
    }
}

/// Per-bonus dispatch — returns the `(signal, delta)` pair for a
/// single [`BonusReference`].
///
/// Module-private helper that keeps the [`analyze_post_hoc`] body
/// terse.  The dispatch table is the load-bearing §8.7 rule
/// surface; M1/M2 mutations target the `Convention` / `Pitfall`
/// arms here.
fn classify(
    b: &BonusReference,
    next_call: &AgentNextCall,
    options: &FeedbackAnalysisOptions,
) -> (FeedbackSignal, f64) {
    match b.bonus_type {
        BonusType::Pitfall => {
            if let Some(keyword) = b.keyword.as_ref() {
                if let Some(reason) = next_call.reason.as_ref() {
                    if reason.to_lowercase().contains(&keyword.to_lowercase()) {
                        return (FeedbackSignal::Used, options.pitfall_used_boost);
                    }
                }
            }
            (FeedbackSignal::Ignored, options.ignored_decay)
        }
        BonusType::Convention => {
            if let Some(keyword) = b.keyword.as_ref() {
                let kw_lc = keyword.to_lowercase();
                if next_call
                    .edits
                    .iter()
                    .any(|e| e.content_after.to_lowercase().contains(&kw_lc))
                {
                    return (FeedbackSignal::Followed, options.convention_followed_boost);
                }
            }
            (FeedbackSignal::Ignored, options.ignored_decay)
        }
        BonusType::RelatedCode => {
            if let Some(file_path) = b.file_path.as_ref() {
                if next_call.files_in_context.iter().any(|p| p == file_path) {
                    return (FeedbackSignal::Used, options.related_code_used_boost);
                }
            }
            (FeedbackSignal::Ignored, options.ignored_decay)
        }
        BonusType::QualityIssue => {
            if let (Some(file_path), Some(line_range)) =
                (b.file_path.as_ref(), b.line_range.as_ref())
            {
                if next_call.edits.iter().any(|e| {
                    &e.file_path == file_path && ranges_overlap(line_range, &e.edited_lines)
                }) {
                    return (FeedbackSignal::Fixed, options.quality_issue_fixed_boost);
                }
            }
            (FeedbackSignal::Ignored, options.ignored_decay)
        }
        BonusType::Test => {
            if let Some(file_path) = b.file_path.as_ref() {
                let in_context = next_call.files_in_context.iter().any(|p| p == file_path);
                let in_edit = next_call.edits.iter().any(|e| &e.file_path == file_path);
                if in_context || in_edit {
                    return (FeedbackSignal::Used, options.test_used_boost);
                }
            }
            (FeedbackSignal::Ignored, options.ignored_decay)
        }
    }
}

/// `true` IFF the two inclusive ranges share at least one integer.
///
/// Master-plan §8.7 line 833-836 quality-issue overlap detection.
/// The standard interval-overlap formula:
/// `max(start_a, start_b) <= min(end_a, end_b)`.
fn ranges_overlap(a: &RangeInclusive<u32>, b: &RangeInclusive<u32>) -> bool {
    let lo = (*a.start()).max(*b.start());
    let hi = (*a.end()).min(*b.end());
    lo <= hi
}

// ── Module-root frozen test (DEC-0007 frozen-selector placement) ─────────────
//
// `test_post_hoc_analyser` lives at module root — NOT inside
// `mod tests { ... }` — so the substring selector
// `cargo test -p ucil-core feedback::test_post_hoc_analyser`
// resolves uniquely without `--exact`.  Per `DEC-0007` +
// WO-0067/0068/0070/0083/0084/0085/0088/0093/0094/0095 precedent:
// the `tests::` infix added by the conventional `mod tests` wrapper
// would break the selector resolution gate.

/// Test [`FeedbackPersistence`] impl — accumulates persisted
/// signals + adjustments via [`std::cell::RefCell`] so the test
/// can assert AFTER calling [`analyze_post_hoc`] +
/// `persistence.persist(...)` round-trip.
///
/// Mirrors `TestBonusContextSource` (WO-0088) /
/// `TestWarmProcessorSource` (WO-0093) shape verbatim.  The
/// `Test` prefix indicates a UCIL-internal trait impl for the
/// dependency-inversion seam — production impls live in
/// `crates/ucil-daemon/`.
#[cfg(test)]
struct TestFeedbackPersistence {
    persisted_signals: std::cell::RefCell<Vec<FeedbackSignalRecord>>,
    applied_adjustments: std::cell::RefCell<Vec<ImportanceAdjustment>>,
}

#[cfg(test)]
impl TestFeedbackPersistence {
    fn new() -> Self {
        Self {
            persisted_signals: std::cell::RefCell::new(Vec::new()),
            applied_adjustments: std::cell::RefCell::new(Vec::new()),
        }
    }
}

#[cfg(test)]
impl FeedbackPersistence for TestFeedbackPersistence {
    fn persist(&self, signals: &[FeedbackSignalRecord]) -> Result<(), FeedbackError> {
        self.persisted_signals
            .borrow_mut()
            .extend_from_slice(signals);
        Ok(())
    }

    fn apply_adjustments(&self, adjustments: &[ImportanceAdjustment]) -> Result<(), FeedbackError> {
        self.applied_adjustments
            .borrow_mut()
            .extend_from_slice(adjustments);
        Ok(())
    }
}

/// Frozen test for [`analyze_post_hoc`] — the load-bearing
/// acceptance signal for `WO-0096` (P3-W11-F12).
///
/// SA tags (mutation-targeted):
///
/// * SA1 — empty `prior_bonuses` ⇒ default outcome (zero
///   signals, zero adjustments).
/// * SA2 — `Pitfall` with matched reason keyword ⇒ `Used` +
///   `+0.1` (M2 target: zeroing the boost magnitude flips this).
/// * SA3 — `Convention` with matched edit `content_after` ⇒
///   `Followed` + `+0.05` (M1 target: zeroing the predicate
///   flips this to `Ignored`).
/// * SA4 — `QualityIssue` overlapping an edit's lines ⇒
///   `Fixed` + `+0.1`.
/// * SA5 — `RelatedCode` whose file appears in
///   `files_in_context` ⇒ `Used` + `+0.05`.
/// * SA6 — `Pitfall` with no matching reason keyword ⇒
///   `Ignored` + `-0.01`.
/// * SA7 — input order preservation across multiple bonuses.
/// * SA8 — caller-provided `now_iso` round-trips verbatim.
#[cfg(test)]
#[allow(clippy::missing_panics_doc)]
#[allow(clippy::too_many_lines)]
#[allow(clippy::items_after_statements)]
#[test]
pub fn test_post_hoc_analyser() {
    use std::path::PathBuf;

    let session_id = "sess-WO-0096";
    let now_iso = "2026-05-09T08:42:51Z";
    let options = FeedbackAnalysisOptions::default();
    let persistence = TestFeedbackPersistence::new();

    // ── SA1 — empty prior_bonuses ⇒ default outcome ──────────────
    let empty_outcome = analyze_post_hoc(
        session_id,
        &[],
        &AgentNextCall::default(),
        now_iso,
        &options,
    );
    assert_eq!(
        empty_outcome,
        FeedbackAnalysisOutcome::default(),
        "(SA1) empty prior_bonuses yields default outcome; left: {observed:?}, right: {expected:?}",
        observed = empty_outcome,
        expected = FeedbackAnalysisOutcome::default(),
    );

    // ── SA2 — Pitfall with matched reason keyword ⇒ Used+0.1 ─────
    let pitfall_matched = BonusReference {
        session_id: session_id.to_owned(),
        bonus_type: BonusType::Pitfall,
        bonus_id: Some(101),
        file_path: None,
        line_range: None,
        keyword: Some("Idempotency_Key".to_owned()),
    };
    let next_call_pitfall = AgentNextCall {
        reason: Some("guard the call with an idempotency_key per the bonus".to_owned()),
        files_in_context: vec![],
        edits: vec![],
    };
    let outcome2 = analyze_post_hoc(
        session_id,
        std::slice::from_ref(&pitfall_matched),
        &next_call_pitfall,
        now_iso,
        &options,
    );
    assert_eq!(
        outcome2.signals.len(),
        1,
        "(SA2) pitfall matched yields one signal; left: {observed}, right: 1",
        observed = outcome2.signals.len(),
    );
    assert_eq!(
        outcome2.signals[0].signal,
        FeedbackSignal::Used,
        "(SA2) pitfall matched yields Used; left: {observed:?}, right: {expected:?}",
        observed = outcome2.signals[0].signal,
        expected = FeedbackSignal::Used,
    );
    assert!(
        (outcome2.adjustments[0].delta - 0.1).abs() < f64::EPSILON,
        "(SA2) pitfall used boost magnitude; left: {observed}, right: 0.1",
        observed = outcome2.adjustments[0].delta,
    );

    // ── SA3 — Convention with matched edit content_after ⇒ Followed+0.05 ─
    let convention_matched = BonusReference {
        session_id: session_id.to_owned(),
        bonus_type: BonusType::Convention,
        bonus_id: Some(202),
        file_path: None,
        line_range: None,
        keyword: Some("thiserror".to_owned()),
    };
    let next_call_convention = AgentNextCall {
        reason: None,
        files_in_context: vec![],
        edits: vec![EditObservation {
            file_path: PathBuf::from("src/error.rs"),
            edited_lines: 1..=20,
            content_after: "use ThisError;\n#[derive(Debug, ThisError)]\nenum ModuleError {}"
                .to_owned(),
        }],
    };
    let outcome3 = analyze_post_hoc(
        session_id,
        std::slice::from_ref(&convention_matched),
        &next_call_convention,
        now_iso,
        &options,
    );
    assert_eq!(
        outcome3.signals[0].signal,
        FeedbackSignal::Followed,
        "(SA3) convention edit yields Followed signal; left: {observed:?}, right: {expected:?}",
        observed = outcome3.signals[0].signal,
        expected = FeedbackSignal::Followed,
    );
    assert!(
        (outcome3.adjustments[0].delta - 0.05).abs() < f64::EPSILON,
        "(SA3) convention followed boost magnitude; left: {observed}, right: 0.05",
        observed = outcome3.adjustments[0].delta,
    );

    // ── SA4 — QualityIssue overlap with edit lines ⇒ Fixed+0.1 ───
    let quality_overlap = BonusReference {
        session_id: session_id.to_owned(),
        bonus_type: BonusType::QualityIssue,
        bonus_id: Some(303),
        file_path: Some(PathBuf::from("src/lib.rs")),
        line_range: Some(40..=44),
        keyword: None,
    };
    let next_call_quality = AgentNextCall {
        reason: None,
        files_in_context: vec![],
        edits: vec![EditObservation {
            file_path: PathBuf::from("src/lib.rs"),
            edited_lines: 42..=43,
            content_after: "let _ = 0;".to_owned(),
        }],
    };
    let outcome4 = analyze_post_hoc(
        session_id,
        std::slice::from_ref(&quality_overlap),
        &next_call_quality,
        now_iso,
        &options,
    );
    assert_eq!(
        outcome4.signals[0].signal,
        FeedbackSignal::Fixed,
        "(SA4) quality_issue overlap yields Fixed signal; left: {observed:?}, right: {expected:?}",
        observed = outcome4.signals[0].signal,
        expected = FeedbackSignal::Fixed,
    );
    assert!(
        (outcome4.adjustments[0].delta - 0.1).abs() < f64::EPSILON,
        "(SA4) quality_issue fixed boost magnitude; left: {observed}, right: 0.1",
        observed = outcome4.adjustments[0].delta,
    );

    // ── SA5 — RelatedCode whose file is in files_in_context ⇒ Used+0.05 ──
    let related_code_matched = BonusReference {
        session_id: session_id.to_owned(),
        bonus_type: BonusType::RelatedCode,
        bonus_id: Some(404),
        file_path: Some(PathBuf::from("src/utils/retry.rs")),
        line_range: None,
        keyword: None,
    };
    let next_call_related = AgentNextCall {
        reason: None,
        files_in_context: vec![PathBuf::from("src/utils/retry.rs")],
        edits: vec![],
    };
    let outcome5 = analyze_post_hoc(
        session_id,
        std::slice::from_ref(&related_code_matched),
        &next_call_related,
        now_iso,
        &options,
    );
    assert_eq!(
        outcome5.signals[0].signal,
        FeedbackSignal::Used,
        "(SA5) related_code in files_in_context yields Used signal; left: {observed:?}, right: {expected:?}",
        observed = outcome5.signals[0].signal,
        expected = FeedbackSignal::Used,
    );
    assert!(
        (outcome5.adjustments[0].delta - 0.05).abs() < f64::EPSILON,
        "(SA5) related_code used boost magnitude; left: {observed}, right: 0.05",
        observed = outcome5.adjustments[0].delta,
    );

    // ── SA6 — Pitfall with no matching reason ⇒ Ignored-0.01 ─────
    let pitfall_unmatched = BonusReference {
        session_id: session_id.to_owned(),
        bonus_type: BonusType::Pitfall,
        bonus_id: Some(606),
        file_path: None,
        line_range: None,
        keyword: Some("idempotency_key".to_owned()),
    };
    let next_call_unmatched = AgentNextCall {
        reason: Some("just refactoring naming".to_owned()),
        files_in_context: vec![],
        edits: vec![],
    };
    let outcome6 = analyze_post_hoc(
        session_id,
        std::slice::from_ref(&pitfall_unmatched),
        &next_call_unmatched,
        now_iso,
        &options,
    );
    assert_eq!(
        outcome6.signals[0].signal,
        FeedbackSignal::Ignored,
        "(SA6) pitfall unmatched yields Ignored signal; left: {observed:?}, right: {expected:?}",
        observed = outcome6.signals[0].signal,
        expected = FeedbackSignal::Ignored,
    );
    assert!(
        (outcome6.adjustments[0].delta - -0.01).abs() < f64::EPSILON,
        "(SA6) ignored decay magnitude; left: {observed}, right: -0.01",
        observed = outcome6.adjustments[0].delta,
    );

    // ── SA7 — input order preservation across multiple bonuses ───
    let order_input = vec![
        pitfall_matched,
        convention_matched,
        BonusReference {
            session_id: session_id.to_owned(),
            bonus_type: BonusType::RelatedCode,
            bonus_id: Some(707),
            file_path: Some(PathBuf::from("src/never/used.rs")),
            line_range: None,
            keyword: None,
        },
    ];
    let combined_call = AgentNextCall {
        reason: Some("guard the call with an idempotency_key per the bonus".to_owned()),
        files_in_context: vec![],
        edits: vec![EditObservation {
            file_path: PathBuf::from("src/error.rs"),
            edited_lines: 1..=20,
            content_after: "use ThisError;\n#[derive(Debug, ThisError)]\nenum ModuleError {}"
                .to_owned(),
        }],
    };
    let outcome7 = analyze_post_hoc(session_id, &order_input, &combined_call, now_iso, &options);
    let observed_signals: Vec<FeedbackSignal> = outcome7.signals.iter().map(|s| s.signal).collect();
    assert_eq!(
        observed_signals,
        vec![
            FeedbackSignal::Used,
            FeedbackSignal::Followed,
            FeedbackSignal::Ignored,
        ],
        "(SA7) input order preserved across mixed bonuses; left: {observed:?}, right: {expected:?}",
        observed = observed_signals,
        expected = vec![
            FeedbackSignal::Used,
            FeedbackSignal::Followed,
            FeedbackSignal::Ignored,
        ],
    );

    // ── SA8 — caller-provided now_iso round-trips verbatim ───────
    for s in &outcome7.signals {
        assert_eq!(
            s.timestamp_iso,
            now_iso,
            "(SA8) timestamp_iso round-trip; left: {observed}, right: {expected}",
            observed = s.timestamp_iso,
            expected = now_iso,
        );
    }

    // Trait round-trip — ceremonial, proves the trait surface
    // compiles + can be implemented in-process.  The load-bearing
    // assertions above exercise `analyze_post_hoc`'s return-value
    // shape directly.
    persistence
        .persist(&outcome7.signals)
        .expect("TestFeedbackPersistence::persist returns Ok");
    persistence
        .apply_adjustments(&outcome7.adjustments)
        .expect("TestFeedbackPersistence::apply_adjustments returns Ok");
    assert_eq!(
        persistence.persisted_signals.borrow().len(),
        outcome7.signals.len(),
        "(SA-trait-roundtrip) persistence captured all signals; left: {observed}, right: {expected}",
        observed = persistence.persisted_signals.borrow().len(),
        expected = outcome7.signals.len(),
    );
    assert_eq!(
        persistence.applied_adjustments.borrow().len(),
        outcome7.adjustments.len(),
        "(SA-trait-roundtrip) persistence captured all adjustments; left: {observed}, right: {expected}",
        observed = persistence.applied_adjustments.borrow().len(),
        expected = outcome7.adjustments.len(),
    );
}
