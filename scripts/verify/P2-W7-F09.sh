#!/usr/bin/env bash
# Acceptance test for P2-W7-F09 — LanceDB per-branch vector-store lifecycle
# (master-plan §6.4 line 144 + §11.2 line 1074 + §12.2 lines 1321-1346 +
#  §18 Phase 2 Week 7 line 1782).  WO-0053.
#
# Behaviour:
#   1. cd to the repo root via `git rev-parse --show-toplevel`.
#   2. Verify the frozen selector lives at the module root of
#      `crates/ucil-daemon/src/branch_manager.rs` per DEC-0007 — `async
#      fn test_lancedb_per_branch` must be at top level (not inside
#      `mod tests {}`).
#   3. Run `cargo test -p ucil-daemon
#      branch_manager::test_lancedb_per_branch -- --nocapture` and tee
#      the output through the cargo-test / cargo-nextest summary regex
#      established in WO-0042/0043/0044/0045/0046/0047/0048/0051.
#   4. On success print `[OK] P2-W7-F09` and exit 0; on any failure
#      print `[FAIL] P2-W7-F09: <reason>` and exit 1, including the
#      specific assertion line on test failure so operators can jump
#      straight to the panic without re-running.
set -euo pipefail
IFS=$'\n\t'

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "${REPO_ROOT}"

TEST_LOG="/tmp/wo-0053-test.log"

# ── Step 1: confirm the frozen selector lives at module root ──────────
SELECTOR_FILE="crates/ucil-daemon/src/branch_manager.rs"
if ! grep -nE '^async fn test_lancedb_per_branch\(' "${SELECTOR_FILE}" \
        > /dev/null; then
    echo "[FAIL] P2-W7-F09: frozen selector \`async fn test_lancedb_per_branch\` \
not found at module root of ${SELECTOR_FILE}" >&2
    echo "[HINT] P2-W7-F09: per DEC-0007, the test must be at module root \
(NOT inside \`mod tests {}\`)" >&2
    exit 1
fi

# ── Step 2: run the frozen acceptance selector ────────────────────────
echo "[INFO] P2-W7-F09: running cargo test branch_manager::test_lancedb_per_branch..."
if ! cargo test -p ucil-daemon branch_manager::test_lancedb_per_branch -- --nocapture 2>&1 \
        | tee "${TEST_LOG}" >/dev/null; then
    echo "[FAIL] P2-W7-F09: cargo test branch_manager::test_lancedb_per_branch \
exited non-zero — see ${TEST_LOG}" >&2
    # Extract the panic assertion line if present so operators can
    # jump straight to the regression site.
    if grep -nE 'panicked at|assertion[ ]+failed' "${TEST_LOG}" >&2; then
        echo "[HINT] P2-W7-F09: see panic line above for the failing assertion" >&2
        echo "[HINT] P2-W7-F09: master-plan refs — §6.4 line 144 (Branch index manager), \
§12.2 lines 1321-1346 (code_chunks schema)" >&2
    fi
    tail -40 "${TEST_LOG}" >&2 || true
    exit 1
fi
if ! grep -Eq 'test result: ok\. .* 0 failed|[0-9]+ tests? passed' \
        "${TEST_LOG}"; then
    echo "[FAIL] P2-W7-F09: cargo test summary line missing in ${TEST_LOG}" >&2
    tail -40 "${TEST_LOG}" >&2 || true
    exit 1
fi

# ── Optional: shellcheck the script itself (PATH-fallback per WO-0044) ─
if command -v shellcheck >/dev/null 2>&1; then
    if ! shellcheck "${REPO_ROOT}/scripts/verify/P2-W7-F09.sh"; then
        echo "[FAIL] P2-W7-F09: shellcheck flagged the verify script" >&2
        exit 1
    fi
else
    echo "[INFO] P2-W7-F09: shellcheck not on PATH; skipping lint."
fi

echo "[OK] P2-W7-F09"
exit 0
