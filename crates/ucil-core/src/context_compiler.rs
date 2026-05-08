//! Aider-style repo-map (reimplemented in Rust) — feature `P3-W10-F01`.
//!
//! Master-plan citations driving this module:
//!
//! * §1.1 line 44 — deep-fusion uses `PageRank` as one of the canonical
//!   ranking signals when deciding which symbols to surface to an agent.
//! * §3.5 — G5 Context group: Aider repo-map is the foundation tool the
//!   downstream Context source depends on.
//! * §4.5 line 345 — "Aider repo-map (reimplemented) — Library
//!   (internal, Rust), P0".
//! * §6.1 line 506 — "Aider-style repo-map (`PageRank`, 50x bias toward
//!   relevant files)".
//! * §6.3 line 660 — "Response assembly — full detail for relevant
//!   content; `PageRank`-driven ranking".
//! * §17.2 line 1634 — `context_compiler.rs` is the canonical filename
//!   in the `ucil-core/src/` layout listing.
//! * §18 Phase 3 Week 10 line 1808 — the deliverable that issues the
//!   solo-feature WO-0087.
//!
//! # Algorithm
//!
//! The repo-map runs a hand-rolled **personalized `PageRank`** over the
//! knowledge-graph's `kind = "calls"` directed-edge subgraph, biased
//! 50× toward symbols whose `file_path` matches a caller-supplied
//! `recently_queried_files` set.  The algorithm is sparse-iterative
//! (no `ndarray` / `nalgebra` / `petgraph` / `sprs` dep):
//!
//! 1. Read every `Entity` and every `Relation` with `kind == "calls"`
//!    out of the [`crate::KnowledgeGraph`] (a one-shot warm-up scan).
//! 2. Build a sparse adjacency `HashMap<entity_id, Vec<entity_id>>`
//!    from the `calls` edges (`source_id → [target_id, ...]`).
//! 3. Build a personalization vector `HashMap<entity_id, f64>` whose
//!    entries summing to 1.0 — entities whose `file_path` is in
//!    `recently_queried_files` get the `recency_bias_multiplier` factor
//!    (default `50.0` per §6.1 line 506) before renormalisation.
//! 4. Iterate the standard damped `PageRank` update rule
//!    `score_new[v] = (1-d) * pers[v] + d * Σ score_old[u]/out_deg(u)`
//!    for incoming `u`, with `damping = 0.85`, `tolerance = 1e-6`,
//!    and `max_iterations = 100` (the workspace-canonical defaults).
//!    Dangling nodes (entities with no outgoing `calls` edges)
//!    distribute their score uniformly across all nodes — the standard
//!    "teleport" treatment.
//! 5. Sort entities by descending score (tie-break by ascending
//!    `qualified_name` for determinism), estimate per-symbol token
//!    cost with a 4-char-per-token heuristic
//!    (`signature.len()/4 + doc_comment.len()/4 + qualified_name.len()/4 + 8`,
//!    the `cl100k_base` lower-bound used by Aider's original
//!    pre-tokenisation budget gate), and greedily fit a strict prefix
//!    that does not exceed `options.token_budget`.
//!
//! The token-counting heuristic is intentionally a structural budget;
//! callers may multiply by an external tokenizer ratio if they need
//! tiktoken-accurate counts.  CJK and dense-token sequences will
//! under-count under the 4-char heuristic — a future ADR may swap in
//! `tiktoken-rs` for production budgeting, but the WO-0087 acceptance
//! test does not depend on `cl100k_base` accuracy, only on the budget
//! being respected on the fixture.
//!
//! # Personalization vector edge cases
//!
//! * **Empty `recently_queried_files`**: every entity gets uniform
//!   `1/N` mass; `PageRank` reduces to the unbiased centrality ranking.
//! * **Every entity's file in `recently_queried_files`**: multiplying
//!   every entry by `50` then renormalising yields the uniform vector
//!   again — the bias is a relative signal, not an absolute one.
//! * **Path comparison**: strict `PathBuf` equality.  No string-suffix
//!   matching — a `recently_queried_files` containing `"src/foo.rs"`
//!   intentionally does NOT match an entity with
//!   `file_path = "vendor/dep/src/foo.rs"`.
//!
//! # Dependencies
//!
//! No new crate-level deps; the implementation uses only the workspace's
//! existing `chrono` / `thiserror` / `tracing` / `rusqlite` (transitively
//! via [`crate::KnowledgeGraph`]) plus `std::collections::HashMap`.
//! `ndarray` is in the workspace but unnecessary for the sparse case.
//!
//! # Hot-cold tier
//!
//! Master-plan §11 hot-cold tier does NOT apply — the repo-map is a
//! data-only algorithm + KG read; no cache surfaces; no warm-tier
//! promotion.  It runs on-demand from cold-tier data (§12.1 entities +
//! relations tables).  A future ADR may explore caching the `PageRank`
//! vector across queries; for now it is computed fresh per call.
//!
//! See `decisions/DEC-0007-remove-cargo-mutants-per-wo-gate.md` for the
//! frozen-test selector module-root placement requirement that puts
//! `test_repo_map_pagerank` at module root (NOT inside `mod tests`).

