#!/usr/bin/env bash
set -euo pipefail
IFS=$'\n\t'
# Throughput asserter for `CodeRankEmbed::embed` — `P2-W8-F06` /
# `WO-0061`.  Master-plan §4.2 line 303 ("CodeRankEmbed ... CPU-friendly,
# 50-150 embeddings/sec") + §18 Phase 2 Week 8 line 1789 ("Benchmark:
# embedding throughput, query latency, recall@10").
#
# Behaviour:
#   1. cd to repo root via `git rev-parse --show-toplevel`.
#   2. Pre-flight `bash scripts/devtools/install-coderankembed.sh`
#      (idempotent — fast path when artefacts already present).
#   3. Warm the build cache via
#      `cargo build --release -p ucil-embeddings --bench throughput`
#      so the first criterion sample does not pay the build-cache
#      miss (debug-build first sample can dominate the measurement).
#   4. Run `cargo bench -p ucil-embeddings --bench throughput` over
#      the criterion harness with explicit `--noplot`,
#      `--warm-up-time 1`, `--measurement-time 5`, `--sample-size 10`,
#      target the `embed_100_snippets` group only.
#   5. Read `target/criterion/embed_100_snippets/embed_100_snippets/
#      new/estimates.json` (criterion's nested
#      `<group>/<bench>/new/estimates.json` shape).
#   6. Use `jq` to extract `.mean.point_estimate` (ns per outer
#      iteration of 100 model.embed calls).
#   7. Compute `cpu_emb_per_sec = 100 / (mean_ns / 1e9)`.  Equivalent:
#      `cpu_emb_per_sec = 1e11 / mean_ns`.
#   8. Print `cpu_emb_per_sec=<N>` to stdout (one line, exact prefix —
#      the verify script greps for this line).
#   9. Wall-time floor: assert `MEAN_NS >= 1_000_000_000` (1 second per
#      iteration of 100 snippets — i.e. ≤100/sec ceiling on plausible
#      CPU throughput; below this the bench is implausibly fast and
#      likely mock-shaped).  Fail with `[FAIL] wall-time floor breached`
#      on stderr.
#  10. Throughput floor: assert `cpu_emb_per_sec >= 50` per master-plan
#      §4.2 line 303.  Fail with `[FAIL] throughput floor breached` on
#      stderr.
#
# Pre-baked mutation contract (per WO-0061 scope_in):
# - Mutation #1 (`CodeRankEmbed::embed` body neutered): no-op return
#   makes 100 calls sub-millisecond → wall-time floor (#9) FAILS.
# - Mutation #2 (`SNIPPETS` array shrunk to 1 element): inner loop
#   runs 1 call instead of 100 → wall-time floor (#9) FAILS (single
#   ~5-15ms call ≪ 1s).
# - Mutation #3 (this script's `>= 50` neutered to `>= 0`): the verify
#   script `scripts/verify/P2-W8-F06.sh` re-checks `>= 50` independently;
#   this script is NOT the authoritative asserter.

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "${REPO_ROOT}"

CRITERION_DIR="target/criterion/embed_100_snippets/embed_100_snippets"
ESTIMATES_JSON="${CRITERION_DIR}/new/estimates.json"

# ── Step 1: idempotent install of model artefacts ─────────────────────
echo "[INFO] P2-W8-F06: pre-flight install-coderankembed.sh..."
if ! bash scripts/devtools/install-coderankembed.sh >/dev/null; then
    rc=$?
    echo "[FAIL] P2-W8-F06: install-coderankembed.sh exited ${rc}" >&2
    echo "[HINT] P2-W8-F06: re-run \`bash -x scripts/devtools/install-coderankembed.sh\` to inspect the curl/sha256 step." >&2
    exit 1
fi

# ── Step 2: warm the build cache (release profile) ────────────────────
echo "[INFO] P2-W8-F06: warming build cache (release)..."
if ! cargo build --release -p ucil-embeddings --bench throughput >/dev/null 2>&1; then
    echo "[FAIL] P2-W8-F06: cargo build --release --bench throughput failed" >&2
    cargo build --release -p ucil-embeddings --bench throughput 2>&1 | tail -40 >&2 || true
    exit 1
fi

