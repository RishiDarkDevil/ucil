#!/usr/bin/env bash
# Acceptance test for P2-W8-F04 — LanceDB chunk indexer
# (master-plan §12.2 lines 1321-1346 + §18 Phase 2 Week 8 line 1789).
# WO-0064.
#
# Behaviour:
#   1. cd to the repo root via `git rev-parse --show-toplevel`.
#   2. Verify the frozen selectors live at MODULE ROOT of
#      `crates/ucil-daemon/src/executor.rs` per DEC-0007 — both
#      `pub async fn test_lancedb_incremental_indexing` and
#      `pub async fn test_lancedb_indexer_handle_processes_events`
#      must be at top level (not inside `mod tests {}`).
#   3. Run `cargo test -p ucil-daemon
#      executor::test_lancedb_incremental_indexing -- --nocapture`
#      and validate the cargo-test summary regex.
#   4. Run the companion handle test
#      `executor::test_lancedb_indexer_handle_processes_events`.
#   5. Run the WO-0053 regression
#      `branch_manager::test_lancedb_per_branch` to prove the
#      `pub fn sanitise_branch_name` promotion did not regress
#      P2-W7-F09 lifecycle behaviour.
#   6. shellcheck pass (PATH-fallback per WO-0044).
#
# On any failure print `[FAIL] P2-W8-F04: <reason>` to stderr and
# exit 1 (operator-readable diagnostic per WO-0051 lessons line 405).
set -euo pipefail
IFS=$'\n\t'

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "${REPO_ROOT}"

TEST_LOG_INC="/tmp/wo-0064-incremental.log"
TEST_LOG_HND="/tmp/wo-0064-handle.log"
TEST_LOG_BM="/tmp/wo-0064-branch-manager.log"

SELECTOR_FILE="crates/ucil-daemon/src/executor.rs"

# ── Step 1: confirm the frozen selectors live at module root ──────────
if ! grep -nE '^(pub )?async fn test_lancedb_incremental_indexing\(' \
        "${SELECTOR_FILE}" > /dev/null; then
    echo "[FAIL] P2-W8-F04: frozen selector \`pub async fn test_lancedb_incremental_indexing\` \
not found at module root of ${SELECTOR_FILE}" >&2
    echo "[HINT] P2-W8-F04: per DEC-0007 the test must be at module root \
(NOT inside \`mod tests {}\`)" >&2
    exit 1
fi
if ! grep -nE '^(pub )?async fn test_lancedb_indexer_handle_processes_events\(' \
        "${SELECTOR_FILE}" > /dev/null; then
    echo "[FAIL] P2-W8-F04: frozen selector \
\`pub async fn test_lancedb_indexer_handle_processes_events\` \
not found at module root of ${SELECTOR_FILE}" >&2
    echo "[HINT] P2-W8-F04: per DEC-0007 the test must be at module root \
(NOT inside \`mod tests {}\`)" >&2
    exit 1
fi

# ── Step 2: run the primary frozen acceptance selector ────────────────
echo "[INFO] P2-W8-F04: running cargo test executor::test_lancedb_incremental_indexing..."
if ! cargo test -p ucil-daemon executor::test_lancedb_incremental_indexing \
        -- --nocapture 2>&1 | tee "${TEST_LOG_INC}" >/dev/null; then
    echo "[FAIL] P2-W8-F04: cargo test executor::test_lancedb_incremental_indexing \
exited non-zero — see ${TEST_LOG_INC}" >&2
    if grep -nE 'panicked at|assertion[ ]+failed' "${TEST_LOG_INC}" >&2; then
        echo "[HINT] P2-W8-F04: see panic line above for the failing assertion" >&2
        echo "[HINT] P2-W8-F04: master-plan refs — §12.2 lines 1321-1346 (code_chunks \
schema), §18 Phase 2 Week 8 line 1789 (background chunk indexing)" >&2
    fi
    tail -40 "${TEST_LOG_INC}" >&2 || true
    exit 1
fi
if ! grep -Eq 'test result: ok\.|[0-9]+ tests? passed' "${TEST_LOG_INC}"; then
    echo "[FAIL] P2-W8-F04: cargo test summary line missing in ${TEST_LOG_INC}" >&2
    tail -40 "${TEST_LOG_INC}" >&2 || true
    exit 1
fi

# ── Step 3: run the companion handle frozen selector ──────────────────
echo "[INFO] P2-W8-F04: running cargo test executor::test_lancedb_indexer_handle_processes_events..."
if ! cargo test -p ucil-daemon \
        executor::test_lancedb_indexer_handle_processes_events \
        -- --nocapture 2>&1 | tee "${TEST_LOG_HND}" >/dev/null; then
    echo "[FAIL] P2-W8-F04: cargo test executor::test_lancedb_indexer_handle_processes_events \
exited non-zero — see ${TEST_LOG_HND}" >&2
    if grep -nE 'panicked at|assertion[ ]+failed' "${TEST_LOG_HND}" >&2; then
        echo "[HINT] P2-W8-F04: see panic line above for the failing assertion" >&2
    fi
    tail -40 "${TEST_LOG_HND}" >&2 || true
    exit 1
fi
if ! grep -Eq 'test result: ok\.|[0-9]+ tests? passed' "${TEST_LOG_HND}"; then
    echo "[FAIL] P2-W8-F04: cargo test summary line missing in ${TEST_LOG_HND}" >&2
    tail -40 "${TEST_LOG_HND}" >&2 || true
    exit 1
fi

# ── Step 4: P2-W7-F09 regression — sanitise_branch_name promotion ─────
echo "[INFO] P2-W8-F04: running WO-0053 regression branch_manager::test_lancedb_per_branch..."
if ! cargo test -p ucil-daemon branch_manager::test_lancedb_per_branch \
        -- --nocapture 2>&1 | tee "${TEST_LOG_BM}" >/dev/null; then
    echo "[FAIL] P2-W8-F04: WO-0053 regression branch_manager::test_lancedb_per_branch \
exited non-zero — pub-fn promotion of sanitise_branch_name regressed \
P2-W7-F09 lifecycle; see ${TEST_LOG_BM}" >&2
    tail -40 "${TEST_LOG_BM}" >&2 || true
    exit 1
fi
if ! grep -Eq 'test result: ok\.|[0-9]+ tests? passed' "${TEST_LOG_BM}"; then
    echo "[FAIL] P2-W8-F04: cargo test summary line missing in ${TEST_LOG_BM}" >&2
    tail -40 "${TEST_LOG_BM}" >&2 || true
    exit 1
fi

# ── Step 5: shellcheck PATH-fallback per WO-0044 standing pattern ─────
if command -v shellcheck >/dev/null 2>&1; then
    if ! shellcheck "${REPO_ROOT}/scripts/verify/P2-W8-F04.sh"; then
        echo "[FAIL] P2-W8-F04: shellcheck flagged the verify script" >&2
        exit 1
    fi
else
    echo "[INFO] P2-W8-F04: shellcheck not on PATH; skipping lint."
fi

echo "[OK] P2-W8-F04"
exit 0
