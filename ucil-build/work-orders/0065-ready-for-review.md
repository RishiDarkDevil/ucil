# WO-0065 Ready for Review

**Work-order**: `WO-0065-vector-query-p95-bench`
**Feature(s)**: `P2-W8-F07`
**Phase**: 2, Week 8
**Branch**: `feat/WO-0065-vector-query-p95-bench`
**Final commit sha**: `5378bfd` (RFR commit) — superseded by the follow-up `chore(work-orders)` commit that fills this line in; see `git log feat/WO-0065-vector-query-p95-bench` HEAD for the latest sha

## Summary

Lands the second Phase 2 Week 8 benchmark leg: a criterion-based
vector query latency bench for the LanceDB+IVF/HNSW path
(master-plan §18 Phase 2 Week 8 line 1789).  The bench builds a
2000-row, 768-d synthetic corpus under `tempfile::tempdir()`,
populates it via `Connection::create_empty_table` + `Table::add`,
trains an `IvfHnswPq` index on the `embedding` column with
`num_partitions = ceil(sqrt(2000)) = 45`, and measures p95 warm-cache
`Table::query().nearest_to(...).limit(10).execute()` latency over 20
deterministic-seed query vectors (round-robin per iteration).
Criterion config: `sample_size(100)`, `warm_up_time(2s)`,
`measurement_time(20s)`.  A shell asserter
(`scripts/bench-vector-query.sh`) parses criterion's `sample.json`,
computes per-sample `mean_ns_per_iter`, sorts ascending, takes
`floor(0.95 * len)` for the p95, prints `p95_warm_ms=<N>` to stdout,
and dual-asserts wall-time floor (`MEAN_NS_PER_ITER >= 100_000` ns —
mock-shape sentinel) AND p95 floor (`< 100` ms per master-plan §18
line 1789).  A frozen verifier wrapper
(`scripts/verify/P2-W8-F07.sh`) re-asserts `< 100` ms independently
per the WO-0061 mutation #3 defence-in-depth pattern.

## What I verified locally

End-to-end measurements on this machine (CPU: x86_64 Linux 6.17):

- `bash scripts/bench-vector-query.sh` → `p95_warm_ms=0.296`
  (~3 orders of magnitude below the 100 ms floor; well above the 100
  µs wall-time floor).
- `bash scripts/verify/P2-W8-F07.sh` → `[OK] P2-W8-F07
  p95_warm_ms=0.266` (rerun at slightly different load gives
  consistent values within noise).
- `cargo build --release -p ucil-embeddings --bench vector_query` →
  exit 0 (after one-time install of `protoc 25.1` to
  `~/.local/bin/protoc` + the `google/protobuf/*.proto` includes
  to `~/.local/include/`; same `PROTOC` requirement disclosed for
  WO-0053 / WO-0064).
- `cargo clippy --all-targets -p ucil-embeddings -- -D warnings` →
  exit 0 (no warnings introduced; the `clippy::cast_*` triplet on
  the `ceil_sqrt_u32` helper is `#[allow]`-ed with an inline
  rationale comment).
- `cargo fmt --all -- --check` → exit 0.
- `cargo test --workspace --no-fail-fast` → exit 0 (workspace tests
  green after `bash scripts/devtools/install-coderankembed.sh` made
  the `ml/models/coderankembed/` artefacts available to the
  `models::test_coderankembed_inference` frozen acceptance test).
- `bash scripts/gate/phase-1.sh` → see "Acceptance criteria
  pass/fail map" below.

### Bench wall-time breakdown

- Cold-cache build (`cargo build --release`) — ~10 min on this
  machine (lance + lancedb + datafusion 44 transitive build, ~600
  Rust crate compilation units, including the protoc-driven
  build-script step).  Verifier-side amortisation: subsequent
  invocations land sub-second per `cargo build`'s incremental cache.
- Bench measurement loop — ~22 s (criterion 100 samples × ~3 ms +
  warm-up 2 s + measurement 20 s).
- p95 parse — instantaneous (single `jq` pipeline).

### Acceptance criteria pass-fail map

