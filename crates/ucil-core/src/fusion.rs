//! G2 group fusion: weighted Reciprocal Rank Fusion (`RRF`) of search hits.
//!
//! Master-plan §5.2 (lines 447-461) defines the G2 search layer as five
//! parallel engines (Probe, ripgrep, `LanceDB`, Zoekt, codedb) whose ranked
//! per-source outputs feed weighted `RRF` to produce a single ranked
//! output with provenance.  This module implements the pure-arithmetic
//! core: the [`G2Source`] enum, hit / result / outcome types, the frozen
//! [`G2_RRF_K`] constant, the [`rrf_weight`] weight table, and the
//! [`fuse_g2_rrf`] fusion function.
//!
//! Master-plan §6.2 line 645 defines the formula
//! `score(d) = Σ_r weight(r) / (k + rank_r(d))` with `k = 60` (tunable,
//! default 60).
//!
//! No subprocess execution lives in this module — wiring real ripgrep /
//! Probe / `LanceDB` clients into [`G2SourceResults`] inputs is deferred
//! to feature `P2-W7-F06` (the `search_code` `MCP` tool, which adapts
//! the in-process ripgrep substrate from `DEC-0009`).

use std::collections::BTreeMap;
use std::path::PathBuf;

// ── Source enum ───────────────────────────────────────────────────────────────

/// Search-engine identifier for `G2` group fusion.
///
/// The 5 variants match master-plan §5.2 line 457 exactly.  Deriving `Ord`
/// makes the enum-discriminant ordering deterministic for tie-breaking
/// inside [`fuse_g2_rrf`] — equal-weight contributors (e.g. ripgrep and
/// `LanceDB`, both 1.5) are sorted by their declaration order, so the
/// output is reproducible across runs.
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
pub enum G2Source {
    /// Plain-text regex search via ripgrep (in-process per `DEC-0009`).
    #[default]
    Ripgrep,
    /// Probe `AST`-aware search (Phase-2 plugin manifest, `WO-0044`).
    Probe,
    /// Vector-similarity search via `LanceDB` (`P2-W7-F09` + `P2-W8-F04`).
    Lancedb,
    /// Zoekt indexed search — Phase-3 source, seated in the enum so the
    /// exhaustive `match` in [`rrf_weight`] enforces compile-time variant
    /// coverage when the wider source-set lands.
    Zoekt,
    /// codedb structured search — Phase-3 source, seated in the enum for
    /// the same compile-time-coverage reason as [`G2Source::Zoekt`].
    Codedb,
}

// ── Hit / source-results / fused-hit / outcome ────────────────────────────────

/// A single per-source hit consumed by [`fuse_g2_rrf`].
///
/// Lines are 1-based.  `start_line == end_line` is permitted.  The
/// `snippet` is the rendered text excerpt from the originating source.
/// The `score` is the per-source raw score the source's own ranker
/// assigned — it is not the fused score (that lives on [`G2FusedHit`]).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct G2Hit {
    /// Path to the file containing the hit.
    pub file_path: PathBuf,
    /// Inclusive 1-based start line.
    pub start_line: u32,
    /// Inclusive 1-based end line.
    pub end_line: u32,
    /// Rendered text excerpt from the originating source.
    pub snippet: String,
    /// Per-source raw score (not the fused score).
    pub score: f64,
}

/// One source's already-ranked list of hits.
///
/// `hits[0]` is rank 1, `hits[1]` is rank 2, etc.  The [`fuse_g2_rrf`]
/// consumer treats `idx + 1` as the 1-based rank in the `RRF` formula.
/// `Default` enables empty-source test inputs.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct G2SourceResults {
    /// Source identifier.
    pub source: G2Source,
    /// Already-ranked hits — `hits[0]` is rank 1.
    pub hits: Vec<G2Hit>,
}

