# Root Cause Analysis: WO-0087 / P3-W10-F01 (Aider repo-map PageRank)

**Analyst session**: rca-wo-0087-r1
**Work-order**: WO-0087
**Feature**: P3-W10-F01
**Branch**: feat/WO-0087-aider-repo-map-pagerank @ d236016918f68fb01fbe9fdad2339313fc4e7f22
**Worktree**: /home/rishidarkdevil/Desktop/ucil-wt/WO-0087
**Attempts before RCA**: 1 (attempts now=1, threshold for next RCA=3)

## Failure pattern

The verifier (vrf-wo-0087-pre-test-pre-rfr, ucil-build/rejections/WO-0087.md) and
critic (crt-wo-0087-aider-repo-map-pagerank, ucil-build/critic-reports/WO-0087.md)
both REJECTED on the same surface failure: commits (4) `test(core): seed KG…` and
(5) `docs(work-orders): WO-0087 ready-for-review` did not land on the pushed branch.
AC13 / AC14 / AC15 / AC16 / AC23 / AC24 / AC25 / AC27 / AC35 all trip on the
absence of `fn test_repo_map_pagerank`.

But the **deeper** failure — and the reason the test commit was never pushed —
is a **planner-spec / algebra mismatch** that the executor discovered locally
but did not (or could not) resolve.

## What I observed

The executor's local-only uncommitted work (`git diff
crates/ucil-core/src/context_compiler.rs` in the worktree) DOES contain a
fully-formed `fn test_repo_map_pagerank` at module root with `seed_repo_map_test_kg`,
`make_test_entity`, `make_test_call_relation`, and the SA1/SA2/SA3 assertion
bodies. The test compiles and the cargo selector resolves uniquely:

```
$ cd /home/rishidarkdevil/Desktop/ucil-wt/WO-0087
$ cargo test -p ucil-core context_compiler::test_repo_map_pagerank -- --list
context_compiler::test_repo_map_pagerank: test
1 test, 0 benchmarks
```

Running the test fails on **SA1** (structural PageRank winner) at
`crates/ucil-core/src/context_compiler.rs:770`:

```
assertion `left == right` failed: (SA1) structural pagerank winner expected
b::handler; observed "c::leaf"
  left: "c::leaf"
 right: "b::handler"
```

This is why the executor never committed/pushed the test: it does not pass on
the topology the planner specified.

## Root cause (hypothesis, **95% confidence**)

The work-order's `scope_in #8` specifies a 6-entity DAG topology and asserts
`b::handler` is the structural-PageRank winner under unbiased PageRank. But
this is **algebraically false** for the standard damped personalized PageRank
with explicit dangling-mass redistribution that the executor implemented per
master-plan §6.1 + §6.3.

### Why the planner intuition is wrong

Under the planner's topology
(`crates/ucil-core/src/context_compiler.rs:691-696` in the local diff):

```
root → child1, child2          (file_a → file_a)
child1 → handler, child2 → handler  (file_a → file_b)
handler → helper               (file_b → file_b)
helper  → leaf                 (file_b → file_c)
```

- `handler` has 2 incoming edges (from child1, child2) **plus 1 outgoing** (to helper).
- The outgoing edge causes handler's score × 0.85 to flow to helper each
  iteration, then to leaf.
- `leaf` is a dangling sink (0 outgoing). Its score is redistributed
  **uniformly** across all 6 nodes per iteration via the standard
  dangling-mass treatment at `context_compiler.rs:382-391` — but with N=6 and
  only one dangling node, only 1/6 of leaf's mass leaks back upstream per
  iteration.
- Net: at equilibrium, score concentrates at the chain's downstream sink
  (`leaf`), not at the "branching hub" (`handler`).

I solved the linear-system equilibrium by hand for unbiased PageRank under
the planner's topology with d=0.85, max_iter=100, tol=1e-6:

| node     | equilibrium score | rank |
| -------- | ----------------- | ---- |
| c::leaf  | **0.279**         | **1**|
| b::helper| 0.253             | 2    |
| b::handler| 0.221            | 3    |
| a::child1| 0.092             | 4    |
| a::child2| 0.092             | 4    |
| a::root  | 0.065             | 6    |

The sum is 1.0 ± rounding. The `assert_eq!(top_qn, "b::handler")` panics with
`observed "c::leaf"` — exactly what the cargo run reports.

### Why this also breaks SA2 (recency-bias flip) on the same topology

I solved the equilibrium under SA2 (50× bias on `src/file_a.rs`, so
`pers[file_a entity] = 50/153 ≈ 0.327`, `pers[non-file_a] = 1/153 ≈ 0.0065`):

| node      | equilibrium score | rank |
| --------- | ----------------- | ---- |
| b::handler| 0.230             | 1    |
| b::helper | 0.229             | 2    |
| c::leaf   | 0.228             | 3    |
| a::child1 | 0.116             | 4    |
| a::child2 | 0.116             | 4    |
| a::root   | 0.081             | 6    |

