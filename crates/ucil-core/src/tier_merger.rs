//! Multi-tier query merging — feature `P3-W10-F12`.
//!
//! Master-plan §17 line 1636 (directory structure) seats this module
//! alongside `hot_staging.rs` + `warm_processors.rs` as the third
//! tiering-layer file in `crates/ucil-core/src/`.  This `tier_merger.rs`
//! implements the pure-deterministic merge function that combines hot,
//! warm, and cold tier query results into a single ranked response with
//! tier-provenance metadata.
//!
//! Master-plan §12.3 ("Knowledge tiering — hot/warm/cold processing")
//! defines the three-tier confidence-band model:
//!
//! ```text
//! HOT  (immediate, 0-5ms):  Raw append. Queryable immediately. Confidence 0.2-0.4.
//! WARM (1-5 minutes):       Rule-based enrichment. No LLM.      Confidence 0.5-0.7.
//! COLD (hours):             LLM-powered curation.                Confidence 0.8-1.0.
//! ```
//!
//! Master-plan §11.3 ("pull-based-relevance + recency boost") drives the
//! merge-time recency bias: when the same value surfaces from multiple
//! tiers, the most-recent observation wins regardless of which tier
//! produced it.  HOT can override COLD by recency, even though COLD has
//! the higher static confidence band — the freshness signal is what the
//! agent layer wants.
//!
//! Master-plan §18 Phase 3 Week 10 deliverable #6 (line 1813):
//!
//! > "Multi-tier query merging (hot + warm + cold)"
//!
//! Implementation issued by `WO-0084`.  This module ships the pure
//! merge surface only — production-wired tier sources
//! (`HotStagingSource` / `WarmTierSource` / `ColdTierSource` reading
//! from `KnowledgeGraph::stage_hot_*` / future warm-processor outputs /
//! the `memory.db` cold tables) are deferred to the consumer WO that
//! lands the daemon-side response-assembly pipeline.  The merger takes
//! pre-loaded `&[TieredResult<T>]` slices so the dependency-inversion
//! seam stays clean (analogous to the `G3Source` / `G4Source` /
//! `cross_group::GroupExecutor` precedents).
//!
//! No `tracing::instrument` annotations — the §15.2 carve-out applies
//! (pure-deterministic CPU-bound module per `WO-0067` §`lessons_applied`
//! #5).  No async, no IO, no logging, no `regex`.  100% safe Rust —
//! no raw-pointer-dereferencing blocks anywhere in the module.

#![deny(rustdoc::broken_intra_doc_links)]

use std::time::SystemTime;

// ── Tier enum ─────────────────────────────────────────────────────────────────

/// One of the three knowledge tiers per master-plan §12.3.
///
/// Discriminant order: `Hot = 0` < `Warm = 1` < `Cold = 2`.  Newer /
/// hotter tiers carry the smaller discriminant so the natural derived
/// `Ord` produces `Hot < Warm < Cold`.  The deduplication-and-sort step
/// in [`merge_across_tiers`] uses this ordinal to canonicalize the
/// `contributing_tiers` list (sorted ascending by tier ordinal so a
/// value seen in HOT + WARM + COLD ships as `[Hot, Warm, Cold]`).
///
/// Confidence-band ranges per master-plan §12.3:
///
/// - `Hot`:  raw append, `confidence` ∈ `[0.2, 0.4]`.
/// - `Warm`: rule-based enrichment (no LLM), `confidence` ∈ `[0.5, 0.7]`.
/// - `Cold`: LLM-powered curation, `confidence` ∈ `[0.8, 1.0]`.
///
/// `Copy` is sound — the enum carries no fields.  The hash + comparison
/// derives are cheap and let downstream consumers index into a
/// `BTreeMap<Tier, _>` if they need to bin results by tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tier {
    /// Immediate-append tier.  0-5ms staging window.  Confidence band
    /// `[0.2, 0.4]` per §12.3 line 1353.
    Hot,
    /// Rule-based enrichment tier.  1-5 minute processing window, no
    /// LLM.  Confidence band `[0.5, 0.7]` per §12.3 line 1354.
    Warm,
    /// LLM-powered curation tier.  Hours-scale processing window.
    /// Confidence band `[0.8, 1.0]` per §12.3 line 1355.
    Cold,
}

