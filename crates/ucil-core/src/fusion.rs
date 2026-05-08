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

// ── §6.2 Query-type weight matrix + classifier (P3-W9-F01) ────────────────────
//
// Master-plan §6.2 lines 643-658 freeze a 10-row × 8-column weight
// matrix that drives cross-group `RRF` fusion.  The 10 rows are the
// canonical `QueryType` variants; the 8 columns are groups
// `[G1, G2, G3, G4, G5, G6, G7, G8]`.  This block lands the matrix
// alongside the deterministic classifier so feature `P3-W9-F01` ships
// the `§6.2` contract end-to-end.  The cross-group `RRF` engine that
// consumes [`group_weights_for`] is `P3-W9-F04`, deferred to a
// follow-up work-order.

/// One of the 10 canonical UCIL query types per master-plan §6.2.
///
/// Variant declaration order MUST stay aligned with the `§6.2` matrix
/// row order so that `query_type as usize == QUERY_WEIGHT_MATRIX` row
/// index is correct (verified by [`group_weights_for`] sub-assertions
/// SA4 + SA5 in the frozen test `test_deterministic_classifier`).
///
/// The enum is intentionally NOT `#[non_exhaustive]` — the 10 variants
/// are frozen by the master-plan; introducing a new query type
/// requires both a master-plan amendment and an `ADR`.
///
/// `serde(rename_all = "snake_case")` produces the JSON wire labels
/// `"understand_code"`, `"find_definition"`, … exactly matching the
/// `§6.2` row names.
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
pub enum QueryType {
    /// "Explain what a file/function/module does, why it exists, its
    /// context" — master-plan §3.2 row 1.  Default per `§8.6` lines
    /// 817-822 (most-permissive bonus-context fallback when both
    /// `tool_name` AND `reason_keywords` are unknown).
    #[default]
    UnderstandCode,
    /// "Go-to-definition with full context" — master-plan §3.2 row 2.
    FindDefinition,
    /// "All references to a symbol" — master-plan §3.2 row 3.
    FindReferences,
    /// "Hybrid search: text + structural + semantic" — master-plan
    /// §3.2 row 4.
    SearchCode,
    /// "Optimal context for editing a file/region" — master-plan §3.2
    /// row 6.  Also the target for `Refactor`-flavoured intents
    /// surfaced via the keyword fallback path.
    GetContextForEdit,
    /// "Upstream and downstream dependency chains" — master-plan §3.2
    /// row 9.
    TraceDependencies,
    /// "What would be affected by changing this code?" — master-plan
    /// §3.2 row 10.
    BlastRadius,
    /// "Analyze diff/PR against conventions, quality, security,
    /// tests, blast radius" — master-plan §3.2 row 13.
    ReviewChanges,
    /// "Run lint + type check + security scan on specified code" —
    /// master-plan §3.2 row 14.
    CheckQuality,
    /// "Store or retrieve agent learnings, decisions, observations" —
    /// master-plan §3.2 row 12.  The `§6.2` sentinel row — its weight
    /// vector `[0, 0, 3.0, 0, 0, 0, 0, 0]` encodes the strict
    /// knowledge-only constraint and is the canary for matrix-row-
    /// shift bugs.
    Remember,
}

/// Output of [`classify_query`] — the query-type plus metadata the
/// downstream cross-group fusion engine (`P3-W9-F04`) and bonus-
/// context selector (`§6.3`) consume.
///
/// The `group_weight_overrides` field is reserved for future
/// per-classification overrides on top of the static
/// [`QUERY_WEIGHT_MATRIX`]; the classifier emits an empty map for now
/// (the override surface lands when the cross-group `RRF` engine
/// gains a runtime knob — master-plan §6.2 line 645).  `intent_hint`
/// and `domain_tags` are reserved for future enrichment by the LLM
/// `QueryInterpreter` agent (`P3.5-W12-F02`); the deterministic-
/// fallback classifier emits empty values per `DEC-0018`'s phase
/// boundary.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ClassifierOutput {
    /// Selected query type.
    pub query_type: QueryType,
    /// Optional human-readable hint about the user's intent — `None`
    /// from the deterministic classifier; populated by
    /// `P3.5-W12-F02`.
    pub intent_hint: Option<String>,
    /// Domain tokens that match the canonical UCIL vocabulary —
    /// empty from the deterministic classifier; populated by the
    /// `CEQP` `parse_reason` path (`P3-W9-F02`).
    pub domain_tags: Vec<String>,
    /// Per-classification overrides on top of [`QUERY_WEIGHT_MATRIX`]
    /// — empty from the deterministic classifier (the
    /// `BTreeMap::new()` default).  Reserved for `P3-W9-F04`.
    pub group_weight_overrides: BTreeMap<G2Source, f32>,
}

