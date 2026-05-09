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

use std::collections::{HashMap, HashSet};
use std::hash::BuildHasher;
use std::path::PathBuf;

use crate::cross_group::{CrossGroupFusedHit, CrossGroupFusedOutcome};
use crate::knowledge_graph::{Entity, KnowledgeGraph, KnowledgeGraphError};

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

// ── PageRank kernel ───────────────────────────────────────────────────────────

/// Run personalized `PageRank` over a sparse directed-edge adjacency.
///
/// Pure-deterministic kernel (no IO, no KG dependency, no async).
/// Takes:
///
/// * `adjacency` — `node_id → Vec<outgoing_neighbor_ids>`.  Every node
///   in the universe must appear as a key, even if its `Vec` is empty
///   (dangling sink).  The personalization vector defines the universe;
///   nodes referenced only as `target_id`s in some other node's
///   outgoing list are folded in by the caller.
/// * `personalization` — `node_id → mass`.  Should sum to `1.0`;
///   renormalized internally if not (so the caller can pass un-
///   normalized 50× weights and let the kernel handle the rescale).
///   Every node must appear as a key, even if its mass is `0.0`.
/// * `options` — provides `damping`, `max_iterations`, `tolerance`.
///
/// Returns `(score_map, iterations, converged)`.
///
/// # Update rule
///
/// ```text
/// score_new[v] = (1 - d) * pers[v]
///              + d * (Σ_{u ∈ in(v)} score_old[u] / out_degree(u))
///              + d * dangling_mass / N
/// ```
///
/// where `dangling_mass = Σ_{u with out_degree(u) == 0} score_old[u]`
/// and `N` is the node count.  The dangling-redistribution term is the
/// standard "teleport" treatment that keeps `Σ score = 1.0` invariant
/// across iterations even when sinks exist (without it, score
/// monotonically leaks to zero).
///
/// # Convergence
///
/// `Σ_v |score_new[v] - score_old[v]| < tolerance` — the L1 norm of
/// the per-iteration delta.  When the norm drops below `tolerance`,
/// the kernel returns `(score_new, iter+1, true)`.  When the
/// `max_iterations` cap is reached, it returns
/// `(last_score, max_iterations, false)`.
///
/// # Initial vector
///
/// Initialised to `1/N` for every node (the uniform distribution).
/// Early iterations rapidly converge toward the personalized
/// equilibrium under standard `damping = 0.85`.
#[tracing::instrument(
    level = "debug",
    skip(adjacency, personalization),
    fields(num_nodes = adjacency.len(), max_iter = options.max_iterations),
    name = "ucil.core.context_compiler.page_rank",
)]
#[must_use]
pub fn personalized_page_rank<A, P>(
    adjacency: &HashMap<i64, Vec<i64>, A>,
    personalization: &HashMap<i64, f64, P>,
    options: &RepoMapOptions,
) -> (HashMap<i64, f64>, usize, bool)
where
    A: BuildHasher,
    P: BuildHasher,
{
    let n = adjacency.len();
    if n == 0 {
        return (HashMap::new(), 0, true);
    }

    // ── Renormalise personalization ────────────────────────────────
    //
    // The caller may pass an un-normalised vector (50× weights on the
    // recent files, 1× elsewhere); rescale to sum to 1.0 here so the
    // update rule is closed-form.  Empty / zero-sum personalization
    // (every entry zero) falls back to the uniform 1/N distribution
    // — the same equilibrium PageRank converges to under no bias.
    #[allow(clippy::cast_precision_loss)]
    let n_f64 = n as f64;
    let pers_sum: f64 = personalization.values().sum();
    let pers: HashMap<i64, f64> = if pers_sum <= 0.0 {
        adjacency.keys().map(|&k| (k, 1.0 / n_f64)).collect()
    } else {
        adjacency
            .keys()
            .map(|&k| {
                let raw = personalization.get(&k).copied().unwrap_or(0.0);
                (k, raw / pers_sum)
            })
            .collect()
    };

    // ── Initial uniform score vector ───────────────────────────────
    let mut score: HashMap<i64, f64> = adjacency.keys().map(|&k| (k, 1.0 / n_f64)).collect();

    // ── Iteration loop ─────────────────────────────────────────────
    //
    // Standard damped PageRank update with explicit dangling-mass
    // redistribution.  We compute incoming contribution per source
    // first (one pass over the adjacency), then add the (1-d)*pers[v]
    // base mass and the dangling-redistribution term per node.
    let mut iterations = 0usize;
    let mut converged = false;
    for _ in 0..options.max_iterations {
        iterations += 1;

        // Compute the dangling-mass total: Σ score[u] for u with no
        // outgoing edges.  These nodes' score must be teleported per
        // iteration to keep Σ score = 1.0 invariant.
        let dangling_mass: f64 = adjacency
            .iter()
            .filter_map(|(node, out)| {
                if out.is_empty() {
                    score.get(node).copied()
                } else {
                    None
                }
            })
            .sum();

        // Push contribution from each source to each of its targets.
        // The `incoming[v]` accumulator builds Σ_{u in in(v)}
        // score_old[u] / out_degree(u).
        let mut incoming: HashMap<i64, f64> = adjacency.keys().map(|&k| (k, 0.0)).collect();
        for (&u, out_neighbors) in adjacency {
            if out_neighbors.is_empty() {
                continue;
            }
            #[allow(clippy::cast_precision_loss)]
            let out_deg = out_neighbors.len() as f64;
            let contrib = score.get(&u).copied().unwrap_or(0.0) / out_deg;
            for &v in out_neighbors {
                if let Some(slot) = incoming.get_mut(&v) {
                    *slot += contrib;
                }
            }
        }

        // Apply update rule + accumulate the L1-norm delta.
        let mut new_score: HashMap<i64, f64> = HashMap::with_capacity(n);
        let mut delta: f64 = 0.0;
        for (&v, &incoming_sum) in &incoming {
            let pers_v = pers.get(&v).copied().unwrap_or(0.0);
            let dangling_share = options.damping * dangling_mass / n_f64;
            // Equivalent to `(1.0 - options.damping) * pers_v + options.damping
            // * incoming_sum + dangling_share`; mul_add fused for the
            // first two terms per `clippy::suboptimal_flops`.
            let new_v = (1.0 - options.damping).mul_add(pers_v, options.damping * incoming_sum)
                + dangling_share;
            let old_v = score.get(&v).copied().unwrap_or(0.0);
            delta += (new_v - old_v).abs();
            new_score.insert(v, new_v);
        }

        score = new_score;

        if delta < options.tolerance {
            converged = true;
            break;
        }
    }

    (score, iterations, converged)
}

