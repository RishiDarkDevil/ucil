#!/usr/bin/env bash
# Acceptance test for P2-W7-F06 — search_code G2 fan-out + weighted
# RRF fusion (master-plan §3.2 row 4 / §5.2 lines 447-461 / §18
# Phase 2 Week 7 line 1786).
#
# Behaviour:
#   1. cd to the repo root via `git rev-parse --show-toplevel`.
#   2. Verify the frozen selector lives at the module root of
#      `crates/ucil-daemon/src/server.rs` per DEC-0007 —
#      `pub async fn test_search_code_fused` must be at top level
#      (not inside `mod tests {}`).
#   3. Run `cargo test -p ucil-daemon server::test_search_code_fused
#      -- --nocapture` and tee the output through the cargo-test /
#      cargo-nextest summary regex per the WO-0042 alternation
#      pattern.
#   4. Run the no-factory negative path
#      `server::test_search_code_fused_no_factory` to assert the
#      Option::None path is byte-identical legacy behaviour per
#      DEC-0015 D1.
#   5. Run the WO-0035 / P1-W5-F09 frozen regression
#      `server::test_search_code_basic` to assert the legacy envelope
#      shape is preserved verbatim per DEC-0015 D1.
#   6. On success print `[OK] P2-W7-F06` and exit 0; on any failure
#      print `[FAIL] P2-W7-F06: <reason>` and exit 1.
set -euo pipefail
IFS=$'\n\t'

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "${REPO_ROOT}"

TEST_LOG="/tmp/wo-0063-test.log"

# WO-0042 alternation regex covering both `cargo test` and
# `cargo nextest` summary line shapes.
SUMMARY_REGEX='test result: ok\. .* 0 failed|[0-9]+ tests? passed'

# ── Step 1: confirm the frozen selector lives at module root ──────────
SELECTOR_FILE="crates/ucil-daemon/src/server.rs"
if ! grep -nE '^pub async fn test_search_code_fused' "${SELECTOR_FILE}" \
        > /dev/null; then
    echo "[FAIL] P2-W7-F06: frozen selector \`pub async fn test_search_code_fused\` \
not found at module root of ${SELECTOR_FILE}" >&2
    echo "[HINT] P2-W7-F06: per DEC-0007, the test must be at module root \
(NOT inside \`mod tests {}\`)" >&2
    exit 1
fi

# ── Step 2: run the frozen acceptance selector ────────────────────────
echo "[INFO] P2-W7-F06: running cargo test server::test_search_code_fused..."
if ! cargo test -p ucil-daemon server::test_search_code_fused -- --nocapture 2>&1 \
        | tee "${TEST_LOG}" >/dev/null; then
    echo "[FAIL] P2-W7-F06: cargo test server::test_search_code_fused exited \
non-zero — see ${TEST_LOG}" >&2
    if grep -nE 'panicked at|assertion[ ]+failed' "${TEST_LOG}" >&2; then
        echo "[HINT] P2-W7-F06: see panic line above for the failing assertion" >&2
    fi
    tail -60 "${TEST_LOG}" >&2 || true
    exit 1
fi
if ! grep -Eq "${SUMMARY_REGEX}" "${TEST_LOG}"; then
    echo "[FAIL] P2-W7-F06: cargo test summary line missing in ${TEST_LOG}" >&2
    tail -40 "${TEST_LOG}" >&2 || true
    exit 1
fi

# ── Step 3: no-factory negative path (DEC-0015 D1 legacy preservation) ─
echo "[INFO] P2-W7-F06: running cargo test server::test_search_code_fused_no_factory..."
if ! cargo test -p ucil-daemon server::test_search_code_fused_no_factory \
        -- --nocapture 2>&1 \
        | tee "${TEST_LOG}" >/dev/null; then
    echo "[FAIL] P2-W7-F06: server::test_search_code_fused_no_factory exited \
non-zero — see ${TEST_LOG}" >&2
    tail -40 "${TEST_LOG}" >&2 || true
    exit 1
fi
if ! grep -Eq "${SUMMARY_REGEX}" "${TEST_LOG}"; then
    echo "[FAIL] P2-W7-F06: no-factory test summary line missing in ${TEST_LOG}" >&2
    tail -40 "${TEST_LOG}" >&2 || true
    exit 1
fi

# ── Step 4: WO-0035 / P1-W5-F09 frozen-shape regression ────────────────
echo "[INFO] P2-W7-F06: running cargo test server::test_search_code_basic (legacy regression)..."
if ! cargo test -p ucil-daemon server::test_search_code_basic -- --nocapture 2>&1 \
        | tee "${TEST_LOG}" >/dev/null; then
    echo "[FAIL] P2-W7-F06: legacy server::test_search_code_basic exited \
non-zero — see ${TEST_LOG}" >&2
    tail -40 "${TEST_LOG}" >&2 || true
    exit 1
fi
if ! grep -Eq "${SUMMARY_REGEX}" "${TEST_LOG}"; then
    echo "[FAIL] P2-W7-F06: legacy regression summary line missing in ${TEST_LOG}" >&2
    tail -40 "${TEST_LOG}" >&2 || true
    exit 1
fi

# ── Optional: shellcheck the script itself ─────────────────────────────
if command -v shellcheck >/dev/null 2>&1; then
    if ! shellcheck "${REPO_ROOT}/scripts/verify/P2-W7-F06.sh"; then
        echo "[FAIL] P2-W7-F06: shellcheck flagged the verify script" >&2
        exit 1
    fi
else
    echo "[INFO] P2-W7-F06: shellcheck not on PATH; skipping lint."
fi

echo "[OK] P2-W7-F06"
exit 0