/// `tool_name → QueryType` static lookup table.  Covers the 12
/// user-facing tools shipped or in-flight by phase 3 week 9 per
/// master-plan §3.2 lines 215-226.  Tools omitted here fall through
/// to the keyword scan in [`classify_query`].
const TOOL_NAME_MAP: &[(&str, QueryType)] = &[
    ("understand_code", QueryType::UnderstandCode),
    ("find_definition", QueryType::FindDefinition),
    ("find_references", QueryType::FindReferences),
    ("search_code", QueryType::SearchCode),
    ("find_similar", QueryType::SearchCode),
    ("get_context_for_edit", QueryType::GetContextForEdit),
    ("get_conventions", QueryType::UnderstandCode),
    ("trace_dependencies", QueryType::TraceDependencies),
    ("blast_radius", QueryType::BlastRadius),
    ("review_changes", QueryType::ReviewChanges),
    ("check_quality", QueryType::CheckQuality),
    ("remember", QueryType::Remember),
];

/// Keyword → `QueryType` precedence list.  [`classify_query`] joins
/// the lower-cased keyword slice with single spaces, pads with a
/// leading + trailing space, then scans this slice for the first
/// matching pattern.  Order matters — see the rustdoc on
/// [`classify_query`] for the precedence ladder.
///
/// The `"refactor" | "rename" | "cleanup"` patterns map to
/// [`QueryType::GetContextForEdit`] because the master-plan §6.2
/// matrix has no dedicated `Refactor` row — refactor-flavoured
/// intents borrow the `get_context_for_edit` weight profile (high
/// G5 conventions weight, balanced retrieval).
const KEYWORD_RULES: &[(&str, QueryType)] = &[
    ("definition", QueryType::FindDefinition),
    ("declared", QueryType::FindDefinition),
    ("references", QueryType::FindReferences),
    ("callers", QueryType::FindReferences),
    ("usages", QueryType::FindReferences),
    ("blast radius", QueryType::BlastRadius),
    ("impact", QueryType::BlastRadius),
    ("affected", QueryType::BlastRadius),
    ("trace", QueryType::TraceDependencies),
    ("depends on", QueryType::TraceDependencies),
    ("refactor", QueryType::GetContextForEdit),
    ("rename", QueryType::GetContextForEdit),
    ("cleanup", QueryType::GetContextForEdit),
    ("review", QueryType::ReviewChanges),
    ("diff", QueryType::ReviewChanges),
    (" pr ", QueryType::ReviewChanges),
    ("lint", QueryType::CheckQuality),
    ("quality", QueryType::CheckQuality),
    ("vulnerab", QueryType::CheckQuality),
    ("find code", QueryType::SearchCode),
    ("search", QueryType::SearchCode),
    ("grep", QueryType::SearchCode),
    ("remember", QueryType::Remember),
    ("save", QueryType::Remember),
    ("persist", QueryType::Remember),
    ("explain", QueryType::UnderstandCode),
    ("how does", QueryType::UnderstandCode),
    ("what is", QueryType::UnderstandCode),
];

/// 10 × 8 weight matrix — master-plan §6.2 lines 649-658.
///
/// Rows are indexed by `QueryType as usize`; each row is the 8-column
/// weight vector `[G1, G2, G3, G4, G5, G6, G7, G8]`.  This is the
/// data table the cross-group `RRF` engine (`P3-W9-F04`, deferred)
/// will consume via [`group_weights_for`].  Validating any row
/// realignment is the job of sub-assertions SA4 + SA5 in
/// [`test_deterministic_classifier`].
const QUERY_WEIGHT_MATRIX: [[f32; 8]; 10] = [
    // §6.2 line 649: understand_code
    [2.0, 1.0, 2.5, 1.5, 2.0, 0.5, 1.0, 0.5],
    // §6.2 line 650: find_definition
    [3.0, 1.5, 1.0, 0.5, 0.5, 0.0, 0.5, 0.0],
    // §6.2 line 651: find_references
    [3.0, 2.0, 0.5, 1.0, 0.5, 0.0, 0.5, 0.0],
    // §6.2 line 652: search_code
    [1.5, 3.0, 0.5, 0.5, 1.0, 0.0, 0.0, 0.0],
    // §6.2 line 653: get_context_for_edit
    [2.0, 2.0, 1.5, 1.5, 2.5, 0.5, 1.5, 1.0],
    // §6.2 line 654: trace_dependencies
    [1.5, 0.5, 1.5, 3.0, 0.5, 0.0, 0.5, 0.0],
    // §6.2 line 655: blast_radius
    [1.5, 0.5, 1.5, 3.0, 0.5, 0.5, 1.0, 1.0],
    // §6.2 line 656: review_changes
    [1.5, 1.0, 1.5, 1.5, 0.5, 1.0, 3.0, 2.5],
    // §6.2 line 657: check_quality
    [1.0, 0.5, 1.0, 1.5, 0.5, 0.5, 3.0, 2.5],
    // §6.2 line 658: remember (sentinel — strict knowledge-only)
    [0.0, 0.0, 3.0, 0.0, 0.0, 0.0, 0.0, 0.0],
];