// ── Budget fitting ────────────────────────────────────────────────────────────

/// Greedy prefix-fit: take ranked entries in order, accumulating their
/// `token_estimate`, and stop at the first entry that would push the
/// running total past `token_budget`.
///
/// Pure-deterministic helper.  Returns `(fitted_prefix, total_tokens)`.
/// When the input is empty OR the first entry already exceeds the
/// budget, returns `(empty_vec, 0)`.  Master-plan §6.3 ("Response
/// assembly — full detail for relevant content; `PageRank`-driven
/// ranking") calls for "fits symbol list into a configurable token
/// budget" — this is the standard Aider repo-map prefix-greedy
/// semantic at top-of-rank.  NOT knapsack — knapsack would reorder
/// or skip mid-rank entries, which violates the Aider semantic and
/// the WO-0087 acceptance test's strict-prefix assertion.
fn fit_to_budget(ranked: Vec<RankedSymbol>, token_budget: usize) -> (Vec<RankedSymbol>, usize) {
    let mut total = 0usize;
    let mut fitted: Vec<RankedSymbol> = Vec::with_capacity(ranked.len());
    for entry in ranked {
        let next_total = total.saturating_add(entry.token_estimate);
        if next_total > token_budget {
            break;
        }
        total = next_total;
        fitted.push(entry);
    }
    (fitted, total)
}

// ── Public entry point ───────────────────────────────────────────────────────

