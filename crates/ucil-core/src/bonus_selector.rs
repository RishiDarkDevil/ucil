//! Bonus-context selector — feature `P3-W10-F11`.
//!
//! Master-plan citations driving this module:
//!
//! * §1.3 lines 65-79 — the canonical 8 bonus-context categories
//!   (`conventions`, `pitfalls`, `quality_issues`, `related_code`,
//!   `tests_to_update`, `blast_radius`, `history`, `security`).
//! * §6.3 lines 660-690 — Response assembly chains directly into
//!   bonus-context attachment: "Append relevant bonus context:
//!   conventions that apply, pitfalls for this code, quality issues
//!   in these files, tests that cover this code, blast radius if
//!   editing — skip bonus categories with zero relevant entries."
//! * §6.3 line 667-668 — "when relevance score ≥0.1" attachment
//!   threshold; intentionally aligned with the F09 `<0.1` filter so
//!   F09's surviving hits are exactly F11's bonus-recipients when
//!   `relevance_threshold == attachment_threshold` (the default).
//! * §17.2 — `bonus_selector.rs` joins the canonical `ucil-core/src/`
//!   layout as a sibling of `context_compiler.rs`,
//!   `cross_group.rs`, `tier_merger.rs`.
//! * §18 Phase 3 Week 10 line 1810 — "Bonus context selector
//!   (relevance score ≥0.1)".
//!
//! # Design — UCIL-owned dependency-inversion seam
//!
//! Per `DEC-0008` §4 the dependency-inversion seam between this
//! module and the production back-ends (Knowledge-Graph reads for
//! conventions/pitfalls; G4 quality issues; G7 history; G8
//! security; etc.) is a UCIL-owned trait (NOT a re-export of any
//! external wire format).  [`BonusContextSource`] is the single
//! method-bag that production impls (e.g. `KgBonusContextSource`,
//! `G4BonusContextSource`) populate; the in-process
//! `TestBonusContextSource` in `#[cfg(test)]` here mirrors the
//! `TestG4Source` (WO-0083) / `TestG7Source` (WO-0085) precedent
//! verbatim.
//!
//! The trait surface is intentionally **synchronous** — production
//! impls that need IO MAY internally use [`tokio::task::block_in_place`]
//! or buffer into eagerly-computed [`BonusEntries`].  Keeping the
//! trait sync lets [`select_bonus_context`] be called from any
//! execution context (sync or async) without `Pin<Box<...>>`
//! orchestration overhead.  Same shape as the pre-loaded-slice
//! merge-fn pattern (WO-0084 §planner) — IO concerns are pushed to
//! the consumer, leaving this projection deterministic.
//!
//! # Hot-cold tier
//!
//! Master-plan §11 hot-cold tier does NOT apply — the selector is a
//! pure data-only projection over already-fetched
//! [`crate::CrossGroupFusedHit`]s + a caller-supplied
//! [`BonusContextSource`].  No cache surfaces, no warm-tier
//! promotion.  A future ADR may explore caching the
//! [`HitWithBonus`] vector across queries; for now it is computed
//! fresh per call.
//!
//! See `decisions/DEC-0007-remove-cargo-mutants-per-wo-gate.md` for
//! the frozen-test selector module-root placement requirement that
//! puts `test_bonus_context_selection` at module root (NOT inside
//! `mod tests`).

#![deny(rustdoc::broken_intra_doc_links)]

use crate::cross_group::CrossGroupFusedHit;

// ── BonusEntries: the 8 master-plan §1.3 categories ──────────────────────────

