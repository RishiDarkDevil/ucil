#!/usr/bin/env bash
# Acceptance test for P2-W7-F01 — G1 parallel-execution orchestrator
# (master-plan §5.1 lines 420-446 + §18 Phase 2 Week 7 line 1780).
#
# Behaviour:
#   1. cd to the repo root via `git rev-parse --show-toplevel`.
#   2. Run `cargo test -p ucil-daemon
#      executor::test_g1_parallel_execution -- --nocapture` and tee
#      the output through the cargo-test / cargo-nextest summary
#      regex established in WO-0038/0039/0042/0043/0044/0045/0046.
#   3. Verify the frozen selector lives at the module root of
#      `crates/ucil-daemon/src/executor.rs` per DEC-0007 — `pub
#      async fn test_g1_parallel_execution` must be at the top
#      level (not inside `mod tests {}`).
#   4. On success print `[OK] P2-W7-F01` and exit 0; on any failure
#      print `[FAIL] P2-W7-F01: <reason>` and exit 1, including the
#      specific assertion line on test failure so operators can
#      jump to the panic without re-running.
set -euo pipefail
IFS=$'\n\t'

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "${REPO_ROOT}"

TEST_LOG="/tmp/wo-0047-test.log"

# ── Step 1: confirm the frozen selector lives at module root ──────────
SELECTOR_FILE="crates/ucil-daemon/src/executor.rs"
if ! grep -nE '^pub async fn test_g1_parallel_execution' "${SELECTOR_FILE}" \
        > /dev/null; then
    echo "[FAIL] P2-W7-F01: frozen selector \`pub async fn test_g1_parallel_execution\` \
not found at module root of ${SELECTOR_FILE}" >&2
    echo "[HINT] P2-W7-F01: per DEC-0007, the test must be at module root \
(NOT inside \`mod tests {}\`)" >&2
    exit 1
fi

# ── Step 2: run the frozen acceptance selector ────────────────────────
echo "[INFO] P2-W7-F01: running cargo test executor::test_g1_parallel_execution..."
if ! cargo test -p ucil-daemon executor::test_g1_parallel_execution -- --nocapture 2>&1 \
        | tee "${TEST_LOG}" >/dev/null; then
    echo "[FAIL] P2-W7-F01: cargo test executor::test_g1_parallel_execution \
exited non-zero — see ${TEST_LOG}" >&2
    # Extract the panic assertion line if present so operators can
    # jump straight to the regression site.
    if grep -nE 'panicked at|assertion[ ]+failed' "${TEST_LOG}" >&2; then
        echo "[HINT] P2-W7-F01: see panic line above for the failing assertion" >&2
    fi
    tail -40 "${TEST_LOG}" >&2 || true
    exit 1
fi
if ! grep -Eq 'test result: ok\. [0-9]+ passed; 0 failed|[0-9]+ tests run: [0-9]+ passed' \
        "${TEST_LOG}"; then
    echo "[FAIL] P2-W7-F01: cargo test summary line missing in ${TEST_LOG}" >&2
    tail -40 "${TEST_LOG}" >&2 || true
    exit 1
fi

# ── Optional: shellcheck the script itself ─────────────────────────────
if command -v shellcheck >/dev/null 2>&1; then
    if ! shellcheck "${REPO_ROOT}/scripts/verify/P2-W7-F01.sh"; then
        echo "[FAIL] P2-W7-F01: shellcheck flagged the verify script" >&2
        exit 1
    fi
else
    echo "[INFO] P2-W7-F01: shellcheck not on PATH; skipping lint."
fi

echo "[OK] P2-W7-F01"
exit 0
