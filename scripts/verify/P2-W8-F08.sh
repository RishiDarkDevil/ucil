#!/usr/bin/env bash
set -euo pipefail
IFS=$'\n\t'
# Acceptance test for P2-W8-F08 — find_similar MCP tool
# (master-plan §3.2 line 219 + §18 Phase 2 Week 8 line 1791
# "Vector search works"; closes Phase 2 Week 8 and the entire
# Phase 2 envelope).  Authored under WO-0066.
#
# Behaviour:
#   1. cd to repo root via `git rev-parse --show-toplevel`.
#   2. Frozen-selector grep — confirm the literal
#      `pub async fn test_find_similar_tool()` lives at column 0
#      in `crates/ucil-daemon/src/server.rs`.  Load-bearing — guards
#      against rename drift.  See DEC-0007 module-root convention.
#   3. Frozen-handler grep — confirm `fn handle_find_similar` lives
#      in `crates/ucil-daemon/src/server.rs`.
#   4. Frozen-builder grep — confirm `fn with_find_similar_executor`
#      lives in `crates/ucil-daemon/src/server.rs`.
#   5. Run the frozen-selector test:
#      `cargo nextest run -p ucil-daemon server::test_find_similar_tool`
#      (falls back to `cargo test -p ucil-daemon -- --exact
#      server::test_find_similar_tool` if cargo-nextest is absent).
#   6. On success print `[OK] P2-W8-F08 find_similar tool wired
#      and verified` and exit 0; on any failure print
#      `[FAIL] P2-W8-F08: <reason>` to stderr and exit 1.

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "${REPO_ROOT}"

SERVER_RS="crates/ucil-daemon/src/server.rs"

# ── Step 1: server.rs existence ─────────────────────────────────────────
if [ ! -f "${SERVER_RS}" ]; then
    echo "[FAIL] P2-W8-F08: ${SERVER_RS} missing" >&2
    exit 1
fi

# ── Step 2: frozen-selector grep — module-root pub async fn ─────────────
if ! grep -qE '^[[:space:]]*pub async fn test_find_similar_tool\(\)' "${SERVER_RS}"; then
    echo "[FAIL] P2-W8-F08: frozen selector \`pub async fn test_find_similar_tool()\` not found at module root in ${SERVER_RS}" >&2
    echo "[HINT] P2-W8-F08: per DEC-0007 the test lives at column 0 (NOT inside \`mod tests {}\`) so the selector \`server::test_find_similar_tool\` resolves directly." >&2
    exit 1
fi

# ── Step 3: frozen-handler grep ─────────────────────────────────────────
if ! grep -qE 'fn handle_find_similar' "${SERVER_RS}"; then
    echo "[FAIL] P2-W8-F08: handler \`fn handle_find_similar\` not found in ${SERVER_RS}" >&2
    exit 1
fi

# ── Step 4: frozen-builder grep ─────────────────────────────────────────
if ! grep -qE 'fn with_find_similar_executor' "${SERVER_RS}"; then
    echo "[FAIL] P2-W8-F08: builder \`fn with_find_similar_executor\` not found in ${SERVER_RS}" >&2
    exit 1
fi

# ── Step 5: run the frozen-selector test ────────────────────────────────
# Prefer cargo-nextest (faster, structured output); fall back to
# `cargo test --exact` if not on PATH.  Both are acceptable per AC26.
if command -v cargo-nextest > /dev/null 2>&1; then
    echo "[INFO] P2-W8-F08: running cargo nextest run -p ucil-daemon server::test_find_similar_tool"
    if ! cargo nextest run -p ucil-daemon server::test_find_similar_tool --no-fail-fast; then
        echo "[FAIL] P2-W8-F08: cargo nextest run exited non-zero" >&2
        exit 1
    fi
else
    echo "[INFO] P2-W8-F08: cargo-nextest not on PATH — falling back to \`cargo test --exact\`"
    if ! cargo test -p ucil-daemon --lib -- --exact server::test_find_similar_tool; then
        echo "[FAIL] P2-W8-F08: cargo test --exact exited non-zero" >&2
        exit 1
    fi
fi

echo "[OK] P2-W8-F08 find_similar tool wired and verified"
exit 0