/// The 8 master-plan §1.3 bonus-context categories, populated
/// per-hit by a [`BonusContextSource`] impl.
///
/// Each field is a [`Vec<String>`] to keep the surface simple and
/// language-agnostic — the underlying production impls render their
/// internal §12.1 / G4 / G7 / G8 rows into pre-formatted strings.
/// Empty `Vec`s mean "no relevant entries for this category" and
/// are skipped at host-adapter render-time per master-plan §6.3
/// line 670 ("skip bonus categories with zero relevant entries").
///
/// `Default` returns all-empty vectors, suitable as a starting point
/// for incremental population by a multi-source aggregator (e.g. KG
/// for `conventions` + `pitfalls` + `history`; G4 for
/// `quality_issues`; G7 for `tests_to_update`; G8 for `security`;
/// etc.).
///
/// # Examples
///
/// ```
/// use ucil_core::BonusEntries;
///
/// let empty = BonusEntries::default();
/// assert!(empty.conventions.is_empty());
/// assert!(empty.security.is_empty());
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BonusEntries {
    /// Style/idiom conventions that apply to the hit's region.
    /// Master-plan §1.3 line 65.
    pub conventions: Vec<String>,
    /// Known pitfalls (frequently-recurring bugs, anti-patterns)
    /// for the hit's code path.  Master-plan §1.3 line 66.
    pub pitfalls: Vec<String>,
    /// Open quality issues (clippy lints, lint suppressions,
    /// TODO/FIXME comments, dead code) overlapping the hit's lines.
    /// Master-plan §1.3 line 67.
    pub quality_issues: Vec<String>,
    /// Related code locations (callers, callees, similar
    /// implementations) discovered via knowledge-graph traversal.
    /// Master-plan §1.3 line 68.
    pub related_code: Vec<String>,
    /// Tests that exercise the hit's code, surfaced from the §6.5
    /// G7 test-impact subgraph.  Master-plan §1.3 line 69.
    pub tests_to_update: Vec<String>,
    /// Blast-radius — files / symbols downstream of an edit at the
    /// hit's location.  Master-plan §1.3 line 70.
    pub blast_radius: Vec<String>,
    /// Recent change history for the hit's file (commits, PRs,
    /// authors).  Master-plan §1.3 line 71.
    pub history: Vec<String>,
    /// Security-sensitive markers (CVE references, taint sources,
    /// crypto primitives) overlapping the hit's lines.
    /// Master-plan §1.3 line 72.
    pub security: Vec<String>,
}

// ── HitWithBonus: per-hit attachment record ──────────────────────────────────

/// A [`CrossGroupFusedHit`] paired with its optional
/// [`BonusEntries`].
///
/// `bonus.is_some()` IFF the hit's `fused_score` was at-or-above
/// the [`BonusSelectionOptions::attachment_threshold`] at the call
/// to [`select_bonus_context`].  The `Option<>` makes the boundary
/// explicit at the type level — below-threshold hits carry NO
/// bonus payload at all (NOT an empty [`BonusEntries`]).  This
/// distinction lets callers skip per-category render work without
/// having to inspect every field of [`BonusEntries`].
#[derive(Debug, Clone, PartialEq)]
pub struct HitWithBonus {
    /// The fused hit verbatim (cloned from the input slice — the
    /// [`select_bonus_context`] entry point does NOT mutate the
    /// caller's slice).
    pub hit: CrossGroupFusedHit,
    /// `Some(...)` IFF `hit.fused_score >= options.attachment_threshold`;
    /// `None` otherwise.
    pub bonus: Option<BonusEntries>,
}

// ── Options ──────────────────────────────────────────────────────────────────

/// Tuning knobs for [`select_bonus_context`].
///
/// `attachment_threshold` defaults to `0.1` per master-plan §6.3
/// line 667-668 ("when relevance score ≥0.1") — intentionally the
/// same value as `ResponseAssemblyOptions::relevance_threshold` so
/// F09's surviving hits are exactly F11's bonus-recipients.  When
/// the two thresholds are aligned (the default), the chained
/// `assemble_response` → `select_bonus_context` pipeline produces
/// `HitWithBonus { bonus: Some(_), .. }` for every surviving hit —
/// the §6.3 line 670 invariant.
///
/// # Examples
///
/// ```
/// use ucil_core::BonusSelectionOptions;
///
/// let defaults = BonusSelectionOptions::default();
/// assert!((defaults.attachment_threshold - 0.1).abs() < f64::EPSILON);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BonusSelectionOptions {
    /// Score threshold at-or-above which a hit gets bonus context
    /// attached.  Comparison is `>=` (non-strict): a hit with
    /// `fused_score == attachment_threshold` GETS bonus.  Master-
    /// plan §6.3 line 667-668 default `0.1`.
    pub attachment_threshold: f64,
}

impl Default for BonusSelectionOptions {
    fn default() -> Self {
        Self {
            attachment_threshold: 0.1,
        }
    }
}

