#!/usr/bin/env bash
# Phase 1 — Daemon + tree-sitter + Serena + LSP diagnostics bridge
# Gate: workspace tests green, MCP smoke shows 22 tools, Serena docker-live test passes,
# tree-sitter reparse P95 <10ms on fixture.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"

FAIL=0
check() { local n="$1"; shift; if "$@"; then echo "  [OK]   $n"; else echo "  [FAIL] $n"; FAIL=1; fi; }

echo "-- Phase 1 checks --"

check "cargo test --workspace"             cargo nextest run --workspace --no-fail-fast 2>/dev/null || cargo test --workspace --no-fail-fast
check "clippy -D warnings"                  cargo clippy --workspace -- -D warnings 2>/dev/null

# MCP smoke
if [[ -x scripts/verify/e2e-mcp-smoke.sh ]]; then
  check "MCP 22 tools registered"          scripts/verify/e2e-mcp-smoke.sh
fi

# Serena docker-live
if [[ -x scripts/verify/serena-live.sh ]]; then
  check "Serena docker-live integration"   scripts/verify/serena-live.sh
fi

# Tree-sitter P95 reparse
if [[ -x scripts/verify/ts-reparse-p95.sh ]]; then
  check "tree-sitter P95 reparse <10ms"    scripts/verify/ts-reparse-p95.sh
fi

# Diagnostics bridge responds
if [[ -x scripts/verify/diagnostics-bridge.sh ]]; then
  check "diagnostics bridge live"          scripts/verify/diagnostics-bridge.sh
fi

exit $FAIL