/// A single fused hit emitted by [`fuse_g2_rrf`].
///
/// `contributing_sources` is sorted by descending [`rrf_weight`] then
/// ascending enum-discriminant for ties, so a reader can spot the
/// highest-weight contributor at index 0.  `per_source_ranks` is
/// `(source, rank)` pairs preserving the per-source rank that the
/// originating source assigned to this location — provenance for the
/// future `P2-W7-F06` `search_code` `MCP` tool to surface
/// "Probe ranked this #1, ripgrep ranked this #3".
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct G2FusedHit {
    /// Path to the file.
    pub file_path: PathBuf,
    /// Inclusive 1-based start line.
    pub start_line: u32,
    /// Inclusive 1-based end line.
    pub end_line: u32,
    /// Snippet from the highest-weight contributing source.
    pub snippet: String,
    /// Fused `RRF` score: `Σ_r weight(r) / (k + rank_r)`.
    pub fused_score: f64,
    /// Sources that contributed to this location, sorted by descending
    /// [`rrf_weight`] (ties broken by ascending enum-discriminant).
    pub contributing_sources: Vec<G2Source>,
    /// `(source, rank)` pairs preserving the per-source rank assigned to
    /// this location — provenance for downstream consumers.
    pub per_source_ranks: Vec<(G2Source, u32)>,
}

/// Fused output of [`fuse_g2_rrf`] — `hits` sorted descending by
/// `fused_score`, with `(file_path, start_line, end_line)` ascending as
/// the deterministic tie-break.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct G2FusedOutcome {
    /// Fused hits sorted descending by `fused_score`.  `hits[0]` is the
    /// top result.
    pub hits: Vec<G2FusedHit>,
}

// ── Constants and weight table ────────────────────────────────────────────────

/// `RRF` k parameter per master-plan §6.2 line 645
/// ("k tunable, default 60").
///
/// Typed as `u32` (not `usize`) so the lossless `f64::from(...)` cast
/// inside the `RRF` formula is unambiguous on 32-bit and 64-bit targets
/// identically.
pub const G2_RRF_K: u32 = 60;

/// Per-source weight in the `RRF` formula, per master-plan §5.2 line 457.
///
/// The exhaustive per-variant `match` is the compile-time guarantee that
/// any future [`G2Source`] variant gains an explicit weight — adding a
/// variant without updating this function fails the build (same pattern
/// as `authority_rank` for `G1` fusion in
/// `crates/ucil-daemon/src/executor.rs`).
///
/// `clippy::match_same_arms` is allowed deliberately: collapsing equal-
/// weight arms (e.g. `Ripgrep | Lancedb => 1.5`) would defeat the
/// per-variant compile-time-coverage guarantee — a future variant added
/// to the bag would silently inherit the merged weight rather than
/// forcing a compiler error here.
#[must_use]
#[allow(clippy::match_same_arms)]
pub const fn rrf_weight(source: G2Source) -> f64 {
    match source {
        G2Source::Ripgrep => 1.5,
        G2Source::Probe => 2.0,
        G2Source::Lancedb => 1.5,
        G2Source::Zoekt => 1.0,
        G2Source::Codedb => 1.0,
    }
}

// ── Fusion function ───────────────────────────────────────────────────────────

