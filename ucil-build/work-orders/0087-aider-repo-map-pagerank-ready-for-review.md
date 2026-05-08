# WO-0087 — ready for review (attempt 2, post-RCA remediation)

**Final commit sha**: `57020fe` (docs(work-orders): WO-0087 ready-for-review)
**Branch**: `feat/WO-0087-aider-repo-map-pagerank`
**Phase / Week**: 3 / 10
**Features**: P3-W10-F01 (Aider repo-map reimplementation in Rust)

## Summary

Implements **P3-W10-F01** — Aider-style repo-map (PageRank, 50× recency
bias, token-budget fitting) in `crates/ucil-core/src/context_compiler.rs`.
This is the second attempt; the first attempt (`d236016`) had the
production code but withheld the test commit because the planner's
example-DAG topology in `scope_in #8` was algebraically incompatible
with the SA1 assertion (RCA hypothesis #1, 95% confidence — see
`ucil-build/verification-reports/root-cause-WO-0087.md`).

Per the RCA's recommended remediation, this attempt revises the test
fixture's edge set to a topology that satisfies SA1/SA2/SA3 while
preserving the spec-frozen invariants (6 entities + 6 `calls` relations
+ 3 files; SA-tagged panic bodies + cargo selector unchanged; M1/M2/M3
mutation contract unchanged).

## Canonical RFR

See `ucil-build/work-orders/0087-ready-for-review.md` for the AC27
canonical RFR document with:

* Full mutation contract (M1 / M2 / M3 patches + selector + expected
  panic body + restore command + md5 round-trip).
* Convergence diagnostics (SA1 / SA2 / SA3 each converged in 12
  iterations; equilibrium scores match the RCA's analytical prediction
  within 0.5%).
* Path #4(a) PREFERRED vs #4(b) ALTERNATE decision log (selected #4(a)
  — added `list_all_entities` + `list_all_calls_relations` to
  `crates/ucil-core/src/knowledge_graph.rs`).
* Disclosed deviations §A (pre-existing ucil-embeddings ONNX artefact
  failure unrelated to WO-0087), §B (topology change per RCA per
  WO-0070/0083/0084 spirit-over-literal precedent), §C (cargo
  `--list` separator note).

## Commits on branch

```
57020fe docs(work-orders): WO-0087 ready-for-review
e0e6cd4 test(core): seed KG with 6-entity DAG and assert structural+recency+budget invariants
d236016 feat(core): wire KG enumeration + 50x recency bias + token-budget fitting in build_repo_map
9ef413b feat(core): implement personalized PageRank kernel with sparse adjacency
5fe4f02 feat(core): add context_compiler module skeleton with RepoMap + RepoMapOptions + RepoMapError types
```

5 commits matching DEC-0005 module-coherence (skeleton + algorithm
kernel + KG-wiring/budget + test + RFR).  Zero merge commits
(`git log feat/WO-0087-aider-repo-map-pagerank ^main --merges` empty).

## Local-acceptance test result

```
$ cargo test -p ucil-core context_compiler::test_repo_map_pagerank
   Compiling ucil-core v0.1.0 (.../crates/ucil-core)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 1.67s
     Running unittests src/lib.rs (.../target/debug/deps/ucil_core-*)

running 1 test
test context_compiler::test_repo_map_pagerank ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 46 filtered out; finished in 0.00s
```

```
$ cargo clippy -p ucil-core --all-targets -- -D warnings   # exit 0
$ cargo fmt --check -p ucil-core                           # exit 0
```

Mutation contract M1 / M2 / M3 each surface the SA-tagged panic body
and md5-round-trip via `git checkout --` returns OK — see canonical
RFR for the verbatim panic strings.

## Verifier handoff

Verifier should reproduce on `57020fe` from a clean state:

```bash
cd /home/rishidarkdevil/Desktop/ucil-wt/WO-0087
git checkout feat/WO-0087-aider-repo-map-pagerank
git pull --ff-only
cargo clean
cargo test -p ucil-core context_compiler::test_repo_map_pagerank
```

— and then run AC01..AC35 from the work-order JSON.
