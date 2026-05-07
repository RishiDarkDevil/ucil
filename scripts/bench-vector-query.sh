#!/usr/bin/env bash
set -euo pipefail
IFS=$'\n\t'
# Vector query latency asserter for `LanceDB`+IVF/HNSW — `P2-W8-F07` /
# `WO-0065`.  Master-plan §18 Phase 2 Week 8 line 1789 ("Benchmark:
# embedding throughput, query latency, recall@10") + §12.2 lines
# 1321-1346 (768-d FixedSizeList<Float32, 768> embedding column;
# "1M 128-dim vectors in 33ms" upper-bound).
#
# Mirror of `scripts/bench-embed-throughput.sh` (`WO-0061`):
#   1. cd to repo root via `git rev-parse --show-toplevel`.
#   2. NO model-artefact pre-flight (no devtools install step) — the
#      synthetic corpus does NOT need the production CodeRankEmbed
#      bundle.  This bench measures LanceDB+IVF/HNSW query latency
#      in isolation (query embeddings are deterministic-seed random
#      vectors).  Skipping the model bundle keeps the bench wall-time
#      bounded to lance-build + criterion measurement time.  AC13
#      asserts the substring is absent from this script.
#   3. Warm the build cache via
#      `cargo build --release -p ucil-embeddings --bench vector_query`
#      so the first criterion sample does not pay the build-cache miss.
#   4. Clean stale criterion data so the parser reads the fresh
#      `sample.json`.
#   5. Run `cargo bench -p ucil-embeddings --bench vector_query`.
#      Criterion config (sample_size=100, warm_up=2s, measurement=20s)
#      is in the bench source per the WO scope_in[9]; no CLI overrides.
#   6. Read
#      `target/criterion/vector_query_p95_warm/vector_query_p95_warm/
#      new/sample.json` (criterion's nested `<group>/<bench>/new/`
#      shape).  jq-extract `.times[]` and `.iters[]`, compute per-sample
#      `mean_ns_per_iter[i] = times[i] / iters[i]`, sort ascending,
#      take element at index `floor(0.95 * len)` for p95.
#   7. Convert p95 ns → milliseconds and print
#      `p95_warm_ms=<N>` to stdout (one line, exact prefix — verify
#      script greps for it).
#   8. WALL-TIME FLOOR: assert `MEAN_NS_PER_ITER >= 100_000` (100 µs
#      per iteration; below this is implausibly fast for a real
#      LanceDB+HNSW query and indicates a stubbed/no-op nearest_to
#      body).  Fail with `[FAIL] wall-time floor breached` on stderr.
#   9. P95 FLOOR: assert `p95_warm_ms < 100` per master-plan §18 line
#      1789 implicit (and §15.x query-duration histogram budget).
#      Fail with `[FAIL] p95 floor breached` on stderr.
#
# Pre-baked mutation contract (per WO-0065 scope_in):
# - Mutation #1 (`Table::query().nearest_to(...).execute()` body
#   neutered): per-iter call replaced with a no-op black_box of the
#   query vector.  Wall-time floor (#8) FAILS because the per-iter
#   body becomes sub-microsecond.
# - Mutation #2 (`QUERY_COUNT` shrunk to 1): round-robin always picks
#   index 0, the engine caches the result, per-iter time drops below
#   the 100 µs wall-time floor (#8) FAILS.
# - Mutation #3 (this script's `< 100` neutered to `< 1000000`): the
#   verify script `scripts/verify/P2-W8-F07.sh` re-checks `< 100`
#   independently; this script is NOT the AUTHORITATIVE asserter for
#   the p95 threshold.

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "${REPO_ROOT}"

CRITERION_DIR="target/criterion/vector_query_p95_warm/vector_query_p95_warm"
SAMPLE_JSON="${CRITERION_DIR}/new/sample.json"

# ── Step 1: warm the build cache (release profile) ────────────────────
echo "[INFO] P2-W8-F07: warming build cache (release)..."
if ! cargo build --release -p ucil-embeddings --bench vector_query >/dev/null 2>&1; then
    echo "[FAIL] P2-W8-F07: cargo build --release --bench vector_query failed" >&2
    cargo build --release -p ucil-embeddings --bench vector_query 2>&1 | tail -40 >&2 || true
    exit 1
fi

# ── Step 2: clean stale criterion data so sample.json is current ──────
rm -rf "${CRITERION_DIR}/new" 2>/dev/null || true

# ── Step 3: run the criterion bench ───────────────────────────────────
echo "[INFO] P2-W8-F07: running cargo bench --bench vector_query..."
BENCH_LOG="$(mktemp -t bench-vector-query.XXXXXX.log)"
trap 'rm -f "${BENCH_LOG}"' EXIT
if ! cargo bench -p ucil-embeddings --bench vector_query > "${BENCH_LOG}" 2>&1; then
    echo "[FAIL] P2-W8-F07: cargo bench failed" >&2
    tail -60 "${BENCH_LOG}" >&2 || true
    exit 1
