#!/usr/bin/env bash
set -euo pipefail
IFS=$'\n\t'
# Acceptance test for P2-W8-F06 — CodeRankEmbed throughput
# benchmark (master-plan §4.2 line 303 "CPU-friendly, 50-150
# embeddings/sec" + §18 Phase 2 Week 8 line 1789 "Benchmark:
# embedding throughput, query latency, recall@10").  Authored
# under WO-0061.
#
# Behaviour:
#   1. cd to repo root via `git rev-parse --show-toplevel`.
#   2. Frozen-script existence + executability check on
#      `scripts/bench-embed-throughput.sh`.
#   3. Frozen-bench-name selector check: confirm the literal
#      `embed_100_snippets` bench identifier lives at the criterion
#      `bench_function(...)` call site in
#      `crates/ucil-embeddings/benches/throughput.rs`.  This
#      identifier is load-bearing — the bench script reads
#      `target/criterion/embed_100_snippets/...` and the verifier
#      depends on the path resolution.
#   4. Run `bash scripts/bench-embed-throughput.sh` and capture its
#      stdout.  The bench script's pre-flight runs the
#      install-coderankembed.sh idempotently so the model artefacts
#      are present.
#   5. INDEPENDENTLY assert that the captured stdout contains a line
#      matching `^cpu_emb_per_sec=<N>$` where `<N> >= 50` per master-
#      plan §4.2 line 303.  This is defence-in-depth — the bench
#      script also checks `>= 50`, but per WO-0061 mutation #3 the
#      bench script's check could be neutered; this verify script
#      is the AUTHORITATIVE asserter.
#   6. On success print `[OK] P2-W8-F06 cpu_emb_per_sec=<N>` and
#      exit 0; on any failure print `[FAIL] P2-W8-F06: <reason>`
#      on stderr and exit 1.

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "${REPO_ROOT}"

BENCH_SCRIPT="scripts/bench-embed-throughput.sh"
BENCH_RS="crates/ucil-embeddings/benches/throughput.rs"

# ── Step 1: frozen-script existence + executable ──────────────────────
if [ ! -f "${BENCH_SCRIPT}" ] || [ ! -x "${BENCH_SCRIPT}" ]; then
    echo "[FAIL] P2-W8-F06: ${BENCH_SCRIPT} missing or not executable" >&2
    exit 1
fi

# ── Step 2: frozen-bench-name selector at criterion call site ─────────
if ! grep -nE '^[[:space:]]*group\.bench_function\("embed_100_snippets"' \
        "${BENCH_RS}" > /dev/null; then
    echo "[FAIL] P2-W8-F06: frozen bench identifier \`embed_100_snippets\` not found at \`group.bench_function(...)\` call in ${BENCH_RS}" >&2
    echo "[HINT] P2-W8-F06: per WO-0061 scope_in, the bench function literal name is load-bearing — the bench script reads \`target/criterion/embed_100_snippets/...\`." >&2
    exit 1
fi

# ── Step 3: run the bench script and capture stdout ───────────────────
echo "[INFO] P2-W8-F06: running ${BENCH_SCRIPT}..."
BENCH_STDOUT="$(mktemp -t verify-P2-W8-F06.XXXXXX.out)"
trap 'rm -f "${BENCH_STDOUT}"' EXIT
if ! bash "${BENCH_SCRIPT}" > "${BENCH_STDOUT}"; then
    rc=$?
    echo "[FAIL] P2-W8-F06: ${BENCH_SCRIPT} exited ${rc}" >&2
    tail -40 "${BENCH_STDOUT}" >&2 || true
    exit 1
fi

# ── Step 4: INDEPENDENTLY parse cpu_emb_per_sec from captured stdout ──
# The bench script emits `cpu_emb_per_sec=<N>` on a single line.
# Extract the canonical line; do not trust the bench script's own
# threshold check (per WO-0061 mutation #3, the bench script could be
# neutered to always pass).
EMB_LINE="$(grep -E '^cpu_emb_per_sec=[0-9]+(\.[0-9]+)?$' "${BENCH_STDOUT}" | tail -1)"
if [ -z "${EMB_LINE}" ]; then
    echo "[FAIL] P2-W8-F06: bench script stdout did not contain a line matching \`^cpu_emb_per_sec=<N>$\`" >&2
    cat "${BENCH_STDOUT}" >&2 || true
    exit 1
fi

CPU_EMB_PER_SEC="${EMB_LINE#cpu_emb_per_sec=}"

# ── Step 5: AUTHORITATIVE >= 50 assertion ─────────────────────────────
THROUGHPUT_FLOOR=50
if awk "BEGIN { exit !(${CPU_EMB_PER_SEC} < ${THROUGHPUT_FLOOR}) }"; then
    echo "[FAIL] P2-W8-F06 cpu_emb_per_sec=${CPU_EMB_PER_SEC} < ${THROUGHPUT_FLOOR}" >&2
    echo "[HINT] P2-W8-F06: master-plan §4.2 line 303 requires cpu_emb_per_sec >= 50 on CPU." >&2
    exit 1
fi

echo "[OK] P2-W8-F06 cpu_emb_per_sec=${CPU_EMB_PER_SEC}"
exit 0