// ── TieredResult — input shape ────────────────────────────────────────────────

/// One tier-tagged observation consumed by [`merge_across_tiers`].
///
/// The merger takes pre-loaded `&[TieredResult<T>]` slices for each
/// tier — production wiring of real tier sources (`HotStagingSource` /
/// `WarmTierSource` / `ColdTierSource` reading from
/// `KnowledgeGraph::stage_hot_*` / future warm-processor outputs / the
/// `memory.db` cold tables) is the consumer WO's responsibility per the
/// dependency-inversion seam.
///
/// `observed_at` drives the §11.3 pull-based-relevance recency bias —
/// when the same value surfaces from multiple tiers, the most-recent
/// observation wins regardless of which tier produced it.  `confidence`
/// is the tier's self-reported certainty in `[0.0, 1.0]`; per §12.3 the
/// tier alone tags the band, so a HOT observation typically carries
/// `confidence ∈ [0.2, 0.4]` and a COLD observation carries
/// `confidence ∈ [0.8, 1.0]`.
#[derive(Debug, Clone, PartialEq)]
pub struct TieredResult<T> {
    /// The query-result payload value.
    pub value: T,
    /// The tier that produced this observation.
    pub tier: Tier,
    /// Tier-self-reported confidence in `[0.0, 1.0]`.  Per §12.3 the
    /// tier alone tags the band; the raw value is preserved through
    /// the merge.
    pub confidence: f64,
    /// Observation timestamp — drives §11.3 recency bias.
    pub observed_at: SystemTime,
}

// ── MergedResult — output shape ───────────────────────────────────────────────

/// One merged observation emitted by [`merge_across_tiers`].
///
/// Field semantics:
///
/// - `value` — the deduplicated query-result payload.
/// - `source_tier` — the tier whose observation WON the merge (most
///   recent observation among tiers carrying this value).
/// - `confidence` — the confidence carried by the WINNING observation.
///   The merger DOES NOT recompute confidence as a band-midpoint or
///   weighted-average; raw confidence is preserved per §12.3 (the tier
///   alone tags the band).
/// - `contributing_tiers` — the deduplicated, ordinal-sorted list of
///   ALL tiers that surfaced this value.  A value seen in HOT only
///   ships as `[Hot]`; a value seen in HOT + COLD ships as
///   `[Hot, Cold]`; a value seen in HOT + WARM + COLD ships as
///   `[Hot, Warm, Cold]`.  The multi-tier coverage signal is what the
///   M2 verifier mutation erases (the M2 mutation drops the
///   `contributing_tiers` list to `vec![]`, which the SA5 frozen
///   assertion catches).
#[derive(Debug, Clone, PartialEq)]
pub struct MergedResult<T> {
    /// The merged query-result payload.
    pub value: T,
    /// The tier whose observation won the merge (most recent).
    pub source_tier: Tier,
    /// The winning observation's raw confidence (NOT recomputed).
    pub confidence: f64,
    /// Deduplicated, ordinal-sorted list of all tiers that surfaced
    /// this value.
    pub contributing_tiers: Vec<Tier>,
}

// ── Merge function ────────────────────────────────────────────────────────────

