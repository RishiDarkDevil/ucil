#!/usr/bin/env bash
# Acceptance test for P2-W6-F06 — probe plugin manifest health-check +
# token-budgeted function-body extraction smoke against the rust-project
# fixture.
#
# Behaviour:
#   1. cd to the repo root via `git rev-parse --show-toplevel`.
#   2. Assert `probe` is on PATH; otherwise exit 1 with a clear hint
#      pointing at `scripts/devtools/install-probe.sh`.
#   3. Print `probe --version` for the verifier log.
#   4. Run `cargo test -p ucil-daemon --test plugin_manifests
#      plugin_manifests::probe_manifest_health_check` and require the
#      cargo-test summary line "1 passed; 0 failed" or the cargo-nextest
#      equivalent — alternation regex per WO-0042 / WO-0043 / WO-0044.
#   5. Run `probe search --max-tokens 4096 'fn evaluate'
#      tests/fixtures/rust-project`, capture output to
#      /tmp/wo-0044-f06-search.log, and assert the log contains
#      `fn evaluate` — proves the AST-aware search returned the function
#      body. The `--max-tokens=4096` flag exercises the token-budget
#      surface from master-plan §4.2 ("token-budgeted complete function
#      bodies"). Verify the output is bounded under a sane upper limit
#      (16384 chars) so the budget actually constrains the response.
#   6. Run `probe extract tests/fixtures/rust-project/src/util.rs#evaluate`
#      to demonstrate the explicit symbol-extraction path that returns
#      the complete function body wrapped in a code fence — captured to
#      /tmp/wo-0044-f06-extract.log, asserted to contain
#      `fn evaluate`.
#   7. On success print `[OK] P2-W6-F06` and exit 0; on any failure
#      print `[FAIL] P2-W6-F06: <reason>` and exit 1.
#
# This script is read-only against the fixture: it never modifies
# tests/fixtures/rust-project (forbidden_paths in WO-0044).
set -euo pipefail
IFS=$'\n\t'

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

# ── Prereq: probe on PATH ──────────────────────────────────────────────
if ! command -v probe >/dev/null 2>&1; then
    echo "[FAIL] P2-W6-F06: probe not on PATH." >&2
    echo "  See scripts/devtools/install-probe.sh for install hints." >&2
    exit 1
fi
echo "[INFO] P2-W6-F06: probe version: $(probe --version)"

# ── Step 1: integration test (real subprocess, real JSON-RPC) ─────────
CARGO_LOG="/tmp/wo-0044-f06-cargo.log"
echo "[INFO] P2-W6-F06: running cargo test plugin_manifests::probe_manifest_health_check..."
if ! cargo test -p ucil-daemon --test plugin_manifests \
        plugin_manifests::probe_manifest_health_check 2>&1 | tee "${CARGO_LOG}" >/dev/null; then
    echo "[FAIL] P2-W6-F06: cargo test exited non-zero — see ${CARGO_LOG}" >&2
    tail -20 "${CARGO_LOG}" >&2 || true
    exit 1
fi
if ! grep -Eq 'test result: ok\. 1 passed; 0 failed|1 tests run: 1 passed' "${CARGO_LOG}"; then
    echo "[FAIL] P2-W6-F06: cargo test summary line missing in ${CARGO_LOG}" >&2
    tail -20 "${CARGO_LOG}" >&2 || true
    exit 1
fi
echo "[INFO] P2-W6-F06: integration test PASS."

# ── Step 2: token-budgeted AST-aware search smoke ─────────────────────
SEARCH_LOG="/tmp/wo-0044-f06-search.log"
MAX_TOKENS=4096
MAX_BYTES_BUDGET=16384
echo "[INFO] P2-W6-F06: running probe search with --max-tokens=${MAX_TOKENS}..."
# `fn evaluate` matches the public eval entry-point at
# tests/fixtures/rust-project/src/util.rs:128. The fixture is read-only.
if ! probe search \
        --max-tokens "${MAX_TOKENS}" \
        "fn evaluate" \
        tests/fixtures/rust-project 2>&1 | tee "${SEARCH_LOG}" >/dev/null; then
    echo "[FAIL] P2-W6-F06: probe search exited non-zero — see ${SEARCH_LOG}" >&2
    tail -20 "${SEARCH_LOG}" >&2 || true
    exit 1
fi
if ! grep -q 'fn evaluate' "${SEARCH_LOG}"; then
    echo "[FAIL] P2-W6-F06: probe search did not return 'fn evaluate' against tests/fixtures/rust-project — see ${SEARCH_LOG}" >&2
    tail -20 "${SEARCH_LOG}" >&2 || true
    exit 1
fi
SEARCH_BYTES="$(wc -c < "${SEARCH_LOG}")"
if [[ "${SEARCH_BYTES}" -gt "${MAX_BYTES_BUDGET}" ]]; then
    echo "[FAIL] P2-W6-F06: token-budgeted search returned ${SEARCH_BYTES} bytes (cap ${MAX_BYTES_BUDGET}) — token budget appears unbounded." >&2
    exit 1
fi
echo "[INFO] P2-W6-F06: search returned ${SEARCH_BYTES} bytes (under ${MAX_BYTES_BUDGET}-byte cap) and contains fn evaluate."

# ── Step 3: explicit function-body extraction ─────────────────────────
EXTRACT_LOG="/tmp/wo-0044-f06-extract.log"
echo "[INFO] P2-W6-F06: running probe extract on tests/fixtures/rust-project/src/util.rs#evaluate..."
if ! probe extract \
        "tests/fixtures/rust-project/src/util.rs#evaluate" 2>&1 | tee "${EXTRACT_LOG}" >/dev/null; then
    echo "[FAIL] P2-W6-F06: probe extract exited non-zero — see ${EXTRACT_LOG}" >&2
    tail -20 "${EXTRACT_LOG}" >&2 || true
    exit 1
fi
if ! grep -q 'fn evaluate' "${EXTRACT_LOG}"; then
    echo "[FAIL] P2-W6-F06: probe extract did not return 'fn evaluate' for util.rs#evaluate — see ${EXTRACT_LOG}" >&2
    tail -20 "${EXTRACT_LOG}" >&2 || true
    exit 1
fi
EXTRACT_BYTES="$(wc -c < "${EXTRACT_LOG}")"
echo "[INFO] P2-W6-F06: extract returned ${EXTRACT_BYTES} bytes containing fn evaluate."

echo "[OK] P2-W6-F06"
exit 0