/// Lookup the `[G1..G8]` weight row for `query_type` per master-plan
/// §6.2 line 645.  Returns the 8-element row from
/// [`QUERY_WEIGHT_MATRIX`] keyed by `query_type as usize`.
///
/// Variant-declaration order on [`QueryType`] is the source of truth
/// for the row index — the `[[f32; 8]; 10]` array length is the
/// compile-time guarantee that exactly 10 rows are present.
///
/// # Examples
///
/// ```
/// use ucil_core::fusion::{group_weights_for, QueryType};
///
/// // §6.2 line 658 — `Remember` is the sentinel row.
/// assert_eq!(
///     group_weights_for(QueryType::Remember),
///     [0.0, 0.0, 3.0, 0.0, 0.0, 0.0, 0.0, 0.0],
/// );
/// ```
#[must_use]
pub const fn group_weights_for(query_type: QueryType) -> [f32; 8] {
    QUERY_WEIGHT_MATRIX[query_type as usize]
}

/// Deterministic-fallback classifier — master-plan §7.1 lines 693-695.
///
/// Per §7.1 ("when `provider = none` is configured, the keyword
/// classifier is the path") and §6.1 lines 585-641 (which places this
/// function under the Query-Interpreter step of the pipeline), this is
/// the fallback brain that runs when no LLM provider is configured.
///
/// Precedence ladder:
///
/// 1. **`tool_name`** is the primary signal — match against the
///    static [`TOOL_NAME_MAP`] derived from master-plan §3.2 lines
///    211-237.  A hit returns immediately.
/// 2. **`reason_keywords`** is the tie-breaker — when `tool_name`
///    is unknown, lower-case the keywords, join with single spaces
///    (and pad with one space on each end), then scan for the first
///    matching pattern in the precedence-ordered keyword table.  A
///    hit returns immediately.
/// 3. **Default** → [`QueryType::UnderstandCode`] per master-plan
///    §8.6 lines 817-822 (most-permissive bonus-context default).
///
/// The function is pure: no IO, no async, no logging.  It never
/// panics and always returns a valid [`ClassifierOutput`].
///
/// # Examples
///
/// ```
/// use ucil_core::fusion::{classify_query, QueryType};
///
/// // Tool-name primary signal.
/// assert_eq!(
///     classify_query("find_definition", &[]).query_type,
///     QueryType::FindDefinition,
/// );
///
/// // Keyword fallback when the tool name is unknown.
/// assert_eq!(
///     classify_query("unknown_tool", &["find", "references"]).query_type,
///     QueryType::FindReferences,
/// );
///
/// // Default when both tool name AND keywords are unknown.
/// assert_eq!(
///     classify_query("", &[]).query_type,
///     QueryType::UnderstandCode,
/// );
/// ```
#[must_use]
pub fn classify_query(tool_name: &str, reason_keywords: &[&str]) -> ClassifierOutput {
    // Step 1: tool_name primary signal.
    for &(name, qt) in TOOL_NAME_MAP {
        if name == tool_name {
            return ClassifierOutput {
                query_type: qt,
                intent_hint: None,
                domain_tags: Vec::new(),
                group_weight_overrides: BTreeMap::new(),
            };
        }
    }

    // Step 2: keyword fallback.  Lower-case once, join with single
    // spaces, then scan precedence rules.  Pad with leading +
    // trailing spaces so the `" pr "` rule (which guards against
    // matching the literal `pr` inside `print` / `provider`) can
    // still hit a single-token slice like `&["pr"]` (joined → `" pr "`).
    let mut joined = String::from(" ");
    for (idx, kw) in reason_keywords.iter().enumerate() {
        if idx > 0 {
            joined.push(' ');
        }
        for c in kw.chars() {
            for lc in c.to_lowercase() {
                joined.push(lc);
            }
        }
    }
    joined.push(' ');

    for &(pattern, qt) in KEYWORD_RULES {
        if joined.contains(pattern) {
            return ClassifierOutput {
                query_type: qt,
                intent_hint: None,
                domain_tags: Vec::new(),
                group_weight_overrides: BTreeMap::new(),
            };
        }
    }

    // Step 3: default per master-plan §8.6.
    ClassifierOutput {
        query_type: QueryType::UnderstandCode,
        intent_hint: None,
        domain_tags: Vec::new(),
        group_weight_overrides: BTreeMap::new(),
    }
}