#![deny(rustdoc::broken_intra_doc_links)]

use crate::knowledge_graph::{Entity, KnowledgeGraphError};

// ── Errors ────────────────────────────────────────────────────────────────────

/// Errors produced by [`build_repo_map`].
///
/// Marked `#[non_exhaustive]` per `.claude/rules/rust-style.md §Errors`
/// so adding new failure modes (e.g. a future budget-overflow guard)
/// does not constitute a `SemVer` break.  Bridges
/// [`KnowledgeGraphError`] via `#[from]` so the entry point's `?`
/// operator can propagate KG read failures transparently.
///
/// [`build_repo_map`]: crate::context_compiler::build_repo_map
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RepoMapError {
    /// Read failure against the knowledge graph — table scan, bind, or
    /// row-iteration error from the `entities` / `relations` queries
    /// that warm up the `PageRank` input.
    #[error("knowledge graph access: {0}")]
    KnowledgeGraph(#[from] KnowledgeGraphError),

    /// The KG has no entities.  Returning a typed error rather than an
    /// empty [`RepoMap`] lets callers distinguish "the project has no
    /// symbols" (a configuration error) from "`PageRank` converged to a
    /// uniform vector with empty `calls` edges" (a real but vacuous
    /// result).
    #[error("empty graph: no `calls` relations and no entities")]
    EmptyGraph,
}

// ── Options ───────────────────────────────────────────────────────────────────

/// Tuning knobs for [`build_repo_map`].
///
/// The defaults reproduce the §6.1 + §6.3 canonical values:
///
/// * `damping = 0.85` — the standard `PageRank` damping factor.  Larger
///   values give heavier weight to the link-graph structure; smaller
///   values give heavier weight to the personalization (recency-bias)
///   vector.
/// * `max_iterations = 100` — convergence cap.  The 6-entity test
///   fixture converges in ~15 iterations; production graphs typically
///   converge under 50.
/// * `tolerance = 1e-6` — L1-norm convergence threshold per iteration.
/// * `recency_bias_multiplier = 50.0` — the §6.1 line 506 "50x bias
///   toward relevant files" constant.
/// * `token_budget = 8000` — the §6.3 default response-assembly
///   budget.  Caller-supplied; the value here is a sentinel default.
///
/// # Examples
///
/// ```
/// use ucil_core::RepoMapOptions;
///
/// let defaults = RepoMapOptions::default();
/// assert!((defaults.damping - 0.85).abs() < f64::EPSILON);
/// assert!((defaults.recency_bias_multiplier - 50.0).abs() < f64::EPSILON);
/// assert_eq!(defaults.max_iterations, 100);
/// assert_eq!(defaults.token_budget, 8000);
/// ```
///
/// [`build_repo_map`]: crate::context_compiler::build_repo_map
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RepoMapOptions {
    /// `PageRank` damping factor.  Master-plan §6.1 + standard `PageRank`
    /// literature: `0.85`.
    pub damping: f64,
    /// Iteration cap before declaring non-convergence.  Default `100`.
    pub max_iterations: usize,
    /// L1-norm convergence threshold.  Default `1e-6`.
    pub tolerance: f64,
    /// Multiplier applied to the personalization vector for entities
    /// whose `file_path` is in the caller-supplied
    /// `recently_queried_files` set.  Default `50.0` per master-plan
    /// §6.1 line 506.
    pub recency_bias_multiplier: f64,
    /// Maximum cumulative `token_estimate` across the returned
    /// [`RepoMap::symbols`] list.  The list is greedily prefix-fitted —
    /// the highest-ranked entry that still fits is the last one
    /// included; the first entry that would overshoot truncates the
    /// list.  Default `8000` per master-plan §6.3 response-assembly
    /// budget.
    pub token_budget: usize,
}

impl Default for RepoMapOptions {
    fn default() -> Self {
        Self {
            damping: 0.85,
            max_iterations: 100,
            tolerance: 1e-6,
            recency_bias_multiplier: 50.0,
            token_budget: 8000,
        }
    }
}

// ── Output rows ───────────────────────────────────────────────────────────────

/// One ranked entry in a [`RepoMap`] — an [`Entity`] paired with its
/// `PageRank` score and the budget-heuristic token estimate.
///
/// `score` is the converged `PageRank` scalar (sum across all entities
/// is 1.0 modulo dangling-node redistribution); `token_estimate` is
/// the integer count returned by [`entity_token_estimate`] which
/// drives the prefix-greedy budget fit.  `entity` is the §12.1
/// projection [`Entity`] verbatim — no fields stripped — so callers
/// rendering the repo-map line have everything ([`Entity::signature`],
/// [`Entity::doc_comment`], [`Entity::qualified_name`],
/// [`Entity::file_path`]) on hand.
#[derive(Debug, Clone, PartialEq)]
pub struct RankedSymbol {
    /// The §12.1 entities-row projection for this rank entry.
    pub entity: Entity,
    /// The converged personalized-`PageRank` score for this entity.
    /// Higher means more central / more recently-relevant.
    pub score: f64,
    /// 4-char-per-token lower-bound estimate of the rendered
    /// repo-map-line cost.  See [`entity_token_estimate`].
    pub token_estimate: usize,
}

/// The repo-map result: a budget-fitted prefix of the personalized-
/// `PageRank`-ranked symbol list, with iteration diagnostics for
/// observability.
///
/// `symbols` is sorted by `score` descending (stable tie-break on
/// `entity.qualified_name` ascending so the ordering is deterministic
/// across runs).  `total_tokens` is the cumulative
/// [`RankedSymbol::token_estimate`] across `symbols` (always
/// `<= options.token_budget`).  `iterations` is the actual loop count
/// when convergence was reached, OR `max_iterations` when it was not;
/// `converged` is the boolean for `iterations < max_iterations`.
#[derive(Debug, Clone, PartialEq)]
pub struct RepoMap {
    /// Ranked-and-budget-fitted symbol list.  Strict prefix of the
    /// full ranking up to the first entry that would exceed
    /// `options.token_budget`.
    pub symbols: Vec<RankedSymbol>,
    /// Cumulative `token_estimate` across `symbols`.  Always
    /// `<= options.token_budget`.
    pub total_tokens: usize,
    /// `PageRank` iteration count when the L1-norm tolerance was
    /// satisfied.  Equal to `options.max_iterations` if convergence
    /// was not reached.
    pub iterations: usize,
    /// Whether the `PageRank` iteration converged within
    /// `options.max_iterations`.
    pub converged: bool,
}

// ── Token-counting heuristic ──────────────────────────────────────────────────

/// Estimate the rendered token cost of an [`Entity`] in a repo-map
/// line.
///
/// Uses the 4-char-per-token lower-bound heuristic
/// (`cl100k_base` ≈ 4 chars / token for ASCII source) the same way
/// Aider's original repo-map uses it for budget-fitting before
/// invoking tiktoken.  Counts `chars()` (not `len()`) so multi-byte
/// source code does not over-count by byte-width.
///
/// The `+ 8` constant is a per-symbol overhead representing the
/// newlines, indentation, and `qualified_name` framing the renderer
/// will add when it emits the line.  Documented inline rather than
/// extracted to a const so the heuristic is greppable when a future
/// ADR swaps in tiktoken-rs.
#[must_use]
pub fn entity_token_estimate(entity: &Entity) -> usize {
    let signature = entity.signature.as_deref().unwrap_or("");
    let doc_comment = entity.doc_comment.as_deref().unwrap_or("");
    let qualified_name = entity.qualified_name.as_deref().unwrap_or(&entity.name);
    signature.chars().count() / 4
        + doc_comment.chars().count() / 4
        + qualified_name.chars().count() / 4
        + 8
}