/// Build a budget-fitted, recency-biased repo-map over the given KG.
///
/// Reads every entity and every `kind = "calls"` relation out of the
/// supplied [`KnowledgeGraph`], builds the sparse adjacency, constructs
/// the personalization vector with 50× bias on entities whose
/// `file_path` is in `recently_queried_files`, runs personalized
/// `PageRank`, sorts by descending score (tie-break on ascending
/// `qualified_name` for determinism), and returns the prefix that
/// fits in `options.token_budget`.
///
/// # Personalization vector
///
/// For each [`Entity`] `e`, let `path = PathBuf::from(&e.file_path)`.
/// If `recently_queried_files.contains(&path)`, the entity's raw
/// personalization mass is `recency_bias_multiplier / N`; otherwise
/// it is `1 / N` (where `N` is the entity count).  The raw vector is
/// then renormalised to sum to `1.0` inside [`personalized_page_rank`].
///
/// Path comparison is strict `PathBuf` equality — no string-suffix
/// match.  A `recently_queried_files` containing `"src/foo.rs"` does
/// NOT match an entity with `file_path = "vendor/dep/src/foo.rs"`.
///
/// # Errors
///
/// * [`RepoMapError::KnowledgeGraph`] — KG table-scan or row-iteration
///   failure (typically a `SQLite` error from
///   [`KnowledgeGraph::list_all_entities`] or
///   [`KnowledgeGraph::list_all_calls_relations`]).
/// * [`RepoMapError::EmptyGraph`] — the KG returned zero entities.
///   When entities exist but no `calls` edges do, the function still
///   succeeds and returns the unbiased uniform-`PageRank` ranking
///   (every entity has equal score).
pub fn build_repo_map<S: BuildHasher>(
    kg: &KnowledgeGraph,
    recently_queried_files: &HashSet<PathBuf, S>,
    options: &RepoMapOptions,
) -> Result<RepoMap, RepoMapError> {
    // ── Read the §12.1 entities + calls-relations slices ───────────
    let entities: Vec<Entity> = kg.list_all_entities()?;
    if entities.is_empty() {
        return Err(RepoMapError::EmptyGraph);
    }
    let relations = kg.list_all_calls_relations()?;

    // ── Build sparse adjacency `source_id -> [target_id]` ──────────
    //
    // Every entity in the universe must appear as a key in `adjacency`
    // — even when it has no outgoing `calls` edges (it becomes a
    // dangling sink under the iteration's "teleport" treatment).  The
    // initialisation pass seeds every key with an empty `Vec`; the
    // edge-walk then pushes targets onto the source's list.
    let mut adjacency: HashMap<i64, Vec<i64>> = HashMap::with_capacity(entities.len());
    for e in &entities {
        if let Some(id) = e.id {
            adjacency.entry(id).or_default();
        }
    }
    for rel in &relations {
        // Skip self-loops and edges referencing entities we did not
        // ingest above (foreign-key consistency is enforced at the
        // schema level via `REFERENCES entities(id)`, but defensive
        // guards cost nothing and protect against post-test fixtures
        // with manually-deleted rows).
        if rel.source_id == rel.target_id {
            continue;
        }
        if !adjacency.contains_key(&rel.source_id) || !adjacency.contains_key(&rel.target_id) {
            continue;
        }
        adjacency
            .entry(rel.source_id)
            .or_default()
            .push(rel.target_id);
    }

    // ── Personalization vector with 50× recency bias ───────────────
    //
    // master-plan §6.1 line 506: "PageRank, 50x bias toward relevant
    // files".  For each entity, base mass is `1 / N`; when the
    // entity's file is in `recently_queried_files`, the mass is
    // multiplied by `options.recency_bias_multiplier` (default 50.0).
    // The kernel renormalises to Σ pers = 1.0 internally.
    #[allow(clippy::cast_precision_loss)]
    let n_f64 = entities.len() as f64;
    let uniform_mass = 1.0 / n_f64;
    let mut personalization: HashMap<i64, f64> = HashMap::with_capacity(entities.len());
    for e in &entities {
        let Some(id) = e.id else {
            continue;
        };
        let path = PathBuf::from(&e.file_path);
        let mass = if recently_queried_files.contains(&path) {
            options.recency_bias_multiplier * uniform_mass
        } else {
            uniform_mass
        };
        personalization.insert(id, mass);
    }

    // ── Run the kernel ──────────────────────────────────────────────
    let (scores, iterations, converged) =
        personalized_page_rank(&adjacency, &personalization, options);

    // ── Sort entities by descending score, tie-break on qualified_name
    //   ascending (deterministic) ──────────────────────────────────
    let mut ranked: Vec<RankedSymbol> = entities
        .into_iter()
        .filter_map(|entity| {
            let id = entity.id?;
            let score = scores.get(&id).copied().unwrap_or(0.0);
            let token_estimate = entity_token_estimate(&entity);
            Some(RankedSymbol {
                entity,
                score,
                token_estimate,
            })
        })
        .collect();
    ranked.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                let a_qn = a.entity.qualified_name.as_deref().unwrap_or(&a.entity.name);
                let b_qn = b.entity.qualified_name.as_deref().unwrap_or(&b.entity.name);
                a_qn.cmp(b_qn)
            })
    });

    // ── Greedy prefix-fit to the token budget ───────────────────────
    let (symbols, total_tokens) = fit_to_budget(ranked, options.token_budget);

    Ok(RepoMap {
        symbols,
        total_tokens,
        iterations,
        converged,
    })
}

// ── Module-root test (DEC-0007 frozen-selector placement) ─────────────────────
//
// `test_repo_map_pagerank` lives at module root — NOT inside
// `mod tests { ... }` — so the substring selector
// `cargo test -p ucil-core context_compiler::test_repo_map_pagerank`
// resolves uniquely without `--exact`.  Per `DEC-0007` +
// `WO-0070` lessons: the `tests::` infix added by the conventional
// `mod tests` wrapper would break the selector resolution gate
// (`! grep -cE 'test_repo_map_pagerank: test'` returning anything other
// than 1 fails AC15).  The `#[cfg(test)]` attribute keeps the test
// out of release builds.

/// Build an [`Entity`] for the SA1/SA2/SA3 fixture.
#[cfg(test)]
fn make_test_entity(name: &str, qname: &str, file: &str) -> Entity {
    Entity {
        id: None,
        kind: "function".to_owned(),
        name: name.to_owned(),
        qualified_name: Some(qname.to_owned()),
        file_path: file.to_owned(),
        start_line: Some(1),
        end_line: Some(10),
        signature: Some(format!("fn {name}()")),
        doc_comment: None,
        language: Some("rust".to_owned()),
        t_valid_from: Some("2026-05-09T00:00:00+00:00".to_owned()),
        t_valid_to: None,
        importance: 0.5,
        source_tool: Some("tree-sitter".to_owned()),
        source_hash: None,
    }
}

/// Build a `kind = "calls"` [`crate::knowledge_graph::Relation`] for
/// the SA1/SA2/SA3 fixture.
#[cfg(test)]
fn make_test_call_relation(source_id: i64, target_id: i64) -> crate::knowledge_graph::Relation {
    crate::knowledge_graph::Relation {
        id: None,
        source_id,
        target_id,
        kind: "calls".to_owned(),
        weight: 1.0,
        t_valid_from: Some("2026-05-09T00:00:00+00:00".to_owned()),
        t_valid_to: None,
        source_tool: Some("tree-sitter".to_owned()),
        source_evidence: None,
        confidence: 1.0,
    }
}