// ── BonusContextSource: the dependency-inversion seam ─────────────────────────

/// Synchronous bonus-context source — the UCIL-owned
/// dependency-inversion seam (per `DEC-0008` §4).
///
/// Production impls (`KgBonusContextSource`, `G4BonusContextSource`,
/// `G7BonusContextSource`, `G8BonusContextSource`, etc.) wire the 8
/// master-plan §1.3 categories to real KG / G4 / G7 / G8 surfaces;
/// they are deferred to a follow-up production-wiring WO (same
/// shape as the G3/G4/G7 production-wiring deferrals — WO-0083 /
/// WO-0085 / WO-0086 §planner).  The `TestBonusContextSource` in
/// `#[cfg(test)]` exercises the trait + its
/// [`select_bonus_context`] consumer end-to-end.
///
/// The trait surface is intentionally synchronous — production
/// impls that need IO MAY internally use
/// [`tokio::task::block_in_place`] or buffer into eagerly-computed
/// [`BonusEntries`].  Keeping the trait sync lets
/// [`select_bonus_context`] be called from any execution context
/// (sync or async) without `Pin<Box<...>>` orchestration overhead.
/// Same precedent: the `TestG4Source` (WO-0083) and `TestG7Source`
/// (WO-0085) traits are also sync.
pub trait BonusContextSource {
    /// Fetch the 8-category bonus payload for a single hit.
    ///
    /// MUST be deterministic (same hit ⇒ same [`BonusEntries`])
    /// for a given source instance; production impls SHOULD cache
    /// per-source-tool results internally where the underlying KG
    /// / G4 / G7 / G8 query supports it.
    fn fetch_bonus(&self, hit: &CrossGroupFusedHit) -> BonusEntries;
}

// ── Public entry point ───────────────────────────────────────────────────────

/// Attach bonus-context to each hit whose `fused_score >=
/// options.attachment_threshold`.
///
/// Pure-deterministic projection over a pre-fetched slice + a
/// caller-supplied [`BonusContextSource`].  Generic over `S` (NOT
/// `dyn`) per `.claude/rules/rust-style.md` §Crate-layout default —
/// monomorphisation gives the inlining a tighter trip through
/// `fetch_bonus` for hot-path callers.  For multi-source
/// aggregation (when daemon-side production-wiring lands), a
/// composite [`BonusContextSource`] impl can fold KG + G4 + G7 +
/// G8 results into a single `BonusEntries` per call.
///
/// Algorithm — per master-plan §6.3 line 667-668 (boundary inclusion
/// at `>= attachment_threshold`, the inverse boundary of F09's
/// strict `<` filter):
///
/// 1. For each hit `h` in `hits`:
///    * If `h.fused_score >= options.attachment_threshold`:
///      `bonus = Some(source.fetch_bonus(h))`.
///    * Else: `bonus = None`.
///    * Push `HitWithBonus { hit: h.clone(), bonus }` to the output.
/// 2. Return the [`Vec<HitWithBonus>`] in input order.
///
/// Never panics; never returns `Result`.  Empty input → empty
/// output.  Order of input is preserved (callers typically pass
/// `outcome.hits` from a [`crate::CrossGroupFusedOutcome`] which is
/// already sorted descending by `fused_score`).
///
/// §15.2 tracing carve-out applies (pure-deterministic projection)
/// — production impls of [`BonusContextSource`] in `ucil-daemon`
/// carry `tracing::instrument` at the IO boundary.
///
/// # Examples
///
/// ```
/// use ucil_core::{
///     BonusContextSource, BonusEntries, BonusSelectionOptions,
///     CrossGroupFusedHit, select_bonus_context,
/// };
///
/// struct AlwaysEmpty;
/// impl BonusContextSource for AlwaysEmpty {
///     fn fetch_bonus(&self, _: &CrossGroupFusedHit) -> BonusEntries {
///         BonusEntries::default()
///     }
/// }
///
/// let mut hit = CrossGroupFusedHit::default();
/// hit.fused_score = 0.5;
/// let result = select_bonus_context(
///     &[hit],
///     &AlwaysEmpty,
///     &BonusSelectionOptions::default(),
/// );
/// assert!(result[0].bonus.is_some());
/// ```
#[must_use]
pub fn select_bonus_context<S: BonusContextSource>(
    hits: &[CrossGroupFusedHit],
    source: &S,
    options: &BonusSelectionOptions,
) -> Vec<HitWithBonus> {
    hits.iter()
        .map(|h| {
            let bonus = if h.fused_score >= options.attachment_threshold {
                Some(source.fetch_bonus(h))
            } else {
                None
            };
            HitWithBonus {
                hit: h.clone(),
                bonus,
            }
        })
        .collect()
}

