---
work_order: WO-0087
feature: P3-W10-F01
phase: 3
week: 10
branch: feat/WO-0087-aider-repo-map-pagerank
ready_at: 2026-05-09T04:30:00Z
attempt: 2
prior_rejection: ucil-build/rejections/WO-0087.md
prior_rca: ucil-build/verification-reports/root-cause-WO-0087.md
---

# WO-0087 — Ready for review (attempt 2)

## Summary

Implements **P3-W10-F01** — Aider-style repo-map (reimplemented in Rust)
per master-plan §1.1 + §3.5 + §4.5 + §6.1 line 506 + §6.3 + §17.2 +
§18 Phase 3 Week 10 line 1808.  New module
`crates/ucil-core/src/context_compiler.rs` implements:

* `RepoMap`, `RepoMapOptions`, `RankedSymbol`, `RepoMapError` public
  types with `Default` impls reproducing the §6.1 + §6.3 canonical
  defaults (`damping = 0.85`, `max_iterations = 100`, `tolerance = 1e-6`,
  `recency_bias_multiplier = 50.0`, `token_budget = 8000`).
* `pub fn personalized_page_rank(...)` — pure-deterministic sparse
  PageRank kernel over `HashMap<i64, Vec<i64>>` adjacency with explicit
  dangling-mass redistribution and L1-norm convergence.  Tracing span
  `ucil.core.context_compiler.page_rank` per master-plan §15.2.
* `pub fn build_repo_map(kg, recently_queried_files, options)` — entry
  point.  Reads every entity + every `kind = "calls"` relation, builds
  the personalization vector with 50× bias on entities whose
  `file_path` is in `recently_queried_files`, runs the kernel, sorts
  descending by score (tie-break on `qualified_name` ascending), and
  greedily fits a strict prefix to the token budget.
* `fn fit_to_budget(...)` — greedy prefix-fit (NOT knapsack) per the
  Aider repo-map semantic.
* `pub fn entity_token_estimate(...)` — 4-char-per-token cl100k_base
  lower-bound heuristic with a `+ 8` per-symbol newline/delimiter
  overhead.
* Module-root frozen test `fn test_repo_map_pagerank` (DEC-0007
  placement, NOT under `mod tests { ... }`) seeds an isolated
  6-entity / 6-relation / 3-file DAG via `tempfile::TempDir` +
  `KnowledgeGraph::open(...)` and asserts SA1 / SA2 / SA3 invariants.

KG enumeration uses **path #4(a) PREFERRED** — two new `pub fn`
helpers `list_all_entities()` + `list_all_calls_relations()` were
added to `crates/ucil-core/src/knowledge_graph.rs` (additive, ~30 LOC
each, full-table SELECT scans with column projections matching the
existing `list_entities_by_file` / `list_relations_by_source` patterns,
each with `#[tracing::instrument(...)]` annotations). Documented inline
as warm-up scans, not hot-loop callers.  These additions are reusable
for downstream G5/G3/G4 wirings (F04/F09/F11/F16).

## Branch

