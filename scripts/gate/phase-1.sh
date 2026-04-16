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

# Effectiveness: must beat baseline on at least the nav scenarios.
check "effectiveness (phase 1 scenarios)" scripts/verify/effectiveness-gate.sh 1
# Multi-language coverage required from Phase 1 onwards.
check "multi-lang probes"                 scripts/verify/multi-lang-coverage.sh 1

# Anti-laziness quality gates: mutation score + coverage floor per crate.
# Phase 1 introduces ucil-core as a live crate; later crates are skipped
# automatically if their directory doesn't exist yet.
for crate in ucil-core ucil-daemon ucil-treesitter ucil-lsp-diagnostics; do
  check "mutation gate: ${crate}"         scripts/verify/mutation-gate.sh "${crate}" 70
  check "coverage gate: ${crate}"         scripts/verify/coverage-gate.sh "${crate}" 85 75
done

exit $FAIL