// ── Conflict resolution (P3-W10-F10) ──────────────────────────────────────────
//
// Master-plan §18 Phase 3 Week 10 deliverable #4 (line 1811):
//
// > "Conflict resolution: agent-based with source authority as soft guidance"
//
// The agent layer (a `ConflictMediator` LLM agent, Phase 3.5+) consumes the
// deterministic [`resolve_conflict`] surface as its input — when the
// deterministic resolver returns [`ConflictResolution::Resolved`] there is
// nothing for the agent to mediate; when it returns
// [`ConflictResolution::Unresolvable`] the agent layer reads the
// retained candidate slate and picks the winner with semantic context the
// deterministic resolver lacks.
//
// The source-authority hierarchy `LSP/AST > SCIP > KG > text` (master-plan
// §18 line 1811) is implemented as a `#[derive(Ord)]`-able enum where the
// LOWEST discriminant is the MOST authoritative source.  Determinism + the
// `min()` reduction does the work; no tracing, no IO, no async.
//
// Production-side wiring of this resolver into the daemon's response-
// assembly pipeline is OUT OF SCOPE for this WO (deferred to the consumer
// WO that lands the F09 quality-maximalist response assembly + F11 bonus-
// context selector — both of which depend on F01 Aider repo-map first).

/// Source-authority hierarchy for conflict resolution per master-plan §18
/// Phase 3 Week 10 deliverable #4 (line 1811): "LSP/AST > SCIP > KG > text".
///
/// The discriminant order is `LspAst = 0` < `Scip = 1` < `Kg = 2` <
/// `Text = 3` so the natural derived `Ord` produces
/// `LspAst < Scip < Kg < Text` — the LOWEST discriminant is the MOST
/// authoritative source.  The [`resolve_conflict`] reducer uses
/// `.iter().map(|c| c.source).min()` to pick the highest-priority tier;
/// inverting the discriminant order would silently invert the precedence
/// (the M1 verifier mutation exercises exactly this failure mode).
///
/// `Copy` is sound — the enum carries no fields.  The hash + comparison
/// derives are cheap and let downstream consumers index into a
/// `BTreeMap<SourceAuthority, _>` if they need to bin candidates by tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SourceAuthority {
    /// LSP/AST tier — the most authoritative source (semantic compiler-
    /// grade signal: LSP `textDocument/definition`, tree-sitter AST nodes,
    /// rust-analyzer symbol resolution).
    LspAst,
    /// SCIP tier — symbol-coverage-information-protocol indexes that
    /// pre-compute cross-file reference graphs.  Slightly less
    /// authoritative than live LSP/AST because SCIP indexes can lag
    /// behind in-flight edits.
    Scip,
    /// Knowledge-graph tier — the cold-tier curated facts persisted in
    /// `memory.db`.  Authoritative for high-level architecture +
    /// convention claims but lower-trust than compiler signal for raw
    /// symbol resolution.
    Kg,
    /// Plain-text tier — ripgrep / Probe / regex matches on raw file
    /// content.  The lowest-authority tier — text matches drive search
    /// candidates but never override structural signal.
    Text,
}

/// One candidate in a conflict-resolution input slate.
///
/// `value` is the candidate payload (the disputed field — typically a
/// symbol name, a file path, a definition location, …).  `source` is the
/// candidate's [`SourceAuthority`] tier.  `confidence` is the candidate-
/// originating tier's self-reported certainty in `[0.0, 1.0]` — used by
/// [`resolve_conflict`] only as the tie-break within a single tier (a
/// higher-authority tier with low confidence still beats a lower-
/// authority tier with high confidence per master-plan §18 line 1811's
/// "source authority as soft guidance" hierarchy).
///
/// The struct is generic over `T` so the same surface fits any
/// disputed-payload shape — `&'static str` in the frozen test, more
/// elaborate in the daemon-side consumer.
#[derive(Debug, Clone, PartialEq)]
pub struct ConflictCandidate<T> {
    /// The disputed payload value.
    pub value: T,
    /// The candidate's source-authority tier.
    pub source: SourceAuthority,
    /// The candidate-originating tier's self-reported confidence in
    /// `[0.0, 1.0]`.
    pub confidence: f64,
}