* Branch: `feat/WO-0087-aider-repo-map-pagerank`
* HEAD at RFR push (will be updated by the RFR commit): `e0e6cd4`
* Commits on this branch (5 total expected):
  1. `5fe4f02` — `feat(core): add context_compiler module skeleton with RepoMap + RepoMapOptions + RepoMapError types`
  2. `9ef413b` — `feat(core): implement personalized PageRank kernel with sparse adjacency`
  3. `d236016` — `feat(core): wire KG enumeration + 50x recency bias + token-budget fitting in build_repo_map` (folds path #4(a) KG helpers — see scope_in #25 carve-out)
  4. `e0e6cd4` — `test(core): seed KG with 6-entity DAG and assert structural+recency+budget invariants`
  5. _(this commit)_ — `docs(work-orders): WO-0087 ready-for-review`

## Acceptance criteria — local verification

| AC | Status | Notes |
|----|--------|-------|
| AC01 | ✅ | `crates/ucil-core/src/context_compiler.rs` exists |
| AC02 | ✅ | `pub mod context_compiler;` in `lib.rs` |
| AC03 | ✅ | `pub use context_compiler::{...};` in `lib.rs` |
| AC04 | ✅ | `pub struct RepoMap` declared |
| AC05 | ✅ | `pub struct RepoMapOptions` declared |
| AC06 | ✅ | `pub struct RankedSymbol` declared |
| AC07 | ✅ | `pub enum RepoMapError` declared (`thiserror::Error`, `#[non_exhaustive]`) |
| AC08 | ✅ | `pub fn build_repo_map` |
| AC09 | ✅ | `pub fn personalized_page_rank` |
| AC10 | ✅ | `recency_bias_multiplier: 50.0` default |
| AC11 | ✅ | `damping: 0.85` default |
| AC12 | ✅ | `name = "ucil.core.context_compiler.page_rank"` tracing span |
| AC13 | ✅ | `#[cfg(test)]` ≥1 (test gate present) |
| AC14 | ✅ | `fn test_repo_map_pagerank` at module root |
| AC15 | ✅ | `cargo test ... -- --list` shows exactly 1 entry |
| AC16 | ✅ | `cargo test -p ucil-core context_compiler::test_repo_map_pagerank` passes (1 passed) |
| AC17 | ✅ | `cargo clippy -p ucil-core --all-targets -- -D warnings` exits 0; `cargo fmt --check -p ucil-core` exits 0 |
| AC18 | ✅* | `cargo test --workspace --no-fail-fast` — **1 pre-existing failure** in `ucil-embeddings::models::test_coderankembed_inference` (missing ONNX model artefacts at `ml/models/coderankembed/` — known WO-0059 standing requirement, unrelated to WO-0087; see Disclosed deviations §A) |
| AC19 | ✅ | Word-ban: no `mock|fake|stub` in production-side files |
| AC20 | ✅ | No stubs (`todo!`/`unimplemented!`/`NotImplementedError`/`raise NotImplementedError`/`TODO: implement`) |
| AC21 | ✅ | No `#[ignore]` / `.skip()` / `xfail` |
| AC22 | ✅ | `Cargo.toml` workspace + crate manifests unchanged: `git diff main -- Cargo.toml crates/ucil-core/Cargo.toml = 0 lines` |
| AC23 | ✅ | M1 mutation FAILS with SA2 panic (see Mutation contract §1) |
| AC24 | ✅ | M2 mutation FAILS with SA3a panic (see Mutation contract §2) |
| AC25 | ✅ | M3 mutation FAILS with SA1 panic (see Mutation contract §3) |
| AC26 | ✅ | All three mutations reversible via `git checkout -- crates/ucil-core/src/context_compiler.rs`; `md5sum -c /tmp/wo-0087-context-compiler-orig.md5` returns `OK` after each |
| AC27 | ✅ | This RFR exists with summary, mutation contract, convergence diagnostics, path-decision log, disclosed deviations |
| AC28 | ✅ | `git log feat/WO-0087-aider-repo-map-pagerank ^main --merges` returns empty (zero merge commits) |
| AC29 | ✅ | `tests/fixtures/**` not modified |
| AC30 | ✅ | `feature-list.json` + `feature-list.schema.json` not modified |
| AC31 | ✅ | `ucil-master-plan-v2.1-final.md` not modified |
| AC32 | ✅ | No `std::process::Command` |
| AC33 | ✅ | No `unsafe` blocks |
| AC34 | ✅ | No `.unwrap()` / `.expect()` outside `#[cfg(test)]` |
| AC35 | ✅ | 5 commits matching DEC-0005 module-coherence (skeleton + algorithm kernel + KG-wiring/budget + test + RFR) |

## Convergence diagnostics

Captured via temporary `eprintln!` instrumentation on a one-off test
run (reverted to `git`-clean before the RFR commit; md5 round-trip
verified):

```
RFR-DIAG SA1: iterations=12 converged=true
RFR-DIAG SA1[0] qn=Some("b::handler") score=0.270073 tokens=13
RFR-DIAG SA1[1] qn=Some("a::child1") score=0.194331 tokens=12
RFR-DIAG SA1[2] qn=Some("a::child2") score=0.162101 tokens=12
RFR-DIAG SA1[3] qn=Some("c::leaf")   score=0.145985 tokens=11
RFR-DIAG SA1[4] qn=Some("a::root")   score=0.113755 tokens=11
RFR-DIAG SA1[5] qn=Some("b::helper") score=0.113755 tokens=12

RFR-DIAG SA2: iterations=12 converged=true
RFR-DIAG SA2[0] qn=Some("a::child1") score=0.226845 fp="src/file_a.rs"
RFR-DIAG SA2[1] qn=Some("b::handler") score=0.220123 fp="src/file_b.rs"
RFR-DIAG SA2[2] qn=Some("a::child2") score=0.200576 fp="src/file_a.rs"
RFR-DIAG SA2[3] qn=Some("a::root")   score=0.140755 fp="src/file_a.rs"
RFR-DIAG SA2[4] qn=Some("c::leaf")   score=0.118985 fp="src/file_c.rs"
RFR-DIAG SA2[5] qn=Some("b::helper") score=0.092716 fp="src/file_b.rs"

RFR-DIAG SA3: iterations=12 converged=true total_tokens=25 len=2
RFR-DIAG SA3[0] qn=Some("b::handler") score=0.270073 tokens=13
RFR-DIAG SA3[1] qn=Some("a::child1") score=0.194331 tokens=12
```

* **SA1**: PageRank converged in 12 iterations.  Top symbol is
  `b::handler` (0.2701) — beating `a::child1` (0.1943) by ~0.076,
  well above the 1e-6 tie-break threshold.  ✅
* **SA2**: PageRank converged in 12 iterations.  Top symbol is
  `a::child1` (0.2268, file_path `src/file_a.rs`) — beating
  `b::handler` (0.2201) by ~0.0067.  Top file_path matches
  `src/file_a.rs` per SA2's contract.  ✅
* **SA3**: PageRank converged in 12 iterations.  Token budget = 30
  truncated the 6-symbol list to 2 symbols
  (`b::handler` 13 tokens + `a::child1` 12 tokens = 25 ≤ 30); the
  3rd symbol (`a::child2` 12 tokens) would yield 37 > 30, so the
  greedy prefix stops.  Returned symbols are a strict prefix of the
  unbudgeted ranking.  ✅

(All three runs converge in exactly 12 iterations — the 6-entity
fixture's small spectral radius gives fast L1 convergence under
`tolerance = 1e-6`.)

## Mutation contract (M1 / M2 / M3)

Pre-mutation md5 snapshot:

```
$ md5sum crates/ucil-core/src/context_compiler.rs > /tmp/wo-0087-context-compiler-orig.md5
$ cat /tmp/wo-0087-context-compiler-orig.md5
a13e2b0b4aec53ba1863ec50b9817785  crates/ucil-core/src/context_compiler.rs
```

Each mutation form below was applied via `Edit`, the cargo-test
selector was re-run, the SA-tagged panic body was confirmed, and the
file was restored via `git checkout --` followed by `md5sum -c`
returning `OK`.

### §1 — M1 — Recency-bias multiplier removal (targets SA2)

**File**: `crates/ucil-core/src/context_compiler.rs`

**Patch** (in `build_repo_map`, the per-entity personalization mass
assignment):

```diff
         let mass = if recently_queried_files.contains(&path) {
-            options.recency_bias_multiplier * uniform_mass
+            uniform_mass
         } else {
             uniform_mass
         };
```

**Selector**: `cargo test -p ucil-core context_compiler::test_repo_map_pagerank`

**Expected panic body** (SA2):

```
thread 'context_compiler::test_repo_map_pagerank' panicked at
crates/ucil-core/src/context_compiler.rs:<line>:5:
assertion `left == right` failed: (SA2) recency-bias top symbol expected
file_a.rs; observed file_path="src/file_b.rs", qualified_name="b::handler"
  left: "src/file_b.rs"
 right: "src/file_a.rs"
```

**Observed**: ✅ exactly the SA2 panic body, with `top_fp = "src/file_b.rs"`,
`top_qn = "b::handler"` (the unbiased structural winner reverts when
the multiplier is removed).

**Restore**: `git checkout -- crates/ucil-core/src/context_compiler.rs`

**md5 round-trip**: ✅ `md5sum -c /tmp/wo-0087-context-compiler-orig.md5 = OK`

### §2 — M2 — Token-budget fit removal (targets SA3)

**File**: `crates/ucil-core/src/context_compiler.rs`

**Patch** (in `fit_to_budget`):

```diff
-fn fit_to_budget(ranked: Vec<RankedSymbol>, token_budget: usize) -> (Vec<RankedSymbol>, usize) {
-    let mut total = 0usize;
-    let mut fitted: Vec<RankedSymbol> = Vec::with_capacity(ranked.len());
-    for entry in ranked {
-        let next_total = total.saturating_add(entry.token_estimate);
-        if next_total > token_budget {
-            break;
-        }
-        total = next_total;
-        fitted.push(entry);
-    }
-    (fitted, total)
-}
+fn fit_to_budget(ranked: Vec<RankedSymbol>, _token_budget: usize) -> (Vec<RankedSymbol>, usize) {
+    let total: usize = ranked.iter().map(|s| s.token_estimate).sum();
+    (ranked, total)
+}
```

**Selector**: `cargo test -p ucil-core context_compiler::test_repo_map_pagerank`

**Expected panic body** (SA3a):

```
thread 'context_compiler::test_repo_map_pagerank' panicked at
crates/ucil-core/src/context_compiler.rs:<line>:5:
(SA3a) token budget exceeded: total_tokens=71 > 30
```

(Total of all 6 entities' token estimates: 13 + 12 + 12 + 11 + 11 + 12 = 71.)

**Observed**: ✅ exactly the SA3a panic body, `total_tokens=71 > 30`.

**Restore**: `git checkout -- crates/ucil-core/src/context_compiler.rs`

**md5 round-trip**: ✅ `OK`

### §3 — M3 — PageRank update equation negation (targets SA1)

**File**: `crates/ucil-core/src/context_compiler.rs`

**Patch** (in `personalized_page_rank` iteration kernel):

```diff
-            let new_v = (1.0 - options.damping).mul_add(pers_v, options.damping * incoming_sum)
+            let new_v = (1.0 - options.damping).mul_add(pers_v, -options.damping * incoming_sum)
                 + dangling_share;
```

**Selector**: `cargo test -p ucil-core context_compiler::test_repo_map_pagerank`

**Expected panic body** (SA1):

```
thread 'context_compiler::test_repo_map_pagerank' panicked at
crates/ucil-core/src/context_compiler.rs:<line>:5:
assertion `left == right` failed: (SA1) structural pagerank winner expected
b::handler; observed "a::root"
  left: "a::root"
 right: "b::handler"
```

(With the sign flip on the incoming-sum term, high-incoming nodes
accumulate the most-negative score; nodes with NO incoming edges
— `root`, `helper` — top the ranking.  Tie-break on
`qualified_name` ascending puts `a::root` ahead of `b::helper`.)

**Observed**: ✅ exactly the SA1 panic body, `observed "a::root"`.

**Restore**: `git checkout -- crates/ucil-core/src/context_compiler.rs`

**md5 round-trip**: ✅ `OK`

## Path #4(a) PREFERRED vs #4(b) ALTERNATE — decision log

The work-order's scope_in #4 offered two KG enumeration paths:

* **#4(a) PREFERRED** — add `list_all_entities()` + `list_all_calls_relations()`
  to `crates/ucil-core/src/knowledge_graph.rs` as additive `pub fn`
  helpers (~30 LOC each, full-table SELECT scans).
* **#4(b) ALTERNATE** — use `KnowledgeGraph::execute_in_transaction` +
  inline `prepare(...)` SQL in `build_repo_map`.

**Selected**: **#4(a) PREFERRED**.

**Rationale**:

1. The new helpers are reusable for downstream features (F04 G5
   Context query, F09 quality-maximalist response assembly, F11 bonus
   context selector, F16 full query-pipeline integration suite) — a
   single concrete API surface beats N copies of inline SQL.
2. The query shape (`SELECT <columns> FROM entities` /
   `SELECT <columns> FROM relations WHERE kind = ?1`) is trivially
   testable in isolation via the existing knowledge-graph integration
   tests (no new test file added — the WO-0087 unit test exercises
   both helpers transitively).
3. The work-order's `scope_in #4` explicitly recommends path #4(a)
   ("recommended because the new methods are reusable for downstream
   G5/G3/G4 wirings").
4. Path #4(a) avoids the `prepare()` + raw SQL leak inside
   `build_repo_map`, keeping the algorithm-side code free of SQLite
   string literals.

**Cost**: ~60 additional LOC in `knowledge_graph.rs` (split across
the two helpers + their rustdoc + tracing instrumentation).

**Implications**: zero schema change; zero migration; zero new index;
the helpers are thin reads with the same projection mappings as the
existing `list_*_by_*` methods.  The pre-flight md5 snapshot for
`knowledge_graph.rs` (per scope_in #9) was recorded at planner-time
and round-tripped via `md5sum -c` after each mutation — confirmed
zero accidental change to the helpers under M1/M2/M3.

## Disclosed deviations

### §A — Pre-existing `ucil-embeddings::models::test_coderankembed_inference` failure (AC18)

`cargo test --workspace --no-fail-fast` reports one failing test on
the WO-0087 branch:

```
test models::test_coderankembed_inference ... FAILED

thread 'models::test_coderankembed_inference' panicked at crates/ucil-embeddings/src/models.rs:920:5:
CodeRankEmbed model artefacts not present at "ml/models/coderankembed";
run `bash scripts/devtools/install-coderankembed.sh` first (P2-W8-F02 / WO-0059);
got model.onnx exists=false, tokenizer.json exists=false
```

This is a **pre-existing standing-protocol environmental dependency**
unrelated to WO-0087 (the repo-map module under WO-0087 does NOT
touch the ONNX inference path).  The failure originates in
`P2-W8-F02 / WO-0059` and requires the developer to first run
`bash scripts/devtools/install-coderankembed.sh` to download the
ONNX model artefacts (which the test gates on at runtime).  It is
NOT a regression introduced by WO-0087 — `git stash` + same test on
`main` (before the WO-0087 commits) reproduces the same failure.

### §B — Topology change from scope_in #8 (per RCA `verification-reports/root-cause-WO-0087.md` hypothesis #1)

The first-attempt verifier rejected (`ucil-build/rejections/WO-0087.md`)
because the load-bearing test commit + RFR commit did not land.  The
root-cause-finder (`ucil-build/verification-reports/root-cause-WO-0087.md`)
diagnosed at 95% confidence that the test commit was withheld because
the planner's example-DAG topology in `scope_in #8` does not produce
`b::handler` as the unbiased PageRank winner — the planner's chain
graph (`root → child{1,2} → handler → helper → leaf`) concentrates
equilibrium PageRank at the chain sink `c::leaf` (analytical
equilibrium: leaf=0.279, helper=0.253, handler=0.221).

This RFR's test uses the RCA's recommended replacement edge set:

```rust
make_test_call_relation(id_root,   id_child1),  // file_a internal
make_test_call_relation(id_root,   id_child2),  // file_a internal
make_test_call_relation(id_helper, id_handler), // file_b internal (1st handler-incoming)
make_test_call_relation(id_leaf,   id_handler), // file_c → file_b (2nd handler-incoming)
make_test_call_relation(id_helper, id_leaf),    // file_b → file_c
make_test_call_relation(id_helper, id_child1),  // file_b → file_a (gives child1 a feeder)
```

**Spec-frozen invariants preserved**:

* 6 entities + 6 `kind = "calls"` relations + 3 files (matches
  scope_in #8's `6 entities + 6+ Relations` literal).
* SA1/SA2/SA3 panic bodies and the cargo selector
  (`context_compiler::test_repo_map_pagerank`) unchanged.
* Mutation contract M1/M2/M3 still surfaces SA2/SA3a/SA1 respectively
  (verified empirically — see Mutation contract §1/§2/§3 above).

**Spirit-over-literal precedent**: WO-0070 (g3-parallel-merge),
WO-0083 (knowledge-graph schema migration), WO-0084
(tier-merger-and-conflict-resolution).  In each precedent the
executor disclosed a topology / fixture / data-shape deviation from
the planner's example while preserving the load-bearing acceptance
contract — the same shape applied here.

**Why the planner's topology is algebraically wrong** (paraphrasing
the RCA):

1. Under the planner's chain `root → child{1,2} → handler → helper → leaf`,
   `handler` has 2 incoming edges (from child1, child2) but its score
   immediately flows to `helper` (out_deg=1), then to `leaf`
   (out_deg=1, dangling).
2. The dangling-mass redistribution at `personalized_page_rank` line
   ~382 spreads the leaf's accumulated score uniformly across all 6
   nodes per iteration — giving each upstream node only `1/6` of leaf's
   teleported mass per iteration.
3. The 2 incoming edges into `handler` are insufficient to overcome
   the downstream concentration — equilibrium settles with `c::leaf`
   on top, not `b::handler`.

**Why the new topology works** (verified analytically + empirically):

1. `b::handler` now has 2 incoming edges (helper, leaf) AND has
   `out_deg = 0` (dangling) — so its incoming mass accumulates
   without bleeding downstream, and only 1/6 of its mass leaks out
   via the dangling-redistribution path.  `handler` reaches
   equilibrium score 0.270 (vs. the next-best `a::child1` at 0.194).
2. Under 50× bias on `src/file_a.rs`, the bias amplifies the
   personalization vector entries on file_a's 3 entities (root,
   child1, child2).  `a::child1` has 2 structural incoming edges
   (root, helper) so it accumulates the bias-amplified mass without
   bleeding it back to handler, reaching equilibrium 0.227 vs.
   `b::handler` 0.220.  Top file_path is `src/file_a.rs`. ✅

### §C — `cargo test ... --list` separator

The acceptance-criteria gate string in the work-order's
`acceptance_criteria` array uses the form
`cargo test -p ucil-core context_compiler::test_repo_map_pagerank --list`,
but recent Cargo (1.94+) requires the `-- --list` separator
(`cargo test -p ucil-core context_compiler::test_repo_map_pagerank -- --list`).
The substring count is identical between the two invocation forms
(both pipe through `grep -cE 'test_repo_map_pagerank: test'` returning
`1`).  Verifier should use the `-- --list` form (the rejection report
already used this form, so no operator change needed).

## Reproduction

Verifier's reproduction sequence:

```bash
cd /home/rishidarkdevil/Desktop/ucil-wt/WO-0087
git checkout feat/WO-0087-aider-repo-map-pagerank
git pull --ff-only
cargo clean
cargo test -p ucil-core context_compiler::test_repo_map_pagerank
# expect: test result: ok. 1 passed
cargo clippy -p ucil-core --all-targets -- -D warnings
# expect: exit 0
cargo fmt --check -p ucil-core
# expect: exit 0
md5sum crates/ucil-core/src/context_compiler.rs > /tmp/wo-0087-orig.md5
# Apply M1 / M2 / M3 mutations per §1 / §2 / §3 above
# Re-run cargo test, expect SA-tagged panic
git checkout -- crates/ucil-core/src/context_compiler.rs
md5sum -c /tmp/wo-0087-orig.md5
# expect: OK
```

## Citations

* Work-order: `ucil-build/work-orders/0087-aider-repo-map-pagerank.json`
* First-attempt rejection: `ucil-build/rejections/WO-0087.md`
* Root-cause analysis: `ucil-build/verification-reports/root-cause-WO-0087.md`
* Critic report (first attempt): `ucil-build/critic-reports/WO-0087.md`
* Master plan citations: §1.1 line 44, §3.5, §4.5 line 345, §6.1 line 506,
  §6.3 line 660, §17.2 line 1634, §18 Phase 3 Week 10 line 1808
* Decision: `ucil-build/decisions/DEC-0007-remove-cargo-mutants-per-wo-gate.md`
  (frozen-test selector module-root placement)
* Decision: `ucil-build/decisions/DEC-0005-WO-0006-module-coherence-commits.md`
  (module-coherence commit carve-out)
* Style rules: `.claude/rules/rust-style.md` §Crate-layout, §Errors,
  §Tracing, §Async, §Unsafe
* Spirit-over-literal precedent: WO-0070, WO-0083, WO-0084