/// Seed an isolated [`KnowledgeGraph`] with the 6-entity DAG used by
/// SA1/SA2/SA3.  Returns the KG plus the [`tempfile::TempDir`] that
/// owns the underlying `knowledge.db` (tempdir cleanup is `Drop`-bound;
/// the caller must hold both for the lifetime of the test).
#[cfg(test)]
fn seed_repo_map_test_kg() -> (KnowledgeGraph, tempfile::TempDir) {
    let tmp = tempfile::TempDir::new().expect("tempdir must be creatable");
    let mut kg = KnowledgeGraph::open(&tmp.path().join("knowledge.db"))
        .expect("KnowledgeGraph::open must succeed");

    let id_root = kg
        .upsert_entity(&make_test_entity("root", "a::root", "src/file_a.rs"))
        .expect("a::root insert");
    let id_child1 = kg
        .upsert_entity(&make_test_entity("child1", "a::child1", "src/file_a.rs"))
        .expect("a::child1 insert");
    let id_child2 = kg
        .upsert_entity(&make_test_entity("child2", "a::child2", "src/file_a.rs"))
        .expect("a::child2 insert");
    let id_handler = kg
        .upsert_entity(&make_test_entity("handler", "b::handler", "src/file_b.rs"))
        .expect("b::handler insert");
    let id_helper = kg
        .upsert_entity(&make_test_entity("helper", "b::helper", "src/file_b.rs"))
        .expect("b::helper insert");
    let id_leaf = kg
        .upsert_entity(&make_test_entity("leaf", "c::leaf", "src/file_c.rs"))
        .expect("c::leaf insert");

    // Topology rationale (per RCA `verification-reports/root-cause-WO-0087.md`):
    // the planner's original edge set in scope_in #8 made `c::leaf` (a chain
    // sink) dominate the equilibrium, not `b::handler`.  This revised set —
    // 6 entities + 6 `calls` relations + 3 files preserved — has been
    // analytically and empirically verified to give:
    //   * unbiased PageRank winner: `b::handler` (2 incoming: helper, leaf) → SA1
    //   * 50× src/file_a.rs bias winner: `a::child1` (file_a entity)         → SA2
    //   * token-budget=30 truncates the 6-symbol list to a strict prefix    → SA3
    // The mutation contract (M1/M2/M3) surfaces SA2/SA3/SA1 panics respectively.
    for rel in [
        make_test_call_relation(id_root, id_child1), // file_a internal
        make_test_call_relation(id_root, id_child2), // file_a internal
        make_test_call_relation(id_helper, id_handler), // file_b internal (1st handler-incoming)
        make_test_call_relation(id_leaf, id_handler), // file_c → file_b (2nd handler-incoming)
        make_test_call_relation(id_helper, id_leaf), // file_b → file_c
        make_test_call_relation(id_helper, id_child1), // file_b → file_a (gives child1 a feeder)
    ] {
        kg.upsert_relation(&rel).expect("relation insert");
    }

    (kg, tmp)
}

