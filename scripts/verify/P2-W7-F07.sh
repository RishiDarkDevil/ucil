#!/usr/bin/env bash
# Acceptance test for P2-W7-F07 — ripgrep plugin manifest end-to-end smoke
# (WO-0051).
#
# Per DEC-0009 (search-code-in-process-ripgrep), ripgrep runs in-process via
# `crates/ucil-daemon/src/text_search.rs` (WO-0035) — NOT as an MCP server.
# The manifest at `plugins/search/ripgrep/plugin.toml` is therefore
# declarative metadata; this script does NOT exercise an MCP health-check
# round-trip (probe / ast-grep do, in scripts/verify/P2-W6-F05.sh /
# P2-W6-F06.sh).
#
# Sub-checks (all must pass for [OK] P2-W7-F07):
#   1. ripgrep on PATH; print `rg --version`. Optional shellcheck if avail.
#   2. Manifest exists at plugins/search/ripgrep/plugin.toml.
#   3. cargo test plugin_manifests::ripgrep_manifest_parses passes
#      (alternation regex per WO-0042 / WO-0043 / WO-0044).
#   4. `rg --json 'fn evaluate' tests/fixtures/rust-project` emits the
#      structured JSON markers `"type":"match"`, `"path"`, `fn evaluate`.
#   5. `.gitignore` is honoured: `rg --files-with-matches 'fn evaluate' .`
#      at repo root MUST NOT return paths under `target/`. The same call
#      MUST find the in-tree fixture (proves search actually ran).
#
# Read-only against tests/fixtures/rust-project (forbidden_paths in WO-0051).
set -euo pipefail
IFS=$'\n\t'

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

# Optional shellcheck (AC18 fallback per WO-0044).
if command -v shellcheck >/dev/null 2>&1; then
    if ! shellcheck "$0" scripts/devtools/install-ripgrep.sh; then
        echo "[FAIL] P2-W7-F07: shellcheck flagged issues" >&2
        exit 1
    fi
    echo "[INFO] P2-W7-F07: shellcheck PASS."
else
    echo "[INFO] shellcheck not on PATH; skipping shellcheck step."
fi

# ── Prereq: ripgrep on PATH ───────────────────────────────────────────
if ! command -v rg >/dev/null 2>&1; then
    echo "[FAIL] P2-W7-F07: rg not on PATH; see scripts/devtools/install-ripgrep.sh" >&2
    exit 1
fi
echo "[INFO] P2-W7-F07: $(rg --version | head -n1)"

# ── Sub-check: manifest exists ────────────────────────────────────────
if ! test -f plugins/search/ripgrep/plugin.toml; then
    echo "[FAIL] P2-W7-F07: missing plugins/search/ripgrep/plugin.toml" >&2
    exit 1
fi
echo "[INFO] P2-W7-F07: manifest plugins/search/ripgrep/plugin.toml present."

# ── Sub-check: cargo test (parse-only — no MCP round-trip) ────────────
CARGO_LOG="/tmp/wo-0051-cargo.log"
echo "[INFO] P2-W7-F07: running cargo test plugin_manifests::ripgrep_manifest_parses..."
if ! cargo test -p ucil-daemon --test plugin_manifests \
        plugin_manifests::ripgrep_manifest_parses 2>&1 | tee "${CARGO_LOG}" >/dev/null; then
    echo "[FAIL] P2-W7-F07: cargo test exited non-zero — see ${CARGO_LOG}" >&2
    tail -20 "${CARGO_LOG}" >&2 || true
    exit 1
fi
if ! grep -Eq 'test result: ok\. 1 passed; 0 failed|1 tests? passed' "${CARGO_LOG}"; then
    echo "[FAIL] P2-W7-F07: cargo test summary line missing in ${CARGO_LOG}" >&2
    tail -20 "${CARGO_LOG}" >&2 || true
    exit 1
fi
echo "[INFO] P2-W7-F07: cargo test PASS."

# ── Sub-check: ripgrep --json structured output against fixture ───────
JSON_LOG="/tmp/wo-0051-json.log"
echo "[INFO] P2-W7-F07: exercising rg --json against tests/fixtures/rust-project..."
# `pub fn evaluate(expr: &Expr)` lives at tests/fixtures/rust-project/src/util.rs:128
# (read-only fixture per forbidden_paths).
if ! rg --json 'fn evaluate' tests/fixtures/rust-project 2>&1 | tee "${JSON_LOG}" >/dev/null; then
    echo "[FAIL] P2-W7-F07: rg --json invocation exited non-zero — see ${JSON_LOG}" >&2
    tail -20 "${JSON_LOG}" >&2 || true
    exit 1
fi
if ! grep -q '"type":"match"' "${JSON_LOG}"; then
    echo "[FAIL] P2-W7-F07: ripgrep --json output missing structural marker '\"type\":\"match\"' — see ${JSON_LOG}" >&2
    tail -20 "${JSON_LOG}" >&2 || true
    exit 1
fi
if ! grep -q '"path"' "${JSON_LOG}"; then
    echo "[FAIL] P2-W7-F07: ripgrep --json output missing '\"path\"' field — see ${JSON_LOG}" >&2
    tail -20 "${JSON_LOG}" >&2 || true
    exit 1
fi
if ! grep -q 'fn evaluate' "${JSON_LOG}"; then
    echo "[FAIL] P2-W7-F07: ripgrep --json output missing match text 'fn evaluate' — see ${JSON_LOG}" >&2
    tail -20 "${JSON_LOG}" >&2 || true
    exit 1
fi
echo "[INFO] P2-W7-F07: rg --json structural markers PASS."

# ── Sub-check: .gitignore-respect at repo root ────────────────────────
echo "[INFO] P2-W7-F07: warming workspace build (target/ must exist for the gitignore-respect check)..."
cargo build --workspace --quiet 2>&1 | tail -20 || true

IGNORE_LOG="/tmp/wo-0051-gitignore.log"
echo "[INFO] P2-W7-F07: rg --files-with-matches 'fn evaluate' . — must NOT descend into target/..."
if ! rg --files-with-matches 'fn evaluate' . 2>&1 | tee "${IGNORE_LOG}" >/dev/null; then
    echo "[FAIL] P2-W7-F07: rg --files-with-matches at repo root exited non-zero — see ${IGNORE_LOG}" >&2
    tail -20 "${IGNORE_LOG}" >&2 || true
    exit 1
fi
if grep -Eq '(^|/)target/' "${IGNORE_LOG}"; then
    echo "[FAIL] P2-W7-F07: ripgrep returned target/ paths — .gitignore not honoured" >&2
    grep -E '(^|/)target/' "${IGNORE_LOG}" >&2 || true
    exit 1
fi
if ! grep -q 'tests/fixtures/rust-project/src/util.rs' "${IGNORE_LOG}"; then
    echo "[FAIL] P2-W7-F07: ripgrep did not return the in-tree fixture — search did not run as expected" >&2
    tail -20 "${IGNORE_LOG}" >&2 || true
    exit 1
fi
echo "[INFO] P2-W7-F07: .gitignore-respect sub-check PASS."

echo "[OK] P2-W7-F07"
exit 0