# ── Step 3: clean stale criterion data so estimates.json is current ───
rm -rf "${CRITERION_DIR}/new" 2>/dev/null || true

# ── Step 4: run the criterion bench ───────────────────────────────────
echo "[INFO] P2-W8-F06: running cargo bench --bench throughput..."
BENCH_LOG="$(mktemp -t bench-embed-throughput.XXXXXX.log)"
trap 'rm -f "${BENCH_LOG}"' EXIT
if ! cargo bench -p ucil-embeddings --bench throughput -- \
        --noplot --warm-up-time 1 --measurement-time 5 --sample-size 10 \
        embed_100_snippets > "${BENCH_LOG}" 2>&1; then
    echo "[FAIL] P2-W8-F06: cargo bench failed" >&2
    tail -60 "${BENCH_LOG}" >&2 || true
    exit 1
fi

# ── Step 5: read criterion's mean estimate ────────────────────────────
if [ ! -s "${ESTIMATES_JSON}" ]; then
    echo "[FAIL] P2-W8-F06: criterion estimates.json missing at ${ESTIMATES_JSON}" >&2
    echo "[HINT] P2-W8-F06: bench log tail:" >&2
    tail -40 "${BENCH_LOG}" >&2 || true
    exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
    echo "[FAIL] P2-W8-F06: \`jq\` not on PATH; install jq to parse criterion's estimates.json" >&2
    exit 1
fi

MEAN_NS="$(jq -r '.mean.point_estimate' "${ESTIMATES_JSON}")"
if [ -z "${MEAN_NS}" ] || [ "${MEAN_NS}" = "null" ]; then
    echo "[FAIL] P2-W8-F06: jq could not extract .mean.point_estimate from ${ESTIMATES_JSON}" >&2
    head -40 "${ESTIMATES_JSON}" >&2 || true
    exit 1
fi

# ── Step 6: wall-time floor (1e9 ns = 1 s per 100-snippet outer iter) ─
# A no-op embed (mutation #1) would put MEAN_NS sub-millisecond; a
# 1-snippet array (mutation #2) would put MEAN_NS at ~5-15ms.  Both
# breach this floor.  Real CodeRankEmbed at 50 emb/sec produces
# 100/50 = 2 seconds per iteration → MEAN_NS ≈ 2e9, well above floor.
WALL_TIME_FLOOR_NS=1000000000  # 1e9 nanoseconds = 1 second
if awk "BEGIN { exit !(${MEAN_NS} < ${WALL_TIME_FLOOR_NS}) }"; then
    echo "[FAIL] wall-time floor breached: MEAN_NS=${MEAN_NS} < ${WALL_TIME_FLOOR_NS} (1e9)" >&2
    echo "[HINT] P2-W8-F06: this is the mock-shape sentinel — see WO-0061 mutation #1/#2." >&2
    exit 1
fi

# ── Step 7: compute cpu_emb_per_sec = 100 / (MEAN_NS / 1e9) ───────────
# Equivalent: cpu_emb_per_sec = 1e11 / MEAN_NS.  Use awk for portable
# floating point arithmetic (bc may not be installed on minimal CI).
CPU_EMB_PER_SEC="$(awk -v m="${MEAN_NS}" 'BEGIN { printf "%.2f", 1e11 / m }')"

# ── Step 8: emit canonical stdout line ────────────────────────────────
echo "cpu_emb_per_sec=${CPU_EMB_PER_SEC}"

# ── Step 9: throughput floor (>= 50 per master-plan §4.2 line 303) ────
THROUGHPUT_FLOOR=50
if awk "BEGIN { exit !(${CPU_EMB_PER_SEC} < ${THROUGHPUT_FLOOR}) }"; then
    echo "[FAIL] throughput floor breached: cpu_emb_per_sec=${CPU_EMB_PER_SEC} < ${THROUGHPUT_FLOOR}" >&2
    echo "[HINT] P2-W8-F06: master-plan §4.2 line 303 requires cpu_emb_per_sec >= 50." >&2
    exit 1
fi

echo "[OK] P2-W8-F06: cpu_emb_per_sec=${CPU_EMB_PER_SEC} (>= ${THROUGHPUT_FLOOR})"
exit 0