/// Result of [`resolve_conflict`] over a slate of candidates.
///
/// `Resolved` carries the winning value + winning source + winning
/// confidence — the deterministic resolver could pick a single dominant
/// candidate.
///
/// `Unresolvable` carries the FULL retained candidate slate (NOT just the
/// tied-top-tier subset — lower-tier candidates are preserved so the
/// downstream agent-mediator surface can read the full slate).  The
/// `reason` is a short human-readable string ("empty input" / "tied at
/// tier {source:?}") naming WHY the deterministic resolver could not
/// pick a winner.  The agent-mediator (Phase 3.5+) reads `Unresolvable`
/// and consumes its semantic context to break the tie.
#[derive(Debug, Clone, PartialEq)]
pub enum ConflictResolution<T> {
    /// The resolver picked a single dominant candidate.
    Resolved {
        /// The winning value.
        value: T,
        /// The winning candidate's source tier.
        source: SourceAuthority,
        /// The winning candidate's confidence.
        confidence: f64,
    },
    /// The resolver could not pick a single winner — the retained
    /// candidate slate is forwarded to the agent-mediator surface.
    Unresolvable {
        /// The retained candidate slate (preserves all input candidates
        /// including lower-tier candidates).
        candidates: Vec<ConflictCandidate<T>>,
        /// Short human-readable reason naming why resolution failed.
        reason: String,
    },
}

