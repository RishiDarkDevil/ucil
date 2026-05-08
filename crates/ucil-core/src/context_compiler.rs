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