| AC  | Check                                                                                              | Result |
|-----|----------------------------------------------------------------------------------------------------|--------|
| AC01 | `Cargo.toml` adds `[dev-dependencies]` `lancedb`, `arrow-array`, `arrow-schema` (workspace pins)  | OK (`fb55164`) |
| AC02 | `Cargo.toml` registers `[[bench]] name = "vector_query" harness = false` after `throughput`       | OK (`fb55164`) |
| AC03 | `crates/ucil-embeddings/benches/vector_query.rs` exists                                            | OK (`d65dd16`) |
| AC04 | `pub const CORPUS_SIZE: usize = 2000;` / `QUERY_COUNT: usize = 20;` / `EMBEDDING_DIM: usize = 768;` | OK |
| AC05 | `group.bench_function("vector_query_p95_warm", ...)` literal at criterion call site              | OK |
| AC06 | `tempdir()` + `lancedb::connect` + `create_empty_table` + `Table::add` + `IvfHnswPq` create_index | OK |
| AC07 | Per-iter `nearest_to(...).unwrap().limit(10).execute().await` + `black_box` on result            | OK |
| AC08 | Literal RNG seeds `0x_C0DE_BABE` (corpus) + `0x_FACE_FEED` (queries)                              | OK |
| AC09 | `sample_size(100)`, `warm_up_time(Duration::from_secs(2))`, `measurement_time(Duration::from_secs(20))` | OK |
| AC10 | `cargo build --release -p ucil-embeddings --bench vector_query`                                   | OK |
| AC11 | `cargo bench` produces `target/criterion/vector_query_p95_warm/vector_query_p95_warm/new/sample.json` | OK |
| AC12 | `scripts/bench-vector-query.sh` exists, executable, `#!/usr/bin/env bash` + `set -euo pipefail` + `IFS` | OK (`8c0a230`) |
| AC13 | bench-script does NOT contain literal `install-coderankembed`                                     | OK |
| AC14 | bench-script reads `sample.json`, computes `mean_ns_per_iter[i] = times[i]/iters[i]`, sorts, p95  | OK |
| AC15 | bench-script emits `p95_warm_ms=<N>` matching `^[0-9]+(\.[0-9]+)?$`                               | OK |
| AC16 | bench-script asserts wall-time floor `MEAN_NS_PER_ITER >= 100_000`                                | OK |
| AC17 | bench-script asserts p95 floor `p95_warm_ms < 100`                                                | OK |
| AC18 | `scripts/verify/P2-W8-F07.sh` exists, executable                                                  | OK (`8c0a230`) |
| AC19 | verify-script greps `^[[:space:]]*group\.bench_function\("vector_query_p95_warm"`                | OK |
| AC20 | verify-script INDEPENDENTLY parses stdout + asserts `< 100`                                       | OK |
| AC21 | `bash scripts/verify/P2-W8-F07.sh` exits 0; prints `[OK] P2-W8-F07 p95_warm_ms=<N>`               | OK |
| AC22 | `cargo clippy --all-targets -p ucil-embeddings -- -D warnings`                                    | OK |
| AC23 | `cargo fmt --all -- --check`                                                                      | OK |
| AC24 | `cargo test --workspace --no-fail-fast`                                                           | OK |
| AC25 | `bash scripts/gate/phase-1.sh`                                                                    | OK (clippy + workspace test green; downstream MCP/Serena/effectiveness checks unchanged) |
| AC26 | Coverage gate INFORMATIONAL: `cargo llvm-cov --package ucil-embeddings --summary-only --json` ≥ 85 | INFORMATIONAL — bench file is outside `cargo llvm-cov` scope; existing src/ coverage unchanged |
| AC27 | Word-ban `rg -i 'mock\|fake\|stub\|fixture' crates/ucil-embeddings/benches/vector_query.rs` zero matches | OK |
| AC28 | Conventional Commits + Phase/Feature/Work-order trailers + Co-Authored-By                         | OK (3 implementation commits + 1 RFR commit; all under 4-soft target estimate) |
| AC29 | All commits pushed to `origin/feat/WO-0065-vector-query-p95-bench`                                | OK |
| AC30 | M1 mutation (verifier-applied)                                                                     | DELEGATED to verifier per WO-0061 line 690 — see "Mutation execution log" below |
| AC31 | M2 mutation (verifier-applied)                                                                     | DELEGATED to verifier |
| AC32 | M3 mutation (verifier-applied)                                                                     | DELEGATED to verifier |

## Mutation execution log

Per WO-0065 scope_in[16] + WO-0061 line 690 standing precedent:
mutations are delegated to the verifier (no commit-then-revert
in-line).  Below documents the literal edit, expected failure mode,
and restoration command for each.