/// Seed an isolated KG with a 6-entity directed-acyclic call graph and
/// assert structural-`PageRank` ranking, 50× recency-bias inversion,
/// and token-budget fit per WO-0087 SA1/SA2/SA3 contract.
///
/// # Graph topology (revised per RCA `verification-reports/root-cause-WO-0087.md`)
///
/// ```text
/// a::root    --calls-->  a::child1, a::child2  (file_a internal)
/// b::helper  --calls-->  b::handler            (1st handler-incoming, file_b internal)
/// b::helper  --calls-->  c::leaf               (file_b → file_c)
/// b::helper  --calls-->  a::child1             (file_b → file_a; gives child1 a feeder)
/// c::leaf    --calls-->  b::handler            (2nd handler-incoming, file_c → file_b)
/// ```
///
/// Files:
///   * `src/file_a.rs` → `a::root`, `a::child1`, `a::child2`
///   * `src/file_b.rs` → `b::handler`, `b::helper`
///   * `src/file_c.rs` → `c::leaf`
///
/// Why this topology (not the planner's original — see RCA hypothesis #1):
/// the planner's original DAG (`root → child{1,2} → handler → helper → leaf`)
/// concentrated equilibrium `PageRank` at the chain sink `c::leaf`, not at
/// `b::handler`, breaking SA1.  This revised set keeps the spec-frozen
/// invariants (6 entities + 6+ `calls` relations + 3 files), routes
/// `b::handler` as the 2-incoming hub (helper, leaf), and uses
/// `helper → child1` to give `a::child1` a non-bleeding feeder so the
/// 50× `file_a` bias surfaces an `a::*` symbol on top.
///
/// Under unbiased `PageRank` (analytical equilibrium): `b::handler`
/// 0.270 > `a::child1` 0.194 > `a::child2` 0.162 > `c::leaf` 0.146 >
/// `a::root` 0.114 = `b::helper` 0.114 (tie-break: `a::root` < `b::helper`
/// alphabetical).
///
/// Under 50× bias on `src/file_a.rs` (analytical equilibrium):
/// `a::child1` 0.227 > `b::handler` 0.220 > `a::child2` 0.201 > … —
/// top `file_path` is `src/file_a.rs`.
///
/// # SA tags (mutation-targeted)
///
/// * SA1 — structural-`PageRank` winner is `b::handler` (M3 target).
/// * SA2 — recency-bias on `src/file_a.rs` flips top to a `file_a`
///   entity (M1 target).
/// * SA3 — token-budget = 30 truncates the 6-symbol list to a strict
///   prefix (M2 target — three sub-assertions).
#[cfg(test)]
#[test]
fn test_repo_map_pagerank() {
    let (kg, _tmpdir_guard) = seed_repo_map_test_kg();

    // ── SA1 — structural-PageRank winner is b::handler ──────────────
    //
    // Empty recency set → uniform personalization → unbiased PageRank.
    // The 6-entity DAG centres mass on `b::handler` (2 incoming + 1
    // outgoing, in the middle of the chain).  Token budget large
    // enough to admit every symbol (no truncation).
    let large_budget = RepoMapOptions {
        token_budget: 100_000,
        ..RepoMapOptions::default()
    };
    let empty_recent: HashSet<PathBuf> = HashSet::new();
    let unbudgeted = build_repo_map(&kg, &empty_recent, &large_budget)
        .expect("SA1 build_repo_map must succeed on the seeded KG");
    assert!(
        unbudgeted.converged,
        "(SA1) PageRank must converge within max_iterations on the 6-entity fixture; iterations={}",
        unbudgeted.iterations,
    );
    assert_eq!(
        unbudgeted.symbols.len(),
        6,
        "(SA1) large-budget run must include all 6 symbols; got {}",
        unbudgeted.symbols.len(),
    );
    let top_qn = unbudgeted.symbols[0]
        .entity
        .qualified_name
        .as_deref()
        .unwrap_or("<no qualified_name>");
    assert_eq!(
        top_qn, "b::handler",
        "(SA1) structural pagerank winner expected b::handler; observed {top_qn:?}",
    );

    // ── SA2 — 50× recency bias on src/file_a.rs flips ranking ───────
    //
    // The personalization 50× multiplier on the three file_a entities
    // pushes one of `a::root` / `a::child1` / `a::child2` above
    // `b::handler`.  We assert the top-ranked symbol's file_path is
    // `src/file_a.rs` regardless of which file_a symbol wins (the
    // ordering among the three is implementation-stable but not
    // contract-frozen).
    let mut recent: HashSet<PathBuf> = HashSet::new();
    recent.insert(PathBuf::from("src/file_a.rs"));
    let biased = build_repo_map(&kg, &recent, &large_budget)
        .expect("SA2 build_repo_map must succeed on the seeded KG");
    let top_fp = &biased.symbols[0].entity.file_path;
    let top_qn_biased = biased.symbols[0]
        .entity
        .qualified_name
        .as_deref()
        .unwrap_or("<no qualified_name>");
    assert_eq!(
        top_fp, "src/file_a.rs",
        "(SA2) recency-bias top symbol expected file_a.rs; \
         observed file_path={top_fp:?}, qualified_name={top_qn_biased:?}",
    );

    // ── SA3 — token-budget=30 truncates to a strict prefix ──────────
    //
    // The 6-entity ranking is the unbudgeted run above; the budgeted
    // run with token_budget=30 must:
    //   (a) total_tokens <= 30 (budget honored)
    //   (b) symbols.len() < 6 (truncation occurred)
    //   (c) returned symbols are a strict prefix of the unbudgeted
    //       ranking (NOT a knapsack selection).
    let small_budget = RepoMapOptions {
        token_budget: 30,
        ..RepoMapOptions::default()
    };
    let budgeted = build_repo_map(&kg, &empty_recent, &small_budget)
        .expect("SA3 build_repo_map must succeed on the seeded KG");
    let total = budgeted.total_tokens;
    assert!(
        total <= 30,
        "(SA3a) token budget exceeded: total_tokens={total} > 30",
    );
    let n_budgeted = budgeted.symbols.len();
    assert!(
        n_budgeted < 6,
        "(SA3b) budget did not truncate: returned all {n_budgeted} symbols",
    );
    for (i, budgeted_sym) in budgeted.symbols.iter().enumerate() {
        let budgeted_qn = budgeted_sym
            .entity
            .qualified_name
            .as_deref()
            .unwrap_or("<no qualified_name>");
        let unbudgeted_qn = unbudgeted.symbols[i]
            .entity
            .qualified_name
            .as_deref()
            .unwrap_or("<no qualified_name>");
        assert_eq!(
            budgeted_qn, unbudgeted_qn,
            "(SA3c) budgeted prefix mismatch at index {i}: \
             symbol[i].qualified_name={budgeted_qn:?} != \
             unbudgeted[i].qualified_name={unbudgeted_qn:?}",
        );
    }
}

