#!/usr/bin/env bash
# Phase 3.5 — Agent layer
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"
FAIL=0
check() { local n="$1"; shift; if "$@"; then echo "  [OK]   $n"; else echo "  [FAIL] $n"; FAIL=1; fi; }
echo "-- Phase 3.5 checks --"
check "cargo test --workspace"                cargo nextest run --workspace --no-fail-fast 2>/dev/null || cargo test --workspace --no-fail-fast
[[ -x scripts/verify/agent-canary-ollama.sh ]]   && check "agents respond with ollama"  scripts/verify/agent-canary-ollama.sh
[[ -x scripts/verify/agent-canary-none.sh ]]     && check "agents respond with none"    scripts/verify/agent-canary-none.sh
[[ -x scripts/verify/mcp-elicitation.sh ]]       && check "MCP Elicitation round-trip"  scripts/verify/mcp-elicitation.sh
exit $FAIL
