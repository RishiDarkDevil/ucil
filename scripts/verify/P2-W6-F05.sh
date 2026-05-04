#!/usr/bin/env bash
# Acceptance test for P2-W6-F05 — ast-grep plugin manifest health-check +
# structural-search smoke against the typescript-project fixture.
#
# Behaviour:
#   1. cd to the repo root via `git rev-parse --show-toplevel`.
#   2. Assert `ast-grep` is on PATH; otherwise exit 1 with a clear hint
#      pointing at `scripts/devtools/install-ast-grep.sh`.
#   3. Print `ast-grep --version` for the verifier log.
#   4. Run `cargo test -p ucil-daemon --test plugin_manifests
#      plugin_manifests::ast_grep_manifest_health_check` and require the
#      cargo-test summary line "1 passed; 0 failed" or the cargo-nextest
#      equivalent — alternation regex per WO-0042 / WO-0043 / WO-0044.
#   5. Run `ast-grep run --pattern 'class TaskManager { $$$ }' --lang ts
#      tests/fixtures/typescript-project`, capture output to
#      /tmp/wo-0044-f05-search.log, and assert the captured output
#      contains the substring `TaskManager` — proves the structural
#      pattern actually matched code in the fixture.
#   6. On success print `[OK] P2-W6-F05` and exit 0; on any failure
#      print `[FAIL] P2-W6-F05: <reason>` and exit 1.
#
# This script is read-only against the fixture: it never modifies
# tests/fixtures/typescript-project (forbidden_paths in WO-0044).
set -euo pipefail
IFS=$'\n\t'

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

# ── Prereq: ast-grep on PATH ───────────────────────────────────────────
if ! command -v ast-grep >/dev/null 2>&1; then
    echo "[FAIL] P2-W6-F05: ast-grep not on PATH." >&2
    echo "  See scripts/devtools/install-ast-grep.sh for install hints." >&2
    exit 1
fi
echo "[INFO] P2-W6-F05: ast-grep version: $(ast-grep --version)"

# ── Step 1: integration test (real subprocess, real JSON-RPC) ─────────
CARGO_LOG="/tmp/wo-0044-f05-cargo.log"
echo "[INFO] P2-W6-F05: running cargo test plugin_manifests::ast_grep_manifest_health_check..."
if ! cargo test -p ucil-daemon --test plugin_manifests \
        plugin_manifests::ast_grep_manifest_health_check 2>&1 | tee "${CARGO_LOG}" >/dev/null; then
    echo "[FAIL] P2-W6-F05: cargo test exited non-zero — see ${CARGO_LOG}" >&2
    tail -20 "${CARGO_LOG}" >&2 || true
    exit 1
fi
if ! grep -Eq 'test result: ok\. 1 passed; 0 failed|1 tests run: 1 passed' "${CARGO_LOG}"; then
    echo "[FAIL] P2-W6-F05: cargo test summary line missing in ${CARGO_LOG}" >&2
    tail -20 "${CARGO_LOG}" >&2 || true
    exit 1
fi
echo "[INFO] P2-W6-F05: integration test PASS."

# ── Step 2: structural-search smoke ────────────────────────────────────
SEARCH_LOG="/tmp/wo-0044-f05-search.log"
echo "[INFO] P2-W6-F05: running ast-grep structural search against tests/fixtures/typescript-project..."
# `class TaskManager { $$$ }` matches the TaskManager class body in
# tests/fixtures/typescript-project/src/task-manager.ts:133. The fixture
# is read-only — we adapt to whatever symbols already exist there.
if ! ast-grep run \
        --pattern 'class TaskManager { $$$ }' \
        --lang ts \
        tests/fixtures/typescript-project 2>&1 | tee "${SEARCH_LOG}" >/dev/null; then
    echo "[FAIL] P2-W6-F05: ast-grep run exited non-zero — see ${SEARCH_LOG}" >&2
    tail -20 "${SEARCH_LOG}" >&2 || true
    exit 1
fi
if ! grep -q 'TaskManager' "${SEARCH_LOG}"; then
    echo "[FAIL] P2-W6-F05: ast-grep did not return any match for 'class TaskManager { \$\$\$ }' against tests/fixtures/typescript-project — see ${SEARCH_LOG}" >&2
    tail -20 "${SEARCH_LOG}" >&2 || true
    exit 1
fi
SEARCH_BYTES="$(wc -c < "${SEARCH_LOG}")"
echo "[INFO] P2-W6-F05: structural search returned ${SEARCH_BYTES} bytes containing TaskManager."

echo "[OK] P2-W6-F05"
exit 0