// ── Response assembly (P3-W10-F09) ───────────────────────────────────────────
//
// Master-plan §6.3 lines 660-690 — Response assembly applies session-
// dedup + relevance threshold + token annotation over a
// `CrossGroupFusedOutcome` to produce the final response shape that
// the host adapter renders.  The §6.3 contract drives the algorithm:
//
//   1. "Skip files already in the agent's context" — session dedup
//      against a caller-supplied `files_in_context` set (master-plan
//      §6.3 line 663).
//   2. "Skip if relevance score < 0.1" — strict-`<` relevance
//      threshold (master-plan §6.3 line 667).
//   3. "Annotate with token cost" — 4-char-per-token lower-bound
//      heuristic per surviving hit (master-plan §6.3 line 700-701
//      `_meta.token_count`).
//
// The function chains directly into `crate::bonus_selector::select_bonus_context`
// when the caller wants the full §6.3 line 670 response shape — F09's
// surviving hits are F11's bonus-recipients exactly when the two
// thresholds are aligned (the default 0.1).

/// Per-response counters returned alongside the surviving hits.
///
/// Master-plan §6.3 line 700-701: "Format: Markdown with file paths
/// as headers, full code blocks ... Add `_meta.token_count` for
/// host-adapter trimming."  These four counters compose that
/// `_meta` block.
///
/// # Invariants
///
/// * `total_input == hits-surviving + deduped_count + filtered_count`
///   (the conservation invariant — every input hit ends up in
///   exactly one of the three buckets).
/// * `token_count` is the sum of [`hit_token_estimate`] across
///   surviving hits; never negative.
///
/// NOT marked `#[non_exhaustive]` — this is a stable response-shape
/// contract per master-plan §6.3 line 700-701 (the host adapter
/// reads `_meta.token_count` to drive trimming).
///
/// # Examples
///
/// ```
/// use ucil_core::ResponseMeta;
///
/// let meta = ResponseMeta {
///     token_count: 42,
///     deduped_count: 1,
///     filtered_count: 2,
///     total_input: 5,
/// };
/// // Conservation invariant — 5 inputs minus 1 dedup minus 2
/// // filter equals 2 surviving.
/// let surviving = meta.total_input - meta.deduped_count - meta.filtered_count;
/// assert_eq!(surviving, 2);
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResponseMeta {
    /// Total token estimate across surviving hits (the
    /// 4-char-per-token lower-bound heuristic).  Master-plan §6.3
    /// line 700-701 `_meta.token_count` for host-adapter trimming.
    pub token_count: usize,
    /// Number of hits removed because their `file_path` was in the
    /// caller-supplied `files_in_context` set.  Master-plan §6.3
    /// line 663 session-dedup.
    pub deduped_count: usize,
    /// Number of hits removed because their `fused_score` was
    /// strictly less than `options.relevance_threshold`.  Master-
    /// plan §6.3 line 667 ("Skip if relevance score < 0.1").
    pub filtered_count: usize,
    /// Input outcome's hit count BEFORE any filtering.
    pub total_input: usize,
}

/// The §6.3 final response shape — surviving hits paired with
/// projection-counter metadata.
///
/// `hits` preserves the input outcome's order
/// ([`CrossGroupFusedOutcome::hits`] is sorted descending by
/// `fused_score` per WO-0068's §6.2 contract; this projection ONLY
/// filters, never re-sorts).  Length invariant:
/// `hits.len() == meta.total_input - meta.deduped_count - meta.filtered_count`.
///
/// NOT marked `#[non_exhaustive]` — this is a stable response-shape
/// contract per master-plan §6.3 line 700-701.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AssembledResponse {
    /// Surviving hits, in input order (descending `fused_score`).
    pub hits: Vec<CrossGroupFusedHit>,
    /// Per-response counters (token-count, dedup-count,
    /// filter-count, total-input).
    pub meta: ResponseMeta,
}

/// Tuning knobs for [`assemble_response`].
///
/// The defaults reproduce the master-plan §6.3 line 663-668
/// canonical values:
///
/// * `relevance_threshold = 0.1` — strict-`<` filter floor; a hit
///   with `fused_score == 0.1` IS surviving (boundary inclusion).
///   Master-plan §6.3 line 667 verbatim ("Skip if relevance score
///   < 0.1").
/// * `dedup_enabled = true` — apply session dedup against
///   `files_in_context`.  Disable to retain every hit regardless of
///   the caller's session state (only useful for debug / tracing
///   workflows).
///
/// # Examples
///
/// ```
/// use ucil_core::ResponseAssemblyOptions;
///
/// let defaults = ResponseAssemblyOptions::default();
/// assert!((defaults.relevance_threshold - 0.1).abs() < f64::EPSILON);
/// assert!(defaults.dedup_enabled);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResponseAssemblyOptions {
    /// Strict-`<` filter floor.  Hits with `fused_score < threshold`
    /// are filtered out; hits with `fused_score == threshold` ARE
    /// surviving.  Master-plan §6.3 line 667 default `0.1`.
    pub relevance_threshold: f64,
    /// Whether to apply session-dedup against the caller-supplied
    /// `files_in_context` set.  Default `true`.
    pub dedup_enabled: bool,
}

impl Default for ResponseAssemblyOptions {
    fn default() -> Self {
        Self {
            relevance_threshold: 0.1,
            dedup_enabled: true,
        }
    }
}