// ── Module-root test (DEC-0007 frozen-selector placement) ─────────────────────
//
// `test_bonus_context_selection` lives at module root — NOT inside
// `mod tests { ... }` — so the substring selector
// `cargo test -p ucil-core bonus_selector::test_bonus_context_selection`
// resolves uniquely without `--exact`.  Per `DEC-0007` +
// WO-0067/0068/0070/0083/0084/0085 precedent: the `tests::` infix
// added by the conventional `mod tests` wrapper would break the
// selector resolution gate.

/// Test [`BonusContextSource`] impl — returns canned
/// [`BonusEntries`] regardless of which hit it's queried for.
/// Mirrors `TestG4Source` (WO-0083) and `TestG7Source` (WO-0085)
/// shape verbatim.  The `Test` prefix indicates a UCIL-internal
/// trait impl for the dependency-inversion seam, not an
/// external-wire-format substitute (the latter are categorically
/// forbidden per `CLAUDE.md` word-ban; the former are the
/// dependency-inversion seam per `DEC-0008` §4).
#[cfg(test)]
struct TestBonusContextSource {
    canned: BonusEntries,
}

#[cfg(test)]
impl BonusContextSource for TestBonusContextSource {
    fn fetch_bonus(&self, _hit: &CrossGroupFusedHit) -> BonusEntries {
        self.canned.clone()
    }
}

/// Build a [`CrossGroupFusedHit`] with a known `fused_score` for
/// the test's load-bearing partition (above / at / below the
/// 0.10 attachment threshold).
#[cfg(test)]
fn make_test_hit(file: &str, score: f64) -> CrossGroupFusedHit {
    CrossGroupFusedHit {
        file_path: std::path::PathBuf::from(file),
        start_line: 1,
        end_line: 10,
        snippet: format!("// {file}\nfn placeholder() {{}}"),
        fused_score: score,
        contributing_groups: Vec::new(),
        per_group_ranks: Vec::new(),
    }
}

