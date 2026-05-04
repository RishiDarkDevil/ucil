#!/usr/bin/env bash
# Acceptance test for P2-W6-F08 — plugin-lifecycle integration suite at
# tests/integration/test_plugin_lifecycle.rs (per DEC-0010).
#
# Behaviour:
#   1. cd to the repo root via `git rev-parse --show-toplevel`.
#   2. Warm-build the `mock-mcp-plugin` binary so the three
#      module-root #[tokio::test]s do not race the binary build at
#      first spawn.
#   3. Run `cargo test --test test_plugin_lifecycle -- --nocapture
#      --test-threads=1` and require the cargo-test / cargo-nextest
#      summary line via the alternation regex established in
#      WO-0038/0039/0042/0043/0044/0045.
#   4. On success print `[OK] P2-W6-F08` and exit 0; on any failure
#      print `[FAIL] P2-W6-F08: <reason>` and exit 1, including an
#      operator-actionable hint pointing at
#      `cargo build -p ucil-daemon --bin mock-mcp-plugin`.
#
# This script never touches `tests/fixtures/**`. Every test manifest
# inside test_plugin_lifecycle.rs is a struct literal built in the
# test fn; no on-disk fixture is required.
set -euo pipefail
IFS=$'\n\t'

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "${REPO_ROOT}"

BUILD_LOG="/tmp/wo-0046-mock-build.log"
TEST_LOG="/tmp/wo-0046-test.log"

# ── Step 1: warm-build mock-mcp-plugin ─────────────────────────────────
echo "[INFO] P2-W6-F08: warm-building mock-mcp-plugin..."
if ! cargo build -p ucil-daemon --bin mock-mcp-plugin 2>&1 | tee "${BUILD_LOG}" >/dev/null; then
    echo "[FAIL] P2-W6-F08: cargo build -p ucil-daemon --bin mock-mcp-plugin failed — see ${BUILD_LOG}" >&2
    tail -30 "${BUILD_LOG}" >&2 || true
    echo "[HINT] P2-W6-F08: rerun manually:" >&2
    echo "         cargo build -p ucil-daemon --bin mock-mcp-plugin" >&2
    exit 1
fi

MOCK_BIN_DEBUG="${REPO_ROOT}/target/debug/mock-mcp-plugin"
MOCK_BIN_RELEASE="${REPO_ROOT}/target/release/mock-mcp-plugin"
if [[ ! -x "${MOCK_BIN_DEBUG}" && ! -x "${MOCK_BIN_RELEASE}" ]]; then
    echo "[FAIL] P2-W6-F08: mock-mcp-plugin binary not produced at ${MOCK_BIN_DEBUG} (or release path)" >&2
    echo "[HINT] P2-W6-F08: rerun manually:" >&2
    echo "         cargo build -p ucil-daemon --bin mock-mcp-plugin" >&2
    exit 1
fi

# ── Step 2: run the test_plugin_lifecycle test target ──────────────────
echo "[INFO] P2-W6-F08: running cargo test --test test_plugin_lifecycle..."
if ! cargo test --test test_plugin_lifecycle -- --nocapture --test-threads=1 2>&1 \
    | tee "${TEST_LOG}" >/dev/null; then
    echo "[FAIL] P2-W6-F08: cargo test --test test_plugin_lifecycle exited non-zero — see ${TEST_LOG}" >&2
    tail -40 "${TEST_LOG}" >&2 || true
    echo "[HINT] P2-W6-F08: if a test reports the mock binary is missing, rerun manually:" >&2
    echo "         cargo build -p ucil-daemon --bin mock-mcp-plugin" >&2
    exit 1
fi
if ! grep -Eq 'test result: ok\. [0-9]+ passed; 0 failed|[0-9]+ tests run: [0-9]+ passed' "${TEST_LOG}"; then
    echo "[FAIL] P2-W6-F08: cargo test summary line missing in ${TEST_LOG}" >&2
    tail -40 "${TEST_LOG}" >&2 || true
    exit 1
fi

# ── Optional: shellcheck the script itself ─────────────────────────────
if command -v shellcheck >/dev/null 2>&1; then
    if ! shellcheck "${REPO_ROOT}/scripts/verify/P2-W6-F08.sh"; then
        echo "[FAIL] P2-W6-F08: shellcheck flagged the verify script" >&2
        exit 1
    fi
else
    echo "[INFO] P2-W6-F08: shellcheck not on PATH; skipping lint."
fi

echo "[OK] P2-W6-F08"
exit 0