/// Fuse a slice of per-source ranked results into a single ranked output
/// using weighted Reciprocal Rank Fusion (`RRF`).
///
/// Algorithm (master-plan §5.2 line 457 + §6.2 line 645):
///
/// 1. Group hits by `(file_path, start_line, end_line)` location key
///    using a `BTreeMap` — iteration order is the deterministic location
///    ordering, eliminating an end-of-pass sort on the location key.
/// 2. For each grouped location, compute
///    `fused_score = Σ_r rrf_weight(r) * 1 / (G2_RRF_K + rank_r)` over the
///    contributing sources, where `rank_r` is the 1-based position of the
///    location in source `r`'s `hits` slice.
/// 3. Build `contributing_sources` and sort by descending [`rrf_weight`],
///    breaking ties by ascending enum-discriminant via the derived `Ord`.
/// 4. Pick the snippet from the highest-weight contributing source —
///    `contributing_sources[0]` after the sort above.
/// 5. Sort the final `hits` `Vec` descending by `fused_score`, breaking
///    ties by ascending `(file_path, start_line, end_line)` so the output
///    is fully deterministic.
///
/// The function is pure `CPU` on the input — no `tokio::spawn`, no
/// `tokio::time::timeout`, no subprocess calls.  It never `panic!`s and
/// never returns a `Result` — `RRF` over a possibly-empty slice is just
/// `G2FusedOutcome { hits: vec![] }`.
///
/// # Examples
///
/// ```
/// use std::path::PathBuf;
/// use ucil_core::fusion::{fuse_g2_rrf, G2Hit, G2Source, G2SourceResults};
///
/// let ripgrep = G2SourceResults {
///     source: G2Source::Ripgrep,
///     hits: vec![G2Hit {
///         file_path: PathBuf::from("foo.rs"),
///         start_line: 10,
///         end_line: 20,
///         snippet: "fn foo() // ripgrep".to_owned(),
///         score: 0.8,
///     }],
/// };
/// let probe = G2SourceResults {
///     source: G2Source::Probe,
///     hits: vec![G2Hit {
///         file_path: PathBuf::from("foo.rs"),
///         start_line: 10,
///         end_line: 20,
///         snippet: "fn foo() // probe".to_owned(),
///         score: 0.95,
///     }],
/// };
///
/// let outcome = fuse_g2_rrf(&[ripgrep, probe]);
///
/// assert_eq!(outcome.hits.len(), 1);
/// assert!(outcome.hits[0].fused_score > 0.057);
/// assert!(outcome.hits[0].fused_score < 0.058);
/// assert_eq!(outcome.hits[0].contributing_sources[0], G2Source::Probe);
/// assert_eq!(outcome.hits[0].contributing_sources[1], G2Source::Ripgrep);
/// ```
#[must_use]
#[tracing::instrument(
    name = "ucil.group.search.fusion",
    level = "debug",
    skip(per_source),
    fields(
        input_sources = per_source.len(),
        input_hits = per_source.iter().map(|r| r.hits.len()).sum::<usize>(),
    ),
)]
pub fn fuse_g2_rrf(per_source: &[G2SourceResults]) -> G2FusedOutcome {
    // Step 1: group by location.  `BTreeMap` (not `HashMap`) so iteration
    // order is the deterministic location-key ordering.  `&G2Hit` borrow
    // is sound because `per_source` outlives the fusion call.
    #[allow(clippy::type_complexity)]
    let mut groups: BTreeMap<(PathBuf, u32, u32), Vec<(G2Source, u32, &G2Hit)>> = BTreeMap::new();
    for results in per_source {
        for (idx, hit) in results.hits.iter().enumerate() {
            // 1-based rank.  `try_from` defends against the (unreachable
            // in practice) case of more than `u32::MAX` hits.
            let rank: u32 = u32::try_from(idx + 1).unwrap_or(u32::MAX);
            let key = (hit.file_path.clone(), hit.start_line, hit.end_line);
            groups
                .entry(key)
                .or_default()
                .push((results.source, rank, hit));
        }
    }

    // Step 2-4: per-location fusion.
    let mut hits: Vec<G2FusedHit> = Vec::with_capacity(groups.len());
    for ((file_path, start_line, end_line), contributors) in groups {
        // Step 2: fused score.  `f64::from(u32)` is the lossless
        // conversion that satisfies `clippy::cast_precision_loss`.
        let fused_score: f64 = contributors
            .iter()
            .map(|(src, rank, _)| {
                rrf_weight(*src) * (1.0_f64 / (f64::from(G2_RRF_K) + f64::from(*rank)))
            })
            .sum();

        // `per_source_ranks` in input encounter order — preserves the
        // per_source slice ordering for downstream consumers that want
        // to know "which engine ranked this where".
        let per_source_ranks: Vec<(G2Source, u32)> = contributors
            .iter()
            .map(|(src, rank, _)| (*src, *rank))
            .collect();

        // Step 3: `contributing_sources` sorted by descending weight,
        // ascending enum-discriminant for ties.  `partial_cmp` over the
        // {1.0, 1.5, 2.0} weight set is total (no NaN possible from
        // [`rrf_weight`]); the `unwrap_or(Equal)` is defensive.
        let mut contributing_sources: Vec<G2Source> =
            contributors.iter().map(|(src, _, _)| *src).collect();
        contributing_sources.sort_by(|a, b| {
            let weight_a = rrf_weight(*a);
            let weight_b = rrf_weight(*b);
            weight_b
                .partial_cmp(&weight_a)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.cmp(b))
        });

        // Step 4: snippet from the highest-weight contributing source.
        // `contributing_sources` is non-empty because `contributors` is
        // non-empty by `BTreeMap` construction (we only created a group
        // when we pushed into it).
        let top_source = contributing_sources[0];
        let snippet = contributors
            .iter()
            .find(|(src, _, _)| *src == top_source)
            .map_or_else(String::new, |(_, _, hit)| hit.snippet.clone());

        hits.push(G2FusedHit {
            file_path,
            start_line,
            end_line,
            snippet,
            fused_score,
            contributing_sources,
            per_source_ranks,
        });
    }

    // Step 5: sort hits descending by `fused_score`, breaking ties by
    // ascending `(file_path, start_line, end_line)`.  `partial_cmp`
    // fallback to `Equal` is `NaN`-safe — the formula yields only
    // positive-finite values over the {1.0, 1.5, 2.0} weight set, so the
    // `Equal` branch is unreachable in practice.
    hits.sort_by(|a, b| {
        b.fused_score
            .partial_cmp(&a.fused_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.file_path.cmp(&b.file_path))
            .then_with(|| a.start_line.cmp(&b.start_line))
            .then_with(|| a.end_line.cmp(&b.end_line))
    });

    G2FusedOutcome { hits }
}