### M1 — neutered `nearest_to(...).execute()` body

Edit `crates/ucil-embeddings/benches/vector_query.rs` lines around
the criterion `b.iter` body.  Replace:

```rust
            runtime.block_on(async {
                let stream = table
                    .query()
                    .nearest_to(query_vec)
                    .expect("VectorQuery::nearest_to on synthetic 768-d query vector")
                    .limit(10)
                    .execute()
                    .await
                    .expect("ExecutableQuery::execute on warm IVF/HNSW index");
                let _ = black_box(stream);
            });
```

with:

```rust
            let _ = black_box(query_vec);
```

(Drops the actual LanceDB call; the per-iter body becomes a single
`black_box` of the vector slice reference.)

**Expected failure**: `bash scripts/bench-vector-query.sh` exits 1
with `[FAIL] wall-time floor breached: MEAN_NS_PER_ITER=<sub-1000>
< 100000 (1e5)` on stderr.  The per-iter body is sub-microsecond
because there is no LanceDB I/O.

**Restoration**: `git checkout -- crates/ucil-embeddings/benches/vector_query.rs`.

### M2 — `QUERY_COUNT` shrunk to 1

Edit `crates/ucil-embeddings/benches/vector_query.rs`.  Replace:

```rust
pub const QUERY_COUNT: usize = 20;
```

with:

```rust
pub const QUERY_COUNT: usize = 1;
```

**Expected failure**: `bash scripts/bench-vector-query.sh` exits 1
with `[FAIL] wall-time floor breached: ...` on stderr (LanceDB caches
the single repeated query result; per-iter time drops below the
100 µs floor).  Note: depending on LanceDB's caching behaviour the
mean may stay above 100 µs but the p95 still drops markedly — the
wall-time floor is the load-bearing sentinel here.

**Restoration**: `git checkout -- crates/ucil-embeddings/benches/vector_query.rs`.

### M3 — bench-script p95 floor neutered

Edit `scripts/bench-vector-query.sh`.  Replace:

```bash
P95_FLOOR=100
```

with:

```bash
P95_FLOOR=1000000
```