/// Estimate the rendered token cost of a [`CrossGroupFusedHit`] in
/// the §6.3 response shape.
///
/// Mirrors [`entity_token_estimate`] — the 4-char-per-token
/// lower-bound heuristic (`cl100k_base` ≈ 4 chars/token for ASCII
/// source) used by Aider's original repo-map for budget-fitting
/// before invoking tiktoken.  Counts `chars()` (not `len()`) so
/// multi-byte source code does not over-count by byte-width.
///
/// The `+ 8` constant is the per-hit Markdown-rendering overhead
/// (file-path header + line-range annotation + code-block
/// delimiters per master-plan §6.3 line 700 "Format: Markdown with
/// file paths as headers, full code blocks").  Documented inline
/// rather than extracted to a const so the heuristic is greppable
/// when a future ADR swaps in `tiktoken-rs`.
///
/// See also: [`entity_token_estimate`].
#[must_use]
pub fn hit_token_estimate(hit: &CrossGroupFusedHit) -> usize {
    hit.snippet.chars().count() / 4 + hit.file_path.to_string_lossy().chars().count() / 4 + 8
}

/// Assemble the §6.3 final response — apply session-dedup +
/// relevance threshold + token annotation to a fused outcome.
///
/// Pure-deterministic projection; never panics; never returns
/// `Result`.  Empty input → empty output.  Order of input is
/// preserved (the caller's [`CrossGroupFusedOutcome::hits`] is
/// already sorted descending by `fused_score` per the WO-0068
/// §6.2 contract; this projection ONLY filters, never re-sorts).
///
/// §15.2 tracing carve-out applies (pure-deterministic projection)
/// — production impls of the daemon-side response-assembly
/// orchestration carry `tracing::instrument` at the boundary.
///
/// # Algorithm — master-plan §6.3 lines 665-685
///
/// 1. Capture `total_input = outcome.hits.len()`.
/// 2. If `options.dedup_enabled`, partition hits into
///    `(kept, deduped)` where a hit is `deduped` IFF
///    `files_in_context.contains(&hit.file_path)` (strict
///    `HashSet`-on-`PathBuf` comparison; no string-suffix match,
///    no canonicalization).  Otherwise `kept = all input`.
/// 3. Partition `kept` into `(surviving, filtered)` where a hit is
///    `filtered` IFF `hit.fused_score < options.relevance_threshold`
///    (strict-`<` per master-plan §6.3 line 667 — boundary
///    inclusion: `0.1 < 0.1` is false so the hit IS surviving).
/// 4. Compute `token_count = Σ hit_token_estimate(h) for h in surviving`.
/// 5. Construct + return [`AssembledResponse`].
///
/// # Examples
///
/// ```
/// use std::collections::HashSet;
/// use std::path::PathBuf;
/// use ucil_core::{
///     CrossGroupFusedOutcome, ResponseAssemblyOptions, assemble_response,
/// };
///
/// let outcome = CrossGroupFusedOutcome::default();
/// let files: HashSet<PathBuf> = HashSet::new();
/// let options = ResponseAssemblyOptions::default();
/// let result = assemble_response(&outcome, &files, &options);
/// assert_eq!(result.meta.total_input, 0);
/// assert!(result.hits.is_empty());
/// ```
#[must_use]
pub fn assemble_response<S: BuildHasher>(
    outcome: &CrossGroupFusedOutcome,
    files_in_context: &HashSet<PathBuf, S>,
    options: &ResponseAssemblyOptions,
) -> AssembledResponse {
    let total_input = outcome.hits.len();

    // ── Step 2 — session dedup ─────────────────────────────────
    //
    // Strict `HashSet::contains` on `PathBuf` — no string-suffix
    // matching, no canonicalization.  When `dedup_enabled` is
    // false we skip this partition entirely (deduped_count = 0).
    let mut deduped_count: usize = 0;
    let kept: Vec<&CrossGroupFusedHit> = outcome
        .hits
        .iter()
        .filter(|hit| {
            if options.dedup_enabled && files_in_context.contains(&hit.file_path) {
                deduped_count += 1;
                false
            } else {
                true
            }
        })
        .collect();

    // ── Step 3 — strict-`<` relevance threshold ────────────────
    //
    // Boundary inclusion: a hit with `fused_score == relevance_threshold`
    // IS surviving (`0.1 < 0.1` is false).
    let mut filtered_count: usize = 0;
    let surviving: Vec<CrossGroupFusedHit> = kept
        .into_iter()
        .filter(|hit| {
            if hit.fused_score < options.relevance_threshold {
                filtered_count += 1;
                false
            } else {
                true
            }
        })
        .cloned()
        .collect();

    // ── Step 4 — token annotation ──────────────────────────────
    let token_count: usize = surviving.iter().map(hit_token_estimate).sum();

    AssembledResponse {
        hits: surviving,
        meta: ResponseMeta {
            token_count,
            deduped_count,
            filtered_count,
            total_input,
        },
    }
}

// ── Module-root test for assemble_response (DEC-0007 frozen-selector) ─────────
//
// `test_response_assembly` lives at module root — NOT inside
// `mod tests { ... }` — so the substring selector
// `cargo test -p ucil-core context_compiler::test_response_assembly`
// resolves uniquely without `--exact`.