// ── Frozen acceptance test ────────────────────────────────────────────────────
//
// Per `DEC-0007`, the frozen acceptance selector lives at MODULE ROOT —
// NOT wrapped in `#[cfg(test)] mod tests { … }` — so the `cargo test`
// selector `fusion::test_g2_rrf_weights` resolves to
// `ucil_core::fusion::test_g2_rrf_weights` directly.

#[cfg(test)]
#[test]
#[allow(clippy::too_many_lines, clippy::float_cmp)]
fn test_g2_rrf_weights() {
    let ripgrep = G2SourceResults {
        source: G2Source::Ripgrep,
        hits: vec![
            G2Hit {
                file_path: PathBuf::from("foo.rs"),
                start_line: 10,
                end_line: 20,
                snippet: "fn foo() // ripgrep".to_owned(),
                score: 0.8,
            },
            G2Hit {
                file_path: PathBuf::from("foo.rs"),
                start_line: 30,
                end_line: 40,
                snippet: "fn bar() // ripgrep".to_owned(),
                score: 0.7,
            },
            G2Hit {
                file_path: PathBuf::from("baz.rs"),
                start_line: 5,
                end_line: 10,
                snippet: "fn baz() // ripgrep".to_owned(),
                score: 0.6,
            },
        ],
    };
    let probe = G2SourceResults {
        source: G2Source::Probe,
        hits: vec![
            G2Hit {
                file_path: PathBuf::from("foo.rs"),
                start_line: 10,
                end_line: 20,
                snippet: "fn foo() // probe".to_owned(),
                score: 0.95,
            },
            G2Hit {
                file_path: PathBuf::from("qux.rs"),
                start_line: 1,
                end_line: 3,
                snippet: "fn qux() // probe".to_owned(),
                score: 0.5,
            },
        ],
    };
    let lancedb = G2SourceResults {
        source: G2Source::Lancedb,
        hits: vec![
            G2Hit {
                file_path: PathBuf::from("foo.rs"),
                start_line: 30,
                end_line: 40,
                snippet: "fn bar() // lancedb".to_owned(),
                score: 0.9,
            },
            G2Hit {
                file_path: PathBuf::from("qux.rs"),
                start_line: 1,
                end_line: 3,
                snippet: "fn qux() // lancedb".to_owned(),
                score: 0.7,
            },
        ],
    };

    let outcome = fuse_g2_rrf(&[ripgrep, probe, lancedb]);

    // ── Sub-assertion 1: location grouping (4 distinct locations) ──
    assert_eq!(
        outcome.hits.len(),
        4,
        "(1) location grouping: expected 4 fused hits — locations \
         (foo.rs, 10, 20), (foo.rs, 30, 40), (baz.rs, 5, 10), \
         (qux.rs, 1, 3); got {}: outcome={outcome:?}",
        outcome.hits.len(),
    );

    // ── Sub-assertion 2: top-ranked hit is (foo.rs, 10, 20) ──
    assert!(
        outcome.hits[0].file_path.ends_with("foo.rs"),
        "(2) top hit file_path must end with foo.rs; got {:?}: outcome={outcome:?}",
        outcome.hits[0].file_path,
    );
    assert_eq!(
        outcome.hits[0].start_line, 10,
        "(2) top hit start_line must be 10; got {}: outcome.hits[0]={:?}",
        outcome.hits[0].start_line, outcome.hits[0],
    );
    assert_eq!(
        outcome.hits[0].end_line, 20,
        "(2) top hit end_line must be 20; got {}: outcome.hits[0]={:?}",
        outcome.hits[0].end_line, outcome.hits[0],
    );
    assert!(
        outcome.hits[0].fused_score > 0.057 && outcome.hits[0].fused_score < 0.058,
        "(2) top hit fused_score must be in (0.057, 0.058) for \
         2.0/61 + 1.5/61 ≈ 0.05738; got {}: outcome.hits[0]={:?}",
        outcome.hits[0].fused_score,
        outcome.hits[0],
    );

    // ── Sub-assertion 3: Probe×2.0 dominance — load-bearing ──
    // (qux.rs, 1, 3) at Probe rank 2 + Lancedb rank 2 = 2.0/62 + 1.5/62 ≈ 0.0565
    // (foo.rs, 30, 40) at Lancedb rank 1 + Ripgrep rank 2 = 1.5/61 + 1.5/62 ≈ 0.0488
    // qux.rs:1-3 must outrank foo.rs:30-40 — if Probe's 2.0 weight is
    // silently changed to ≤ 1.5, the ordering reverses.
    assert!(
        outcome.hits[1].file_path.ends_with("qux.rs")
            && outcome.hits[1].start_line == 1
            && outcome.hits[1].end_line == 3,
        "(3) hits[1] must be (qux.rs, 1, 3) — Probe×2.0 + Lancedb×1.5 at \
         rank 2 each must outrank foo.rs:30-40 (Lancedb×1.5 rank 1 + \
         Ripgrep×1.5 rank 2); got hits[1]={:?}, hits[2]={:?} \
         (fused_scores: hits[1]={}, hits[2]={})",
        outcome.hits[1],
        outcome.hits[2],
        outcome.hits[1].fused_score,
        outcome.hits[2].fused_score,
    );
    assert!(
        outcome.hits[2].file_path.ends_with("foo.rs")
            && outcome.hits[2].start_line == 30
            && outcome.hits[2].end_line == 40,
        "(3) hits[2] must be (foo.rs, 30, 40); got hits[2]={:?} \
         (full outcome={outcome:?})",
        outcome.hits[2],
    );

    // ── Sub-assertion 4: constants and weight table ──
    assert_eq!(G2_RRF_K, 60, "(4) G2_RRF_K must be 60");
    assert_eq!(
        rrf_weight(G2Source::Probe),
        2.0,
        "(4) rrf_weight(Probe) must be 2.0"
    );
    assert_eq!(
        rrf_weight(G2Source::Ripgrep),
        1.5,
        "(4) rrf_weight(Ripgrep) must be 1.5"
    );
    assert_eq!(
        rrf_weight(G2Source::Lancedb),
        1.5,
        "(4) rrf_weight(Lancedb) must be 1.5"
    );
    assert_eq!(
        rrf_weight(G2Source::Zoekt),
        1.0,
        "(4) rrf_weight(Zoekt) must be 1.0"
    );
    assert_eq!(
        rrf_weight(G2Source::Codedb),
        1.0,
        "(4) rrf_weight(Codedb) must be 1.0"
    );

    // ── Sub-assertion 5: per_source_ranks provenance (order-insensitive) ──
    let mut actual_ranks = outcome.hits[0].per_source_ranks.clone();
    actual_ranks.sort();
    let mut expected_ranks: Vec<(G2Source, u32)> =
        vec![(G2Source::Ripgrep, 1_u32), (G2Source::Probe, 1_u32)];
    expected_ranks.sort();
    assert_eq!(
        actual_ranks, expected_ranks,
        "(5) per_source_ranks for (foo.rs, 10, 20) must equal the multiset \
         {{(Probe, 1), (Ripgrep, 1)}}; got {:?}",
        outcome.hits[0].per_source_ranks,
    );

    // ── Sub-assertion 6: descending sort invariant ──
    for w in outcome.hits.windows(2) {
        assert!(
            w[0].fused_score >= w[1].fused_score,
            "(6) hits not sorted desc by fused_score: {:?}",
            outcome.hits,
        );
    }

    // ── Sub-assertion 7: contributing_sources highest-weight-first ──
    // For (foo.rs, 10, 20), Ripgrep (1.5) and Probe (2.0) contribute.
    // Sorted by descending weight: Probe first, then Ripgrep.
    assert_eq!(
        outcome.hits[0].contributing_sources[0],
        G2Source::Probe,
        "(7) contributing_sources[0] must be Probe (weight 2.0 \
         beats Ripgrep 1.5); got {:?}",
        outcome.hits[0].contributing_sources,
    );
    assert_eq!(
        outcome.hits[0].contributing_sources[1],
        G2Source::Ripgrep,
        "(7) contributing_sources[1] must be Ripgrep (weight 1.5); \
         got {:?}",
        outcome.hits[0].contributing_sources,
    );
}