fi

# ── Step 4: read criterion's per-sample times + iters ─────────────────
if [ ! -s "${SAMPLE_JSON}" ]; then
    echo "[FAIL] P2-W8-F07: criterion sample.json missing at ${SAMPLE_JSON}" >&2
    echo "[HINT] P2-W8-F07: bench log tail:" >&2
    tail -40 "${BENCH_LOG}" >&2 || true
    exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
    echo "[FAIL] P2-W8-F07: \`jq\` not on PATH; install jq to parse criterion's sample.json" >&2
    exit 1
fi

# Criterion's sample.json shape: { "sampling_mode": "...", "iters":
# [<f64>, ...], "times": [<f64 nanoseconds>, ...] }.  The arrays are
# parallel and equal in length (≥50 by criterion's sample_size=100
# config).  Tolerates additional top-level fields (defensive against
# minor criterion-version drift; we only need iters + times).
TIMES_JSON="$(jq -c '.times' "${SAMPLE_JSON}")"
ITERS_JSON="$(jq -c '.iters' "${SAMPLE_JSON}")"
SAMPLE_LEN="$(jq -r '.times | length' "${SAMPLE_JSON}")"

if [ -z "${SAMPLE_LEN}" ] || [ "${SAMPLE_LEN}" = "null" ] || [ "${SAMPLE_LEN}" -lt 50 ]; then
    echo "[FAIL] P2-W8-F07: criterion sample.json has too few samples (${SAMPLE_LEN}) — need ≥50" >&2
    head -40 "${SAMPLE_JSON}" >&2 || true
    exit 1
fi

# ── Step 5: compute per-sample mean_ns_per_iter[i] = times[i]/iters[i],
# sort ascending, take the floor(0.95 * len) element as p95 ────────────
P95_NS="$(jq -r --argjson times "${TIMES_JSON}" --argjson iters "${ITERS_JSON}" '
  ($times | length) as $n
  | [range(0; $n) | ($times[.] / $iters[.])]
  | sort
  | .[(($n | tonumber) * 0.95 | floor)]
' "${SAMPLE_JSON}")"

# Mean per-iter (ns) — average of per-sample mean_ns_per_iter values.
# Used for the wall-time floor check.  Awk for portable float math.
MEAN_NS_PER_ITER="$(jq -r --argjson times "${TIMES_JSON}" --argjson iters "${ITERS_JSON}" '
  ($times | length) as $n
  | [range(0; $n) | ($times[.] / $iters[.])]
  | add / $n
' "${SAMPLE_JSON}")"

if [ -z "${P95_NS}" ] || [ "${P95_NS}" = "null" ]; then
    echo "[FAIL] P2-W8-F07: jq could not compute p95 from times+iters arrays" >&2
    head -40 "${SAMPLE_JSON}" >&2 || true
    exit 1
fi

# ── Step 6: wall-time floor (100_000 ns = 100 µs per iteration) ───────
# A no-op nearest_to body (mutation #1) puts MEAN_NS_PER_ITER
# sub-microsecond; a 1-element QUERY_COUNT (mutation #2) lets the
# engine cache the result.  Both breach this floor.  Real LanceDB+HNSW
# at N=2000 typically delivers ~0.5-3ms per query (500K-3M ns),
# WELL above 100K.
WALL_TIME_FLOOR_NS=100000  # 1e5 nanoseconds = 100 microseconds
if awk "BEGIN { exit !(${MEAN_NS_PER_ITER} < ${WALL_TIME_FLOOR_NS}) }"; then
    echo "[FAIL] wall-time floor breached: MEAN_NS_PER_ITER=${MEAN_NS_PER_ITER} < ${WALL_TIME_FLOOR_NS} (1e5)" >&2
    echo "[HINT] P2-W8-F07: the bench-body sentinel — see WO-0065 mutation #1/#2." >&2
    exit 1
fi

# ── Step 7: convert p95 ns → ms and emit canonical stdout line ────────
# awk for portable float math (bc may not be installed on minimal CI).
P95_MS="$(awk -v ns="${P95_NS}" 'BEGIN { printf "%.3f", ns / 1e6 }')"
echo "p95_warm_ms=${P95_MS}"

# ── Step 8: p95 floor (< 100 ms per master-plan §18 line 1789) ────────
P95_FLOOR=100
if awk "BEGIN { exit !(${P95_MS} >= ${P95_FLOOR}) }"; then
    echo "[FAIL] p95 floor breached: p95_warm_ms=${P95_MS} >= ${P95_FLOOR}" >&2
    echo "[HINT] P2-W8-F07: master-plan §18 line 1789 requires p95_warm_ms < 100." >&2
    exit 1
fi

echo "[OK] P2-W8-F07: p95_warm_ms=${P95_MS} (< ${P95_FLOOR})"
exit 0