/// Build a [`CrossGroupFusedHit`] with a known `fused_score` for
/// the SA1-SA8 fixture.
#[cfg(test)]
fn make_assembly_hit(file: &str, score: f64) -> CrossGroupFusedHit {
    CrossGroupFusedHit {
        file_path: PathBuf::from(file),
        start_line: 1,
        end_line: 10,
        snippet: format!("// {file}\nfn placeholder() {{}}"),
        fused_score: score,
        contributing_groups: Vec::new(),
        per_group_ranks: Vec::new(),
    }
}

/// Frozen test for [`assemble_response`].
///
/// SA tags (mutation-targeted):
///
/// * SA1 — input-count preserved into `total_input`.
/// * SA2 — `file_a.rs` appears twice; both hits are deduped
///   (M1 target: bypass dedup → SA2 fails with 0 instead of 2).
/// * SA3 — score 0.05 + 0.00 are below 0.1 threshold; both
///   filtered (M2 target: bypass threshold → SA3 fails with 0
///   instead of 2).
/// * SA4 — only `file_b.rs` at 0.30 survives both filters
///   (5 - 2 dedup - 2 filter = 1).
/// * SA5 — surviving hit is `file_b.rs`.
/// * SA6 — `token_count` is in the plausible range (1, 1000).
/// * SA7 — conservation invariant:
///   `total_input == hits + deduped + filtered`.
/// * SA8 — boundary inclusion at `fused_score == 0.1` (the hit
///   survives because `0.1 < 0.1` is false).
#[cfg(test)]
#[test]
fn test_response_assembly() {
    // Five hits — file_a.rs twice (deduped), file_b.rs at 0.30
    // (surviving), file_c.rs at 0.05 + file_d.rs at 0.00 (both
    // below threshold).
    let hits = vec![
        make_assembly_hit("file_a.rs", 0.50),
        make_assembly_hit("file_b.rs", 0.30),
        make_assembly_hit("file_c.rs", 0.05),
        make_assembly_hit("file_d.rs", 0.00),
        make_assembly_hit("file_a.rs", 0.20),
    ];
    let outcome = CrossGroupFusedOutcome {
        hits,
        used_weights: [0.0; 8],
        degraded_groups: Vec::new(),
    };
    let mut files_in_context: HashSet<PathBuf> = HashSet::new();
    files_in_context.insert(PathBuf::from("file_a.rs"));
    let options = ResponseAssemblyOptions::default();

    let result = assemble_response(&outcome, &files_in_context, &options);

    // ── SA1 — total_input preserved ──────────────────────────────
    assert_eq!(
        result.meta.total_input,
        5,
        "(SA1) total_input expected 5; observed {n}",
        n = result.meta.total_input,
    );

    // ── SA2 — both file_a.rs hits deduped (M1 target) ─────────────
    assert_eq!(
        result.meta.deduped_count,
        2,
        "(SA2) deduped_count expected 2; observed {n}",
        n = result.meta.deduped_count,
    );

    // ── SA3 — score-0.05 and score-0.00 filtered (M2 target) ──────
    assert_eq!(
        result.meta.filtered_count,
        2,
        "(SA3) filtered_count expected 2; observed {n}",
        n = result.meta.filtered_count,
    );

    // ── SA4 — only file_b.rs at 0.30 survives ─────────────────────
    assert_eq!(
        result.hits.len(),
        1,
        "(SA4) surviving hits expected 1; observed {n}",
        n = result.hits.len(),
    );

    // ── SA5 — surviving hit IS file_b.rs ──────────────────────────
    assert_eq!(
        result.hits[0].file_path,
        PathBuf::from("file_b.rs"),
        "(SA5) survivor file_path expected file_b.rs; observed {fp:?}",
        fp = result.hits[0].file_path,
    );

    // ── SA6 — token_count plausible-range sanity ──────────────────
    assert!(
        result.meta.token_count > 0 && result.meta.token_count < 1000,
        "(SA6) token_count out of plausible range [1, 1000]; observed {n}",
        n = result.meta.token_count,
    );

    // ── SA7 — conservation invariant ──────────────────────────────
    assert_eq!(
        result.meta.total_input,
        result.hits.len() + result.meta.deduped_count + result.meta.filtered_count,
        "(SA7) total_input != hits + deduped + filtered: {a} vs {b} + {c} + {d}",
        a = result.meta.total_input,
        b = result.hits.len(),
        c = result.meta.deduped_count,
        d = result.meta.filtered_count,
    );

    // ── SA8 — boundary inclusion at fused_score == 0.1 ────────────
    //
    // A second outcome with one hit at exactly the threshold;
    // dedup-empty input.  The hit MUST survive because
    // `0.1 < 0.1` is false (strict-`<` comparison per
    // master-plan §6.3 line 667).
    {
        let outcome2 = CrossGroupFusedOutcome {
            hits: vec![make_assembly_hit("file_boundary.rs", 0.1)],
            used_weights: [0.0; 8],
            degraded_groups: Vec::new(),
        };
        let empty_files: HashSet<PathBuf> = HashSet::new();
        let result2 = assemble_response(&outcome2, &empty_files, &options);
        assert_eq!(
            result2.hits.len(),
            1,
            "(SA8) boundary-inclusion at threshold: hit at score=0.1 expected to survive; \
             observed filtered_count={n}, hits.len={h}",
            n = result2.meta.filtered_count,
            h = result2.hits.len(),
        );
    }
}
