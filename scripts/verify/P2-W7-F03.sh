#!/usr/bin/env bash
# Acceptance test for P2-W7-F03 — G2 group-search RRF fusion
# (master-plan §5.2 lines 447-461 + §6.2 line 645 + §18 Phase 2 Week 7
# line 1781).  Authored under WO-0056 (supersedes the abandoned
# WO-0050 branch).
#
# Behaviour:
#   1. cd to the repo root via `git rev-parse --show-toplevel`.
#   2. Verify the frozen selector lives at the module root of
#      `crates/ucil-core/src/fusion.rs` per DEC-0007 — `fn
#      test_g2_rrf_weights` must be at top level (not inside
#      `mod tests {}`).
#   3. Run `cargo test -p ucil-core
#      fusion::test_g2_rrf_weights -- --nocapture` and tee the
#      output through the cargo-test / cargo-nextest summary regex
#      (shape established in WO-0038/0039/0042/0043/0044/0045/0046/
#      0047/0048).
#   4. On success print `[OK] P2-W7-F03` and exit 0; on any failure
#      print `[FAIL] P2-W7-F03: <reason>` and exit 1, including the
#      specific assertion line on test failure so operators can jump
#      straight to the panic without re-running.
set -euo pipefail
IFS=$'\n\t'

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "${REPO_ROOT}"

TEST_LOG="/tmp/wo-0056-test.log"

# ── Step 1: confirm the frozen selector lives at module root ──────────
SELECTOR_FILE="crates/ucil-core/src/fusion.rs"
if ! grep -nE '^fn test_g2_rrf_weights|^pub fn test_g2_rrf_weights' \
        "${SELECTOR_FILE}" > /dev/null; then
    echo "[FAIL] P2-W7-F03: frozen selector \`fn test_g2_rrf_weights\` \
not found at module root of ${SELECTOR_FILE}" >&2
    echo "[HINT] P2-W7-F03: per DEC-0007, the test must be at module \
root (NOT inside \`mod tests {}\`)" >&2
    exit 1
fi

# ── Step 2: run the frozen acceptance selector ────────────────────────
echo "[INFO] P2-W7-F03: running cargo test fusion::test_g2_rrf_weights..."
if ! cargo test -p ucil-core fusion::test_g2_rrf_weights -- --nocapture 2>&1 \
        | tee "${TEST_LOG}" >/dev/null; then
    echo "[FAIL] P2-W7-F03: cargo test fusion::test_g2_rrf_weights \
exited non-zero — see ${TEST_LOG}" >&2
    # Extract the panic assertion line if present so operators can
    # jump straight to the regression site.
    if grep -nE 'panicked at|assertion[ ]+failed' "${TEST_LOG}" >&2; then
        echo "[HINT] P2-W7-F03: see panic line above for the failing \
assertion" >&2
    fi
    tail -40 "${TEST_LOG}" >&2 || true
    exit 1
fi
if ! grep -Eq 'test result: ok\. .* 0 failed|[0-9]+ tests? passed' \
        "${TEST_LOG}"; then
    echo "[FAIL] P2-W7-F03: cargo test summary line missing in ${TEST_LOG}" >&2
    tail -40 "${TEST_LOG}" >&2 || true
    exit 1
fi

# ── Optional: shellcheck the script itself ─────────────────────────────
if command -v shellcheck >/dev/null 2>&1; then
    if ! shellcheck "${REPO_ROOT}/scripts/verify/P2-W7-F03.sh"; then
        echo "[FAIL] P2-W7-F03: shellcheck flagged the verify script" >&2
        exit 1
    fi
else
    echo "[INFO] P2-W7-F03: shellcheck not on PATH; skipping lint."
fi

echo "[OK] P2-W7-F03"
exit 0