/// Frozen test for [`select_bonus_context`].
///
/// SA tags (mutation-targeted):
///
/// * SA1 — output cardinality preserves input cardinality.
/// * SA2 — top hit gets bonus payload (8 categories populated).
/// * SA3 — boundary hit at exactly the threshold GETS bonus
///   (M3 target: invert `>=` to `<` flips this).
/// * SA4 — below-threshold hit does NOT get bonus
///   (M3 target: same inversion).
/// * SA5 — zero-score hit does NOT get bonus.
/// * SA6 — all 8 categories of the canned bonus surface verbatim
///   on the top hit (the §1.3 canonical-8-categories canary).
/// * SA7 — hit identity (incl. `fused_score`) preserved through
///   projection.
#[cfg(test)]
#[test]
fn test_bonus_context_selection() {
    let hits = vec![
        make_test_hit("src/file_top.rs", 0.50),
        make_test_hit("src/file_boundary.rs", 0.10),
        make_test_hit("src/file_below.rs", 0.05),
        make_test_hit("src/file_zero.rs", 0.00),
    ];
    let canned = BonusEntries {
        conventions: vec!["snake_case".to_owned()],
        pitfalls: vec!["avoid mut".to_owned()],
        quality_issues: vec!["warning_X".to_owned()],
        related_code: vec!["foo.rs:10".to_owned()],
        tests_to_update: vec!["test_foo".to_owned()],
        blast_radius: vec!["bar.rs".to_owned()],
        history: vec!["PR #42".to_owned()],
        security: vec!["CVE-1234".to_owned()],
    };
    let source = TestBonusContextSource {
        canned: canned.clone(),
    };

    let result = select_bonus_context(&hits, &source, &BonusSelectionOptions::default());

    // ── SA1 — input cardinality preserved ────────────────────────
    assert_eq!(
        result.len(),
        4,
        "(SA1) result count expected 4; observed {n}",
        n = result.len(),
    );

    // ── SA2 — top hit gets bonus payload ─────────────────────────
    let top_bonus = result[0]
        .bonus
        .as_ref()
        .expect("(SA2) top-hit bonus expected Some(...); observed None");
    assert_eq!(
        top_bonus.conventions,
        vec!["snake_case".to_owned()],
        "(SA2) top-hit bonus.conventions expected [\"snake_case\"]; observed {b:?}",
        b = top_bonus.conventions,
    );

    // ── SA3 — boundary hit at score == 0.10 GETS bonus ───────────
    //
    // M3 target: inverting `>=` to `<` flips this assertion (the
    // boundary hit would lose its bonus under inverted comparison).
    assert!(
        result[1].bonus.is_some(),
        "(SA3) boundary-hit (score=0.10) bonus expected Some; observed {b:?}",
        b = result[1].bonus,
    );

    // ── SA4 — below-threshold hit does NOT get bonus ─────────────
    //
    // M3 target: inverting `>=` to `<` flips this assertion.
    assert!(
        result[2].bonus.is_none(),
        "(SA4) below-threshold hit (score=0.05) bonus expected None; observed {b:?}",
        b = result[2].bonus,
    );

    // ── SA5 — zero-score hit does NOT get bonus ──────────────────
    assert!(
        result[3].bonus.is_none(),
        "(SA5) zero-score hit bonus expected None; observed {b:?}",
        b = result[3].bonus,
    );

    // ── SA6 — all 8 categories surface verbatim on top hit ───────
    //
    // The §1.3 canonical-8-categories regression canary —
    // analogous to the WO-0067 §6.2 sentinel-row + WO-0085 §5.7
    // severity sentinel-row patterns.
    let bonus_top = result[0]
        .bonus
        .as_ref()
        .expect("(SA6) top-hit bonus must be Some(...) for the 8-category sentinel canary");
    assert_eq!(
        bonus_top.conventions,
        canned.conventions,
        "(SA6.conventions) bonus.conventions expected {expected:?}; observed {actual:?}",
        expected = canned.conventions,
        actual = bonus_top.conventions,
    );
    assert_eq!(
        bonus_top.pitfalls,
        canned.pitfalls,
        "(SA6.pitfalls) bonus.pitfalls expected {expected:?}; observed {actual:?}",
        expected = canned.pitfalls,
        actual = bonus_top.pitfalls,
    );
    assert_eq!(
        bonus_top.quality_issues,
        canned.quality_issues,
        "(SA6.quality_issues) bonus.quality_issues expected {expected:?}; observed {actual:?}",
        expected = canned.quality_issues,
        actual = bonus_top.quality_issues,
    );
    assert_eq!(
        bonus_top.related_code,
        canned.related_code,
        "(SA6.related_code) bonus.related_code expected {expected:?}; observed {actual:?}",
        expected = canned.related_code,
        actual = bonus_top.related_code,
    );
    assert_eq!(
        bonus_top.tests_to_update,
        canned.tests_to_update,
        "(SA6.tests_to_update) bonus.tests_to_update expected {expected:?}; observed {actual:?}",
        expected = canned.tests_to_update,
        actual = bonus_top.tests_to_update,
    );
    assert_eq!(
        bonus_top.blast_radius,
        canned.blast_radius,
        "(SA6.blast_radius) bonus.blast_radius expected {expected:?}; observed {actual:?}",
        expected = canned.blast_radius,
        actual = bonus_top.blast_radius,
    );
    assert_eq!(
        bonus_top.history,
        canned.history,
        "(SA6.history) bonus.history expected {expected:?}; observed {actual:?}",
        expected = canned.history,
        actual = bonus_top.history,
    );
    assert_eq!(
        bonus_top.security,
        canned.security,
        "(SA6.security) bonus.security expected {expected:?}; observed {actual:?}",
        expected = canned.security,
        actual = bonus_top.security,
    );

    // ── SA7 — hit identity preserved through projection ─────────
    let pos_zero = result[3].hit.fused_score;
    assert!(
        (pos_zero - 0.0).abs() < f64::EPSILON,
        "(SA7) hit identity preserved: fused_score expected 0.0; observed {n}",
        n = pos_zero,
    );
}