/// Resolve a slate of [`ConflictCandidate`]s by source-authority hierarchy.
///
/// Implements feature `P3-W10-F10` per master-plan §18 Phase 3 Week 10
/// deliverable #4 (line 1811): "LSP/AST > SCIP > KG > text".  Issued by
/// `WO-0084`.
///
/// Algorithm:
///
/// 1. **Empty input** → [`ConflictResolution::Unresolvable`] with empty
///    `candidates` and `reason = "empty input"`.
/// 2. **Single candidate** → [`ConflictResolution::Resolved`] with that
///    candidate's value/source/confidence.
/// 3. **Multi-candidate** → find the highest-priority [`SourceAuthority`]
///    (smallest discriminant); collect all candidates at that priority.
///    - If they all carry the same `value` (per `T: PartialEq`) →
///      [`ConflictResolution::Resolved`] with the highest-confidence
///      member of the tied-tier subset.
///    - If values differ at the top tier →
///      [`ConflictResolution::Unresolvable`] with the FULL input slate
///      retained (lower-tier candidates included) and `reason` of the
///      form `"tied at tier {top_source:?}"`.
///
/// Authority-precedence dominates raw confidence: a `LspAst` candidate
/// with `confidence = 0.5` BEATS a `Text` candidate with
/// `confidence = 0.99` per master-plan §18 line 1811 ("source authority
/// as soft guidance" — soft for the agent layer, hard for the
/// deterministic reducer).
///
/// The function is pure: no IO, no async, no logging.  No `tracing`
/// span — the §15.2 carve-out applies (pure-deterministic CPU-bound
/// module per `WO-0067` §`lessons_applied` #5).
///
/// # Examples
///
/// ```
/// use ucil_core::fusion::{resolve_conflict, ConflictCandidate, ConflictResolution, SourceAuthority};
///
/// // LSP/AST beats Text even at much lower confidence.
/// let lsp = ConflictCandidate {
///     value: "Symbol::Foo",
///     source: SourceAuthority::LspAst,
///     confidence: 0.5,
/// };
/// let text = ConflictCandidate {
///     value: "Symbol::Bar",
///     source: SourceAuthority::Text,
///     confidence: 0.99,
/// };
/// let outcome = resolve_conflict(&[lsp.clone(), text]);
/// assert!(matches!(
///     outcome,
///     ConflictResolution::Resolved { source: SourceAuthority::LspAst, .. }
/// ));
/// ```
#[must_use]
pub fn resolve_conflict<T: Clone + PartialEq>(
    candidates: &[ConflictCandidate<T>],
) -> ConflictResolution<T> {
    // Step 1: empty input.
    if candidates.is_empty() {
        return ConflictResolution::Unresolvable {
            candidates: Vec::new(),
            reason: "empty input".to_owned(),
        };
    }

    // Step 2: single candidate — short-circuit before the .min() reduction
    // so the common single-source path is one allocation.
    if candidates.len() == 1 {
        let c = &candidates[0];
        return ConflictResolution::Resolved {
            value: c.value.clone(),
            source: c.source,
            confidence: c.confidence,
        };
    }

    // Step 3: multi-candidate.  Find the highest-priority source tier.
    // Manual loop avoids `.min().unwrap()` panic paths — `candidates[0]`
    // is safe by structural invariant (steps 1 + 2 short-circuit empty
    // and single-candidate inputs, so length >= 2 here).
    let mut top_source = candidates[0].source;
    for c in &candidates[1..] {
        if c.source < top_source {
            top_source = c.source;
        }
    }

    // Collect candidates at the top tier (non-empty by construction —
    // `top_source` came from a candidate above).
    let top_tier: Vec<&ConflictCandidate<T>> = candidates
        .iter()
        .filter(|c| c.source == top_source)
        .collect();

    // Sub-step 3a: all top-tier values agree → resolve to the highest-
    // confidence top-tier candidate.
    let first_value = &top_tier[0].value;
    if top_tier.iter().all(|c| c.value == *first_value) {
        // Pick the highest-confidence member of the tied-tier subset
        // via a manual loop — avoids `max_by(...).unwrap()` panic paths
        // and the `clippy::missing_panics_doc` lint.
        let mut winner: &ConflictCandidate<T> = top_tier[0];
        for c in &top_tier[1..] {
            if c.confidence > winner.confidence {
                winner = *c;
            }
        }
        return ConflictResolution::Resolved {
            value: winner.value.clone(),
            source: winner.source,
            confidence: winner.confidence,
        };
    }

    // Sub-step 3b: top-tier values disagree → emit Unresolvable retaining
    // the FULL input slate (lower-tier candidates preserved for the
    // agent-mediator surface).
    ConflictResolution::Unresolvable {
        candidates: candidates.to_vec(),
        reason: format!("tied at tier {top_source:?}"),
    }
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

// ── Frozen acceptance test — P3-W9-F01 deterministic classifier ───────────────
//
// Per `DEC-0007`, the frozen selector lives at MODULE ROOT so the
// `cargo test -p ucil-core fusion::test_deterministic_classifier`
// selector resolves directly without traversing a `mod tests {}`
// barrier.  Sub-assertions are inline-rustdoc-numbered SA1..SA6 so
// failure messages map to a specific assertion-of-record.

#[cfg(test)]
#[test]
#[allow(clippy::too_many_lines, clippy::float_cmp)]
fn test_deterministic_classifier() {
    // ── SA1: tool_name primary signal — all 12 §3.2 mappings ─────────
    //
    // Every entry in `TOOL_NAME_MAP` MUST resolve to its declared
    // `QueryType`.  This is the load-bearing assertion against the
    // M1 verifier mutation (bypass tool_name match to default).
    let tool_cases: &[(&str, QueryType)] = &[
        ("understand_code", QueryType::UnderstandCode),
        ("find_definition", QueryType::FindDefinition),
        ("find_references", QueryType::FindReferences),
        ("search_code", QueryType::SearchCode),
        ("find_similar", QueryType::SearchCode),
        ("get_context_for_edit", QueryType::GetContextForEdit),
        ("get_conventions", QueryType::UnderstandCode),
        ("trace_dependencies", QueryType::TraceDependencies),
        ("blast_radius", QueryType::BlastRadius),
        ("review_changes", QueryType::ReviewChanges),
        ("check_quality", QueryType::CheckQuality),
        ("remember", QueryType::Remember),
    ];
    for &(name, expected) in tool_cases {
        let got = classify_query(name, &[]).query_type;
        assert_eq!(
            got, expected,
            "(SA1) tool_name primary signal: classify_query({name:?}, &[]) \
             must yield {expected:?}; got {got:?}"
        );
    }

    // ── SA2: keyword fallback when tool_name is unknown ──────────────
    //
    // `find` alone matches no rule, but `references` matches the
    // `references` rule → FindReferences.  Validates that the
    // keyword scan runs after the tool-name miss AND that joining
    // multi-token slices works.
    let out = classify_query("unknown_tool", &["find", "references"]);
    assert_eq!(
        out.query_type,
        QueryType::FindReferences,
        "(SA2) keyword fallback: classify_query(\"unknown_tool\", &[\"find\", \
         \"references\"]) must yield FindReferences; got {:?}",
        out.query_type,
    );

    // Bonus: phrase patterns work via the space-padded join.
    let out = classify_query("unknown_tool", &["blast", "radius"]);
    assert_eq!(
        out.query_type,
        QueryType::BlastRadius,
        "(SA2) phrase fallback: classify_query(\"unknown_tool\", &[\"blast\", \
         \"radius\"]) must yield BlastRadius; got {:?}",
        out.query_type,
    );

    // ── SA3: default when tool AND keywords both unknown ─────────────
    //
    // §8.6 lines 817-822 — most-permissive bonus-context default.
    let out = classify_query("", &[]);
    assert_eq!(
        out.query_type,
        QueryType::UnderstandCode,
        "(SA3) default when both unknown: classify_query(\"\", &[]) must \
         yield UnderstandCode; got {:?}",
        out.query_type,
    );
    // ClassifierOutput defaults are populated correctly.
    assert!(
        out.intent_hint.is_none(),
        "(SA3) intent_hint must be None from the deterministic path"
    );
    assert!(
        out.domain_tags.is_empty(),
        "(SA3) domain_tags must be empty from the deterministic path"
    );
    assert!(
        out.group_weight_overrides.is_empty(),
        "(SA3) group_weight_overrides must be empty from the deterministic path"
    );

    // ── SA4: group-weight matrix shape (4 query types) ───────────────
    //
    // §6.2 line 649 — UnderstandCode row.
    assert_eq!(
        group_weights_for(QueryType::UnderstandCode),
        [2.0, 1.0, 2.5, 1.5, 2.0, 0.5, 1.0, 0.5],
        "(SA4) UnderstandCode row mismatch — §6.2 line 649"
    );
    // §6.2 line 650 — FindDefinition row.  Load-bearing against
    // the M2 verifier mutation (swap UnderstandCode ↔ FindDefinition
    // rows in QUERY_WEIGHT_MATRIX).
    assert_eq!(
        group_weights_for(QueryType::FindDefinition),
        [3.0, 1.5, 1.0, 0.5, 0.5, 0.0, 0.5, 0.0],
        "(SA4) FindDefinition row mismatch — §6.2 line 650"
    );
    // §6.2 line 655 — BlastRadius row.
    assert_eq!(
        group_weights_for(QueryType::BlastRadius),
        [1.5, 0.5, 1.5, 3.0, 0.5, 0.5, 1.0, 1.0],
        "(SA4) BlastRadius row mismatch — §6.2 line 655"
    );

    // ── SA5: Remember row is the §6.2 sentinel ───────────────────────
    //
    // §6.2 line 658 — `[0, 0, 3.0, 0, 0, 0, 0, 0]` encodes the strict
    // knowledge-only constraint.  This row is the canary for matrix-
    // row-shift bugs (any insertion above Remember in the matrix
    // would break this assertion).
    assert_eq!(
        group_weights_for(QueryType::Remember),
        [0.0, 0.0, 3.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        "(SA5) Remember sentinel row mismatch — §6.2 line 658 \
         (knowledge-only constraint)"
    );

    // ── SA6: JSON round-trip on QueryType ────────────────────────────
    //
    // Verifies serde(rename_all = "snake_case") matches the §6.2 row
    // labels and that the wire format is stable.
    for qt in [
        QueryType::UnderstandCode,
        QueryType::FindDefinition,
        QueryType::FindReferences,
        QueryType::SearchCode,
        QueryType::GetContextForEdit,
        QueryType::TraceDependencies,
        QueryType::BlastRadius,
        QueryType::ReviewChanges,
        QueryType::CheckQuality,
        QueryType::Remember,
    ] {
        let json = serde_json::to_string(&qt).expect("serialize QueryType");
        let back: QueryType = serde_json::from_str(&json).expect("deserialize QueryType");
        assert_eq!(
            qt, back,
            "(SA6) JSON round-trip mismatch for {qt:?}: serialised={json}"
        );
    }
    // Wire-format spot check — §6.2 row label.
    let wire = serde_json::to_string(&QueryType::FindDefinition).unwrap();
    assert_eq!(
        wire, "\"find_definition\"",
        "(SA6) FindDefinition wire-format must equal \"find_definition\" \
         (the §6.2 row label); got {wire}"
    );
}

// ── Frozen acceptance test — P3-W10-F10 conflict resolution ───────────────────
//
// Per `DEC-0007`, the frozen selector lives at MODULE ROOT so the
// `cargo test -p ucil-core fusion::test_conflict_resolution` selector
// substring-match resolves uniquely without traversing a `mod tests {}`
// barrier.  Sub-assertions are SA1..SA6 panic-tagged so failure
// messages map back to a specific assertion-of-record.

#[cfg(test)]
#[test]
#[allow(clippy::too_many_lines, clippy::float_cmp)]
fn test_conflict_resolution() {
    // ── SA1: empty input → Unresolvable {} with reason "empty input" ─
    let empty: Vec<ConflictCandidate<&'static str>> = Vec::new();
    let out = resolve_conflict(&empty);
    assert_eq!(
        out,
        ConflictResolution::Unresolvable {
            candidates: Vec::new(),
            reason: "empty input".to_owned(),
        },
        "(SA1) empty input must yield Unresolvable {{ candidates: [], \
         reason: \"empty input\" }}; left: {out:?}, right: \
         Unresolvable {{ candidates: [], reason: \"empty input\" }}"
    );

    // ── SA2: single-candidate input → Resolved with that candidate ──
    let single = ConflictCandidate {
        value: "a",
        source: SourceAuthority::Kg,
        confidence: 0.5,
    };
    let out = resolve_conflict(std::slice::from_ref(&single));
    assert_eq!(
        out,
        ConflictResolution::Resolved {
            value: "a",
            source: SourceAuthority::Kg,
            confidence: 0.5,
        },
        "(SA2) single-candidate input must yield Resolved with that \
         candidate's fields; left: {out:?}, right: Resolved {{ value: \
         \"a\", source: Kg, confidence: 0.5 }}"
    );

    // ── SA3: multi-tier non-conflict — LSP/AST wins over KG by authority ─
    //
    // LSP/AST candidate (high authority, high confidence) co-exists with
    // a KG candidate (lower authority, lower confidence) carrying a
    // different value — LSP/AST wins because it is the higher-authority
    // tier.  This SA pairs with the M1 mutation contract (inverting the
    // .min() → .max() reduction flips this assertion: a Text/KG winner
    // would surface, contradicting §18 line 1811).
    let candidates_sa3 = [
        ConflictCandidate {
            value: "a",
            source: SourceAuthority::LspAst,
            confidence: 0.9,
        },
        ConflictCandidate {
            value: "b",
            source: SourceAuthority::Kg,
            confidence: 0.6,
        },
    ];
    let out = resolve_conflict(&candidates_sa3);
    assert_eq!(
        out,
        ConflictResolution::Resolved {
            value: "a",
            source: SourceAuthority::LspAst,
            confidence: 0.9,
        },
        "(SA3) cross-tier authority precedence — LSP/AST beats KG by \
         §18 line 1811 hierarchy; left: {out:?}, right: Resolved {{ \
         value: \"a\", source: LspAst, confidence: 0.9 }}"
    );

    // ── SA4: tied top tier with disagreeing values → Unresolvable ───
    //
    // Two candidates both at the same top tier (LspAst) but carrying
    // different values — the deterministic resolver cannot pick a
    // winner; the slate is forwarded to the agent-mediator surface.
    let candidates_sa4 = [
        ConflictCandidate {
            value: "a",
            source: SourceAuthority::LspAst,
            confidence: 0.9,
        },
        ConflictCandidate {
            value: "b",
            source: SourceAuthority::LspAst,
            confidence: 0.85,
        },
    ];
    let out = resolve_conflict(&candidates_sa4);
    match &out {
        ConflictResolution::Unresolvable { candidates, reason } => {
            assert_eq!(
                candidates.len(),
                2,
                "(SA4) tied-top-tier Unresolvable must retain BOTH \
                 candidates; left: {} candidates, right: 2 candidates; \
                 outcome={out:?}",
                candidates.len(),
            );
            assert!(
                reason.contains("tied at tier"),
                "(SA4) Unresolvable.reason must contain \"tied at tier\" \
                 substring; left: {reason:?}, right: substring \"tied \
                 at tier\""
            );
        }
        ConflictResolution::Resolved { .. } => panic!(
            "(SA4) tied-top-tier disagreeing values must yield Unresolvable; \
             left: {out:?}, right: Unresolvable {{ candidates: [...], \
             reason: \"tied at tier LspAst\" }}"
        ),
    }

    // ── SA5: tied top tier with AGREEING values → Resolved (highest conf) ─
    //
    // Two candidates both at the same top tier (LspAst), carrying the
    // SAME value but different confidences — the deterministic resolver
    // picks the highest-confidence member of the tied-tier subset.
    let candidates_sa5 = [
        ConflictCandidate {
            value: "a",
            source: SourceAuthority::LspAst,
            confidence: 0.9,
        },
        ConflictCandidate {
            value: "a",
            source: SourceAuthority::LspAst,
            confidence: 0.92,
        },
    ];
    let out = resolve_conflict(&candidates_sa5);
    assert_eq!(
        out,
        ConflictResolution::Resolved {
            value: "a",
            source: SourceAuthority::LspAst,
            confidence: 0.92,
        },
        "(SA5) tied-top-tier agreeing values — highest confidence wins; \
         left: {out:?}, right: Resolved {{ value: \"a\", source: \
         LspAst, confidence: 0.92 }}"
    );

    // ── SA6: cross-tier dominance — authority beats raw confidence ──
    //
    // LSP/AST candidate at low confidence (0.5) BEATS a Text candidate
    // at very high confidence (0.99).  Authority precedence dominates
    // raw confidence per §18 line 1811 ("source authority as soft
    // guidance" — soft for the agent layer, hard for the deterministic
    // reducer).  This SA pairs with the M1 mutation contract: inverting
    // the source-tier ordering would surface the Text candidate.
    let candidates_sa6 = [
        ConflictCandidate {
            value: "a",
            source: SourceAuthority::LspAst,
            confidence: 0.5,
        },
        ConflictCandidate {
            value: "b",
            source: SourceAuthority::Text,
            confidence: 0.99,
        },
    ];
    let out = resolve_conflict(&candidates_sa6);
    assert_eq!(
        out,
        ConflictResolution::Resolved {
            value: "a",
            source: SourceAuthority::LspAst,
            confidence: 0.5,
        },
        "(SA6) cross-tier dominance — LspAst@0.5 beats Text@0.99; \
         left: {out:?}, right: Resolved {{ value: \"a\", source: \
         LspAst, confidence: 0.5 }} (authority dominates confidence \
         per §18 line 1811)"
    );
}