(Bench-script's threshold is now effectively unbounded.)

**Expected failure mode** (per WO-0065 scope_in[16] M3 description):
the bench script passes (false green) BUT
`scripts/verify/P2-W8-F07.sh` INDEPENDENTLY re-asserts `< 100` and
catches the violation IF `<N>` exceeds 100.  In the green case (this
machine: `<N> ≈ 0.3` ms) M3 alone does not flip green→red because
real `<N>` is well under 100; M3 is verified by INSPECTION that the
verify script's p95 assertion is independently coded — i.e. NOT a
`set +e ; bench-script ; exit $?` shell.  Confirm by reading
`scripts/verify/P2-W8-F07.sh` lines 67-83 (Step 4 + Step 5): the
verify script parses `^p95_warm_ms=<N>$` from captured stdout and
runs its own `awk "BEGIN { exit !(<N> >= 100) }"` assertion.

**Restoration**: `git checkout -- scripts/bench-vector-query.sh`.

## Cargo.lock diff summary

```
$ git diff a0e9919..HEAD -- Cargo.lock | head -20
diff --git a/Cargo.lock b/Cargo.lock
index fb2fecd..4f06019 100644
--- a/Cargo.lock
+++ b/Cargo.lock
@@ -6678,9 +6678,13 @@ dependencies = [
 name = "ucil-embeddings"
 version = "0.1.0"
 dependencies = [
+ "arrow-array",
+ "arrow-schema",
  "criterion",
+ "lancedb",
  "ndarray 0.16.1",
  "ort",
+ "rand 0.8.5",
  "serde",
  "tokenizers",
```

**Total**: +4 / -0 lines.  Zero new transitive crates — `lancedb`,
`arrow-array`, `arrow-schema`, and `rand 0.8.5` were ALREADY in
`Cargo.lock` (resolved transitively via `ucil-daemon`'s `lancedb`
workspace dep + `tokenizers`'s and `lance`'s transitive `rand`).
Confirms WO-0065 scope_in[1] expectation of "near-zero churn".

## p95_warm_ms reading

Final number on this machine: `p95_warm_ms=0.296` from
`bash scripts/bench-vector-query.sh` (cited above, "What I verified
locally").  The verify-script wrapper re-asserts the same value
independently and prints `[OK] P2-W8-F07 p95_warm_ms=0.266` (slight
re-run noise).  Both readings are ~3 orders of magnitude below the
100 ms master-plan §18 line 1789 floor.

## Coverage gate output

INFORMATIONAL ONLY per AC26 + WO-0061 line 688 (bench-only WO;
benches/ is a separate compilation unit outside `cargo llvm-cov`
crate-coverage scope, so adding a bench cannot regress src/
coverage).  `crates/ucil-embeddings/src/**` was not touched by this
WO (`forbidden_paths` includes `crates/ucil-embeddings/src/**`); the
existing line=89% from WO-0062 RFR + WO-0061 RFR remains accurate.

## Disclosed deviations

1. **Added `rand = "0.8"` as dev-dep** beyond the three deps listed
   in WO-0065 scope_in[1].  Required by AC08 — `StdRng::seed_from_u64(...)`
   needs the `rand` crate.  Pinned `0.8` to the line already in
   `Cargo.lock` (transitive via `lance`/`tokenizers`); zero new-crate
   churn.  Documented inline in `crates/ucil-embeddings/Cargo.toml`.

2. **`PROTOC` toolchain dependency**: `lancedb 0.16` transitively
   depends on `lance-encoding` and `lance-file`, both of which run a
   `prost-build`-driven build script that requires `protoc` on the
   build host.  This machine did not have `protobuf-compiler`
   installed; downloaded the v25.1 release zip from
   `protocolbuffers/protobuf` and unpacked `bin/protoc` →
   `~/.local/bin/protoc` and `include/google/protobuf/*.proto` →
   `~/.local/include/`, then exported `PROTOC` + `PROTOC_INCLUDE`
   for every `cargo` invocation.  Pre-existing dependency disclosed
   in WO-0053 critic report (lines 71-78); same posture for WO-0065.
   Verifier needs `protoc` on PATH (or `PROTOC`/`PROTOC_INCLUDE`
   exported) to reproduce.

3. **Inline `IndexBuilder` setter style** uses
   `IvfHnswPqIndexBuilder::default().num_partitions(45)` instead of a
   struct-literal-with-`Default::default()` spread.  The
   `IvfHnswPqIndexBuilder`'s fields are `pub(crate)` (lancedb 0.16
   `src/index/vector.rs:280-292`), so struct-literal construction is
   not available outside the crate; the builder's `num_partitions()`
   setter (macro-generated via `impl_ivf_params_setter!`) is the only
   public API path.  Documented inline in the bench source.

## Cited precedents

- `DEC-0005` — module-coherence carve-out for single-commit > 200 LOC
  bench files (cited in `feat(embeddings)` commit body).
- `DEC-0007` — frozen-test-at-module-root (load-bearing-frozen-bench-id
  is the bench-precedent equivalent).
- `DEC-0008` — UCIL-internal-trait-boundaries (no critical-dep mocks;
  LanceDB exercised end-to-end via the real client).
- `DEC-0016` §Closed — orphan-branch resolved 2026-05-07 commit
  `57e50ab`; F07 unblocked.
- WO-0061 line 681 — frozen-bench-id load-bearing (criterion's nested
  `<group>/<bench>/new/` output path).
- WO-0061 line 685 — canonical bench shape (`[[bench]]` table +
  `harness = false` + `criterion 0.5` defaults).
- WO-0061 line 686 — pre-flight word-ban grep on `.rs` files only;
  shell scripts exempt.
- WO-0061 line 689 — `.expect()` carve-out in `crates/*/benches/*.rs`.
- WO-0061 line 690 — mutation-execution delegated to verifier.
- WO-0061 line 697 — single-commit > 200 LOC for criterion bench
  files acceptable under DEC-0005.
- WO-0061 line 679 — defence-in-depth dual asserter (bench-script +
  verify-script independently re-check the threshold).
- WO-0061 line 680 — wall-time floor as single mock-shape sentinel.
- WO-0058 / WO-0059 / WO-0060 / WO-0061 — Cargo.lock churn protocol.
- WO-0060 line 644 — upstream-API research checklist (applied for
  `lancedb 0.16` `IvfHnswPqIndexBuilder` API + `Query::nearest_to`
  return shape).
- WO-0042 line 80 — pre-bake mutation checks naming specific call
  sites and constants.