The 50× bias on the personalization vector is multiplied by `(1-d) = 0.15`
in the update rule — only ~7.5% effective leverage. Worse, the bias **propagates
downstream** through file_a's outgoing edges (root→child1/child2→handler), so
the bias actually concentrates on file_b/file_c entities. **SA2's expected
"top file_path is src/file_a.rs" would also fail** if the test ever reached
that assertion (it doesn't because SA1 trips first).

So the planner's topology is doubly broken: under its specified edges,
neither SA1 nor SA2 is satisfiable.

### Implementation is correct (no algorithm bug)

The algorithm at `crates/ucil-core/src/context_compiler.rs:329-436` is the
textbook personalized PageRank with damping = 0.85, dangling-mass
redistribution, and L1-norm convergence — all per master-plan §6.1
specification. The critic's review (BLOCKED but production-side CLEAN)
confirms no stubs / no mocks / clippy clean. There is no algorithm bug to fix.

## Hypothesis tree (ranked by likelihood)

1. **(95%) Planner topology error** — the planner's example DAG in scope_in #8
   does not produce `b::handler` as the unbiased winner under standard PageRank.
   ✓ Confirmed by both empirical test run and analytical equilibrium.

2. **(3%) Convergence not reached in 100 iterations** — falsified by inspecting
   the test failure message: the SA1 assert fires with concrete observed value,
   so the kernel returned a converged or max-iter-truncated result; tracing the
   first 3 iterations by hand shows the wave moves handler→helper→leaf well
   before iter 100.

3. **(1%) Tie-break path** — falsified: the equilibrium scores differ by ≥6%
   between leaf and handler; tie-break on qualified_name would only matter if
   they were within 1e-6 of each other.

4. **(1%) KG row-mapping bug** (entities/relations not seeded as expected) —
   falsified by inspecting the test fixture: `seed_repo_map_test_kg`
   (lines 666-702) inserts exactly 6 entities and 6 relations via
   `kg.upsert_entity` / `kg.upsert_relation`, both round-tripped through the
   existing knowledge_graph.rs API.

## Remediation

**Who**: executor (next-attempt).

**What**: Change the topology in `seed_repo_map_test_kg` (lines 691-696 in the
local diff) so that **(a)** `b::handler` actually IS the unbiased structural
winner, AND **(b)** under 50× file_a bias, a file_a entity wins.

The minimal fix that satisfies both SA1 and SA2 while preserving the spec
"6 entities + 6 calls relations + 3 files":

### Replace lines 691-696 of the local-diff fixture

Current (planner's topology — does NOT pass SA1):
```rust
make_test_call_relation(id_root, id_child1),
make_test_call_relation(id_root, id_child2),
make_test_call_relation(id_child1, id_handler),
make_test_call_relation(id_child2, id_handler),
make_test_call_relation(id_handler, id_helper),
make_test_call_relation(id_helper, id_leaf),
```

Replacement (passes SA1 + SA2 + SA3):
```rust
make_test_call_relation(id_root, id_child1),       // file_a internal
make_test_call_relation(id_root, id_child2),       // file_a internal
make_test_call_relation(id_helper, id_handler),    // file_b internal
make_test_call_relation(id_leaf, id_handler),      // file_c → file_b
make_test_call_relation(id_helper, id_leaf),       // file_b → file_c
make_test_call_relation(id_helper, id_child1),     // file_b → file_a (key: gives child1 a non-bleeding feeder)
```

### Update the docstring topology diagram (lines 710-727)

Replace the ASCII art with:
```text
helper  --calls-->  handler  (b::handler — structural winner, 2 incoming)
helper  --calls-->  leaf
helper  --calls-->  child1   (file_b feeds file_a; child1 wins under SA2 bias)
leaf    --calls-->  handler  (handler's 2nd incoming)
root    --calls-->  child1, child2  (file_a internal)
```

And the SA2 sentence: "Under 50× bias on `src/file_a.rs`, **`a::child1`**
takes the top spot — it has 2 structural incoming edges (root, helper) and
3 file_a entities all share the bias multiplier, but child1's structural
position lets it accumulate the bias-amplified mass without bleeding it
back to handler."

### Update SA2 panic body (line ~792)

The current panic body asserts `top_fp == "src/file_a.rs"` (file-level), which
is correct and need not change. The change above only relabels which file_a
entity wins.

### Why this works (verified analytically)

Under the new topology, equilibrium PageRank is:

**Unbiased** (uniform personalization):

| node      | score | rank |
| --------- | ----- | ---- |
| b::handler| 0.270 | **1**|
| a::child1 | 0.194 |  2   |
| a::child2 | 0.162 |  3   |
| c::leaf   | 0.146 |  4   |
| a::root   | 0.114 |  5   |
| b::helper | 0.114 |  5   |

→ **SA1 passes** (handler wins).

**SA2** (50× bias on src/file_a.rs):

| node      | score | rank |
| --------- | ----- | ---- |
| a::child1 | 0.227 | **1**|
| b::handler| 0.221 |  2   |
| a::child2 | 0.201 |  3   |
| a::root   | 0.141 |  4   |
| c::leaf   | 0.119 |  5   |
| b::helper | 0.093 |  6   |

→ **SA2 passes** (top file_path is src/file_a.rs; specifically a::child1).

**SA3** (token-budget=30, tokens per entity = (qname + signature)/4 + 8):

| symbol      | tokens | running | fits? |
| ----------- | ------ | ------- | ----- |
| b::handler  | 13     | 13      | ✓     |
| a::child1   | 12     | 25      | ✓     |
| a::child2   | 12     | 37      | ✗ STOP|

→ Returns 2 of 6 symbols, total_tokens=25, strict-prefix match. **SA3 passes**.

### Mutation contract preservation

- **M1** (`recency_bias_multiplier * uniform_mass` → `uniform_mass`): bias
  removed → SA2 reverts to unbiased ranking → top is handler (file_b), not
  file_a → SA2 panic body fires. ✓
- **M2** (`fit_to_budget` returns `(ranked, total)` unconditionally): no
  truncation → returns all 6 symbols, total > 30 → SA3a or SA3b panic fires. ✓
- **M3** (sign flip on PageRank update: `+ d * incoming_sum` → `- d * incoming_sum`):
  high-incoming nodes (handler) become heavily penalized (most negative score);
  low-incoming nodes (root, with 0 incoming) win → SA1 panic fires with
  `observed "a::root"` (or whichever no-incoming node). ✓

All three mutations remain reversible via `git checkout --
crates/ucil-core/src/context_compiler.rs` + md5sum verify per
`/tmp/wo-0087-context-compiler-orig.md5`.

## After the topology fix

Once the test compiles + passes locally, the executor must:

1. Commit the test as commit (4) per the planner's commit-cadence plan:
   `test(core): seed KG with 6-entity DAG and assert structural+recency+budget invariants`

2. Write `ucil-build/work-orders/0087-ready-for-review.md` (the missing RFR)
   per AC27 / scope_in #23 with:
   - Summary
   - M1 / M2 / M3 mutation contracts (canonical shape)
   - Convergence diagnostics (iterations + converged flag for SA1 / SA2 / SA3)
   - Decision log: path #4(a) PREFERRED was selected (the executor added
     `list_all_entities` and `list_all_calls_relations` to
     `crates/ucil-core/src/knowledge_graph.rs:1003-1019, 1166-1184` per the
     critic's confirmation)
   - **Disclosed deviations section** explaining the topology change from the
     planner's example (cite this RCA as supporting evidence).

3. Commit the RFR as commit (5):
   `docs(work-orders): WO-0087 ready-for-review`

4. Push both commits. Re-spawn critic on updated tip.

## If hypothesis is wrong

Cheap-to-falsify alternative: if for some reason the equilibrium values I
computed are off (numerical-stability bug in the kernel that I missed), the
executor can `RUST_LOG=ucil.core.context_compiler.page_rank=debug cargo test
-p ucil-core context_compiler::test_repo_map_pagerank` to see iteration counts
and per-node scores via the existing `#[tracing::instrument]` annotation at
`context_compiler.rs:322-327` (level=debug, fields num_nodes + max_iter
already set; one-off `tracing::debug!("score: {score:?}")` inside the
iteration loop would expose per-iteration state but requires source edit —
out of scope for this RCA).

If after the topology fix SA2 still fails (e.g., child1 ≈ handler but
qualified-name tie-break trips), bump child1's structural mass by adding a
**7th** edge `leaf → child1` (the spec says "6+" relations, so 7 is allowed).
Numerical-trace pre-flight: the analytical equilibrium predicts
child1 = 0.227, handler = 0.221 — a 6-point gap, well above tie-break
threshold; this fallback is unlikely to be needed.

## Citations

- Failing test: `crates/ucil-core/src/context_compiler.rs:738-770` (test root)
  + `:691-696` (topology fixture, the load-bearing buggy lines)
- PageRank kernel: `crates/ucil-core/src/context_compiler.rs:329-436`
- Dangling-mass redistribution: `crates/ucil-core/src/context_compiler.rs:382-391`
- Personalization renormalization: `crates/ucil-core/src/context_compiler.rs:343-363`
- Update rule (verified textbook-correct):
  `crates/ucil-core/src/context_compiler.rs:411-425`
- Master plan: §1.1 line 44, §3.5, §6.1 line 506 (50× bias),
  §6.3 line 660 (response assembly)
- Critic report: `ucil-build/critic-reports/WO-0087.md` (BLOCKED)
- Verifier rejection: `ucil-build/rejections/WO-0087.md` (REJECT, retry 1)
- Work-order: `ucil-build/work-orders/0087-aider-repo-map-pagerank.json`
  (scope_in #8 = the broken topology spec)
- Precedent for "spirit-over-literal" deviations:
  WO-0070, WO-0083, WO-0084 (cited in the work-order's scope_in #4(b)).

## Worktree state at end of RCA

`git status` in `/home/rishidarkdevil/Desktop/ucil-wt/WO-0087`:

```
 M crates/ucil-core/src/context_compiler.rs
```

(unchanged from RCA start — local-only test code preserved for the executor
retry; no source edits performed by RCA per `.claude/agents/root-cause-finder.md`
rules).
