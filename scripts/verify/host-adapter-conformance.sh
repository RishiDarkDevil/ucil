#!/usr/bin/env bash
# Multi-host adapter conformance suite.
#
# Master plan §9 + §18 Phase 4: UCIL ships adapters for Claude Code,
# Codex CLI, Aider, Cline/Roo Code, Cursor, Ollama/local. Each adapter
# must correctly transform UCIL's CEQP responses into the host's
# preferred shape (token budget, pagination, capability filtering).
#
# Contract (implement by Phase 4):
#   For each supported host:
#     1. Install UCIL's adapter into an ephemeral sandbox ($HOME=tmpdir).
#     2. Emit a canned CEQP response from UCIL.
#     3. Drive the adapter's transformer.
#     4. Assert the output:
#          - Claude Code: full enriched response, all 22 tools visible
#          - Codex CLI: <= 10KB, <= 256 lines, pagination token present if truncated
#          - Cursor: all 22 tools (under 40-tool cap), full bonus context
#          - Cline/Roo Code: mode-specific tool filtering works
#          - Aider: pre-compressed via HTTP bridge, <4KB
#          - Ollama/local: signature-only mode, bonus context minimal
#     5. Run a round-trip: the host sends a query, adapter forwards to
#        UCIL MCP, response comes back, adapter transforms it. Assert
#        the host sees a usable result.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
PHASE="${1:-$(jq -r .phase ucil-build/progress.json)}"

case "$PHASE" in
  4|5|6|7|8) ;;
  *) echo "[adapter-conformance] phase $PHASE: not required"; exit 0 ;;
esac

echo "[adapter-conformance] phase=$PHASE"
echo "[adapter-conformance] TODO: per-host adapter transform + round-trip; required by Phase 4 gate."
exit 1
