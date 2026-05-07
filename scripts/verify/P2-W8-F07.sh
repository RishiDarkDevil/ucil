#!/usr/bin/env bash
set -euo pipefail
IFS=$'\n\t'
# Acceptance test for P2-W8-F07 — Vector query latency benchmark
# (master-plan §18 Phase 2 Week 8 line 1789 "Benchmark: embedding
# throughput, query latency, recall@10").  Authored under WO-0065.
#
# Behaviour:
#   1. cd to repo root via `git rev-parse --show-toplevel`.
#   2. Frozen-script existence + executability check on
#      `scripts/bench-vector-query.sh` AND
#      `crates/ucil-embeddings/benches/vector_query.rs`.
#   3. Frozen-bench-name selector check: confirm the literal
#      `vector_query_p95_warm` bench identifier lives at the criterion
#      `group.bench_function(...)` call site in
#      `crates/ucil-embeddings/benches/vector_query.rs`.  Load-bearing —
#      the bench script reads
#      `target/criterion/vector_query_p95_warm/...` and the verifier
#      depends on the path resolution.
#   4. Run `bash scripts/bench-vector-query.sh` and capture its stdout.
#   5. INDEPENDENTLY assert that the captured stdout contains a line
#      matching `^p95_warm_ms=<N>$` where `<N> < 100` per master-plan
#      §18 line 1789.  Defence-in-depth — the bench script also
#      checks `< 100`, but per WO-0065 mutation #3 the bench script's
#      check could be neutered; this verify script is the AUTHORITATIVE
#      asserter.
#   6. On success print `[OK] P2-W8-F07 p95_warm_ms=<N>` and exit 0;
#      on any failure print `[FAIL] P2-W8-F07: <reason>` on stderr
#      and exit 1.

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "${REPO_ROOT}"

BENCH_SCRIPT="scripts/bench-vector-query.sh"
BENCH_RS="crates/ucil-embeddings/benches/vector_query.rs"

# ── Step 1: frozen-script existence + executable ──────────────────────
if [ ! -f "${BENCH_SCRIPT}" ] || [ ! -x "${BENCH_SCRIPT}" ]; then
    echo "[FAIL] P2-W8-F07: ${BENCH_SCRIPT} missing or not executable" >&2
    exit 1
fi

if [ ! -f "${BENCH_RS}" ]; then
    echo "[FAIL] P2-W8-F07: ${BENCH_RS} missing" >&2
    exit 1
fi

# ── Step 2: frozen-bench-name selector at criterion call site ─────────
if ! grep -nE '^[[:space:]]*group\.bench_function\("vector_query_p95_warm"' \
        "${BENCH_RS}" > /dev/null; then
    echo "[FAIL] P2-W8-F07: frozen bench identifier \`vector_query_p95_warm\` not found at \`group.bench_function(...)\` call in ${BENCH_RS}" >&2
    echo "[HINT] P2-W8-F07: per WO-0065 scope_in[7], the bench function literal name is load-bearing — the bench script reads \`target/criterion/vector_query_p95_warm/...\`." >&2
    exit 1
fi

# ── Step 3: run the bench script and capture stdout ───────────────────
echo "[INFO] P2-W8-F07: running ${BENCH_SCRIPT}..."
BENCH_STDOUT="$(mktemp -t verify-P2-W8-F07.XXXXXX.out)"
trap 'rm -f "${BENCH_STDOUT}"' EXIT
if ! bash "${BENCH_SCRIPT}" > "${BENCH_STDOUT}"; then
    rc=$?
    echo "[FAIL] P2-W8-F07: ${BENCH_SCRIPT} exited ${rc}" >&2
    tail -40 "${BENCH_STDOUT}" >&2 || true
    exit 1
fi

# ── Step 4: INDEPENDENTLY parse p95_warm_ms from captured stdout ──────
# The bench script emits `p95_warm_ms=<N>` on a single line.  Extract
# the canonical line; do not trust the bench script's own threshold
# check (per WO-0065 mutation #3, the bench script could be neutered to
# always pass).
P95_LINE="$(grep -E '^p95_warm_ms=[0-9]+(\.[0-9]+)?$' "${BENCH_STDOUT}" | tail -1)"
if [ -z "${P95_LINE}" ]; then
    echo "[FAIL] P2-W8-F07: bench script stdout did not contain a line matching \`^p95_warm_ms=<N>$\`" >&2
    cat "${BENCH_STDOUT}" >&2 || true
    exit 1
fi

P95_MS="${P95_LINE#p95_warm_ms=}"

# ── Step 5: AUTHORITATIVE < 100 assertion ─────────────────────────────
P95_FLOOR=100
if awk "BEGIN { exit !(${P95_MS} >= ${P95_FLOOR}) }"; then
    echo "[FAIL] P2-W8-F07 p95_warm_ms=${P95_MS} >= ${P95_FLOOR}" >&2
    echo "[HINT] P2-W8-F07: master-plan §18 line 1789 requires p95_warm_ms < 100." >&2
    exit 1
fi

echo "[OK] P2-W8-F07 p95_warm_ms=${P95_MS}"
exit 0