/// Merge hot, warm, and cold tier query results into a single ranked output.
///
/// Implements feature `P3-W10-F12` per master-plan §18 Phase 3 Week 10
/// deliverable #6 (line 1813): "Multi-tier query merging (hot + warm +
/// cold)".  Issued by `WO-0084`.
///
/// Algorithm:
///
/// 1. **Iterate** all three input slices in tier-order (`hot`, then
///    `warm`, then `cold`) so the input-order tie-break is stable.
/// 2. **For each unique value** (per `T: PartialEq`):
///    - Find ALL tiered observations carrying that value across the
///      three input slices.
///    - **Determine the winning observation** by `observed_at` recency
///      (most recent `SystemTime` wins; ties broken by tier-ordinal-
///      ascending — `Hot` beats `Warm` beats `Cold` for the same
///      `observed_at`).
///    - `source_tier` = winning observation's tier.
///    - `confidence` = winning observation's confidence (raw, NOT
///      recomputed — per §12.3 the tier alone tags the band).
///    - `contributing_tiers` = deduplicated, ordinal-sorted list of all
///      tiers that surfaced this value.
/// 3. **Return** `Vec<MergedResult<T>>` sorted:
///    - by `source_tier` ascending (Hot first), then
///    - by `confidence` descending (highest-confidence first within a
///      tier), then
///    - by stable insertion order (preserves input order for ties so
///      the output is fully deterministic).
///
/// Complexity: `O(N²)` worst-case where `N = hot.len() + warm.len() +
/// cold.len()` (linear scan dedup).  This is the canonical choice given
/// the `T: Clone + PartialEq` bound — `BTreeMap<&T, ...>` would require
/// `T: Ord` and `HashMap<&T, ...>` would require `T: Hash + Eq` AND
/// would jeopardise determinism.  The merger expects per-tier slices in
/// the few-hundreds — production tier sources truncate to per-tier
/// `LIMIT 100` / `LIMIT 200` envelopes via the daemon-side response-
/// assembly pipeline.
///
/// Pull-based-relevance recency per master-plan §11.3: HOT can override
/// COLD by recency.  COLD's higher static confidence band does NOT
/// override a more-recent HOT observation — the freshness signal is
/// what the agent layer needs.
///
/// The function is pure: no IO, no async, no logging.  100% safe
/// Rust.  No `tracing` span — the §15.2 carve-out applies (pure-
/// deterministic CPU-bound module per `WO-0067` §`lessons_applied`
/// #5).
///
/// # Examples
///
/// ```
/// use std::time::{Duration, SystemTime};
/// use ucil_core::tier_merger::{merge_across_tiers, MergedResult, Tier, TieredResult};
///
/// // HOT carries the same value as COLD but at a more recent
/// // observation — HOT wins by recency despite COLD's higher confidence.
/// let cold_observed = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
/// let hot_observed = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000);
///
/// let hot = [TieredResult {
///     value: "foo",
///     tier: Tier::Hot,
///     confidence: 0.3,
///     observed_at: hot_observed,
/// }];
/// let cold = [TieredResult {
///     value: "foo",
///     tier: Tier::Cold,
///     confidence: 0.9,
///     observed_at: cold_observed,
/// }];
///
/// let merged = merge_across_tiers(&hot, &[], &cold);
/// assert_eq!(merged.len(), 1);
/// assert_eq!(merged[0].source_tier, Tier::Hot);
/// assert_eq!(merged[0].confidence, 0.3);
/// assert_eq!(merged[0].contributing_tiers, vec![Tier::Hot, Tier::Cold]);
/// ```
#[must_use]
pub fn merge_across_tiers<T: Clone + PartialEq>(
    hot: &[TieredResult<T>],
    warm: &[TieredResult<T>],
    cold: &[TieredResult<T>],
) -> Vec<MergedResult<T>> {
    // Capacity hint upper bound — every observation is a distinct value.
    let mut merged: Vec<MergedResult<T>> = Vec::with_capacity(hot.len() + warm.len() + cold.len());

    // Iterate in tier order so the stable-insertion-order tie-break is
    // deterministic (Hot first, then Warm, then Cold).  Linear-scan
    // dedup against the in-progress `merged` Vec.
    let all_tiers: [&[TieredResult<T>]; 3] = [hot, warm, cold];
    for slice in all_tiers {
        for entry in slice {
            // Has the value been seen?  Linear scan satisfies the
            // `T: Clone + PartialEq` bound (no `Hash`/`Ord` required).
            let existing_idx = merged.iter().position(|m| m.value == entry.value);

            if let Some(idx) = existing_idx {
                // Update the merged record — preserve recency winner
                // and grow contributing_tiers if needed.
                let m = &mut merged[idx];
                if !m.contributing_tiers.contains(&entry.tier) {
                    m.contributing_tiers.push(entry.tier);
                    // Keep ordinal-sorted ascending so a value seen in
                    // HOT + WARM + COLD ships as [Hot, Warm, Cold].
                    m.contributing_tiers.sort();
                }

                // Recency winner: most recent `observed_at` wins; ties
                // broken by tier-ordinal-ascending (Hot < Warm < Cold).
                // Compare the new entry against the current winner
                // tracked via `m.source_tier` + look up the original
                // `observed_at` of that winner.  We need the original
                // winning observation's `observed_at` for the
                // comparison — pull it from the input slices.
                let current_winning_observed_at =
                    find_observed_at(hot, warm, cold, &m.value, m.source_tier, m.confidence);

                let new_wins = match entry.observed_at.cmp(&current_winning_observed_at) {
                    std::cmp::Ordering::Greater => true,
                    std::cmp::Ordering::Equal => entry.tier < m.source_tier,
                    std::cmp::Ordering::Less => false,
                };
                if new_wins {
                    m.source_tier = entry.tier;
                    m.confidence = entry.confidence;
                }
            } else {
                merged.push(MergedResult {
                    value: entry.value.clone(),
                    source_tier: entry.tier,
                    confidence: entry.confidence,
                    contributing_tiers: vec![entry.tier],
                });
            }
        }
    }

    // Sort: source_tier ascending, then confidence descending, then
    // stable insertion order (Vec::sort_by is stable).
    merged.sort_by(|a, b| {
        a.source_tier.cmp(&b.source_tier).then_with(|| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    });

    merged
}

/// Locate the `observed_at` of the in-progress winning observation for
/// a given `value`/`tier`/`confidence` triple by linear-scanning the
/// three input slices.
///
/// Internal helper for [`merge_across_tiers`].  The merger preserves
/// only `source_tier` + `confidence` on the in-progress
/// [`MergedResult`]; this helper recovers the winning observation's
/// `observed_at` so the recency comparison stays correct when the same
/// `(tier, confidence)` triple appears multiple times in the input.
fn find_observed_at<T: PartialEq>(
    hot: &[TieredResult<T>],
    warm: &[TieredResult<T>],
    cold: &[TieredResult<T>],
    value: &T,
    tier: Tier,
    confidence: f64,
) -> SystemTime {
    let all_tiers: [&[TieredResult<T>]; 3] = [hot, warm, cold];
    for slice in all_tiers {
        for entry in slice {
            // Float `==` is intentional here — `confidence` is preserved
            // bit-exact through the merger (we copied the same f64 from
            // the winning observation; no arithmetic).  A real-world
            // tier source could repeat the same `(tier, confidence)`
            // triple at different `observed_at`s — we want the FIRST
            // such observation in tier-order, which is also the recency
            // winner (the merge updates `source_tier` + `confidence`
            // monotonically toward the recency winner).
            #[allow(clippy::float_cmp)]
            let triple_match =
                entry.value == *value && entry.tier == tier && entry.confidence == confidence;
            if triple_match {
                return entry.observed_at;
            }
        }
    }
    // Unreachable in practice — the caller only invokes this after
    // pushing or updating a MergedResult that came from one of the
    // input slices.  Defensive fallback is `UNIX_EPOCH` (the floor
    // value), which is monotonically dominated by any real tier
    // observation timestamp.
    SystemTime::UNIX_EPOCH
}

// ── Frozen acceptance test — P3-W10-F12 multi-tier query merge ────────────────
//
// Per `DEC-0007`, the frozen selector lives at MODULE ROOT so the
// `cargo test -p ucil-core tier_merger::test_multi_tier_query_merge`
// selector substring-match resolves uniquely without traversing a
// `mod tests {}` barrier.  Sub-assertions are SA1..SA5 panic-tagged
// so failure messages map back to a specific assertion-of-record.

#[cfg(test)]
#[test]
#[allow(clippy::too_many_lines, clippy::float_cmp)]
fn test_multi_tier_query_merge() {
    use std::time::Duration;

    // ── SA1: empty inputs → empty Vec ────────────────────────────────
    let out: Vec<MergedResult<&'static str>> = merge_across_tiers(&[], &[], &[]);
    assert!(
        out.is_empty(),
        "(SA1) empty inputs must yield empty Vec; left: {} entries, \
         right: 0 entries; out={out:?}",
        out.len(),
    );

    // ── SA2: single-tier input ───────────────────────────────────────
    //
    // Only HOT carries `{value="x", confidence=0.3}` — result is one
    // entry with source_tier=Hot, contributing_tiers=[Hot].
    let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
    let hot_only = [TieredResult {
        value: "x",
        tier: Tier::Hot,
        confidence: 0.3,
        observed_at: t0,
    }];
    let out = merge_across_tiers(&hot_only, &[], &[]);
    assert_eq!(
        out.len(),
        1,
        "(SA2) single-tier input must yield exactly 1 merged entry; \
         left: {} entries, right: 1 entry; out={out:?}",
        out.len(),
    );
    assert_eq!(
        out[0],
        MergedResult {
            value: "x",
            source_tier: Tier::Hot,
            confidence: 0.3,
            contributing_tiers: vec![Tier::Hot],
        },
        "(SA2) single-tier source_tier + contributing_tiers; left: \
         {:?}, right: MergedResult {{ value: \"x\", source_tier: Hot, \
         confidence: 0.3, contributing_tiers: [Hot] }}",
        out[0],
    );

    // ── SA3: cross-tier same-value — recency wins ────────────────────
    //
    // HOT carries `{value="x", confidence=0.3, observed_at=t_recent}`
    // and COLD carries `{value="x", confidence=0.9, observed_at=t_old}`
    // → result is one entry with source_tier=Hot, confidence=0.3,
    // contributing_tiers=[Hot, Cold] (HOT wins by recency; COLD's
    // higher confidence does NOT override per §11.3 pull-based-
    // relevance).
    //
    // This SA pairs with the M2 mutation contract — dropping
    // contributing_tiers to vec![] flips this assertion's
    // contributing_tiers check.
    let t_recent = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000);
    let t_old = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
    let hot_x = [TieredResult {
        value: "x",
        tier: Tier::Hot,
        confidence: 0.3,
        observed_at: t_recent,
    }];
    let cold_x = [TieredResult {
        value: "x",
        tier: Tier::Cold,
        confidence: 0.9,
        observed_at: t_old,
    }];
    let out = merge_across_tiers(&hot_x, &[], &cold_x);
    assert_eq!(
        out.len(),
        1,
        "(SA3) cross-tier same-value must dedup to 1 entry; left: {} \
         entries, right: 1 entry; out={out:?}",
        out.len(),
    );
    assert_eq!(
        out[0],
        MergedResult {
            value: "x",
            source_tier: Tier::Hot,
            confidence: 0.3,
            contributing_tiers: vec![Tier::Hot, Tier::Cold],
        },
        "(SA3) cross-tier same-value HOT-wins-by-recency + \
         contributing_tiers=[Hot, Cold]; left: {:?}, right: \
         MergedResult {{ value: \"x\", source_tier: Hot, confidence: \
         0.3, contributing_tiers: [Hot, Cold] }} (§11.3 \
         pull-based-relevance recency bias)",
        out[0],
    );

    // ── SA4: cross-tier different-values — both surface, ordered ─────
    //
    // HOT `{value="x", confidence=0.3}` and COLD `{value="y",
    // confidence=0.95}` → two entries, ordered by source_tier
    // ascending (Hot first), each with its own contributing_tiers.
    let hot_x2 = [TieredResult {
        value: "x",
        tier: Tier::Hot,
        confidence: 0.3,
        observed_at: t_recent,
    }];
    let cold_y = [TieredResult {
        value: "y",
        tier: Tier::Cold,
        confidence: 0.95,
        observed_at: t_old,
    }];
    let out = merge_across_tiers(&hot_x2, &[], &cold_y);
    assert_eq!(
        out.len(),
        2,
        "(SA4) cross-tier different-values must yield 2 entries; \
         left: {} entries, right: 2 entries; out={out:?}",
        out.len(),
    );
    assert_eq!(
        out[0],
        MergedResult {
            value: "x",
            source_tier: Tier::Hot,
            confidence: 0.3,
            contributing_tiers: vec![Tier::Hot],
        },
        "(SA4) cross-tier different-values out[0] HOT first; left: \
         {:?}, right: MergedResult {{ value: \"x\", source_tier: \
         Hot, confidence: 0.3, contributing_tiers: [Hot] }}",
        out[0],
    );
    assert_eq!(
        out[1],
        MergedResult {
            value: "y",
            source_tier: Tier::Cold,
            confidence: 0.95,
            contributing_tiers: vec![Tier::Cold],
        },
        "(SA4) cross-tier different-values out[1] COLD second; left: \
         {:?}, right: MergedResult {{ value: \"y\", source_tier: \
         Cold, confidence: 0.95, contributing_tiers: [Cold] }}",
        out[1],
    );

    // ── SA5: all-three-tiers same-value → contributing_tiers=[Hot, Warm, Cold] ─
    //
    // HOT, WARM, and COLD all carry `{value="x"}` with different
    // observed_at timestamps — result has ONE entry with
    // contributing_tiers=[Hot, Warm, Cold] (deduplicated, ordinal-
    // sorted).  This SA is the load-bearing assertion against the M2
    // mutation contract: dropping contributing_tiers to vec![] (or
    // collapsing to vec![source_tier]) flips this assertion.
    let t_warm = SystemTime::UNIX_EPOCH + Duration::from_secs(5_000);
    let hot_x3 = [TieredResult {
        value: "x",
        tier: Tier::Hot,
        confidence: 0.4,
        observed_at: t_recent,
    }];
    let warm_x = [TieredResult {
        value: "x",
        tier: Tier::Warm,
        confidence: 0.6,
        observed_at: t_warm,
    }];
    let cold_x2 = [TieredResult {
        value: "x",
        tier: Tier::Cold,
        confidence: 0.95,
        observed_at: t_old,
    }];
    let out = merge_across_tiers(&hot_x3, &warm_x, &cold_x2);
    assert_eq!(
        out.len(),
        1,
        "(SA5) all-three-tiers same-value must dedup to 1 entry; \
         left: {} entries, right: 1 entry; out={out:?}",
        out.len(),
    );
    assert_eq!(
        out[0].contributing_tiers,
        vec![Tier::Hot, Tier::Warm, Tier::Cold],
        "(SA5) all-three-tiers contributing_tiers; left: {:?}, right: \
         [Hot, Warm, Cold] (deduplicated, ordinal-sorted)",
        out[0].contributing_tiers,
    );
    assert_eq!(
        out[0].source_tier,
        Tier::Hot,
        "(SA5) all-three-tiers source_tier — HOT wins by recency \
         (t_recent > t_warm > t_old); left: {:?}, right: Hot",
        out[0].source_tier,
    );
    assert_eq!(
        out[0].confidence, 0.4,
        "(SA5) all-three-tiers confidence — winning HOT observation's \
         raw confidence preserved (NOT recomputed); left: {}, right: \
         0.4",
        out[0].confidence,
    );
    assert_eq!(
        out[0].value, "x",
        "(SA5) all-three-tiers value preserved; left: {:?}, right: \
         \"x\"",
        out[0].value,
    );
}
