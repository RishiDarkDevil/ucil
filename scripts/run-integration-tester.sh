#!/usr/bin/env bash
# Launch the integration-tester subagent.
#
# Usage: scripts/run-integration-tester.sh <phase>
#   e.g. scripts/run-integration-tester.sh 1
#
# Brings up real docker-backed fixtures (when needed for the phase),
# runs end-to-end integration tests, and writes
# ucil-build/verification-reports/phase-N-integration.md.
#
# Invoked by scripts/run-phase.sh pre-gate for phases 1, 2, 3, 5, 7.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

PHASE="${1:-}"
if [[ -z "$PHASE" ]]; then
  echo "Usage: $0 <phase>" >&2
  exit 2
fi

case "$PHASE" in
  1|2|3|5|7) ;;
  *)
    echo "[run-integration-tester] phase $PHASE does not require an integration pass; skipping."
    exit 0
    ;;
esac

# shellcheck source=scripts/_load-auth.sh
source "$(dirname "$0")/_load-auth.sh"

LOG="/tmp/ucil-integration-tester-phase-${PHASE}.log"

PROMPT=$(cat <<EOF
You are the UCIL integration-tester. Current phase: ${PHASE}.

Bring up the docker-backed fixtures required for phase ${PHASE} per
.claude/agents/integration-tester.md (Serena + LSP server containers
for Phase 1; add LanceDB/ONNX for Phase 2; etc).

For Phase 1 specifically, also run the three scripts that the phase-1
gate expects:

1. scripts/verify/e2e-mcp-smoke.sh      — ucil-daemon MCP handshake, 22 tools.
2. scripts/verify/serena-live.sh        — Serena uvx MCP handshake.
3. scripts/verify/diagnostics-bridge.sh — pyright LSP diagnostics round-trip.

Collect each script's exit code and tail(stdout+stderr). If any fails,
the integration pass fails; write the report with verdict FAIL.

Write the report at ucil-build/verification-reports/phase-${PHASE}-integration.md
following the template in .claude/agents/integration-tester.md.

Tear down docker services cleanly. Commit + push the report. Never edit
source code — your job is to observe, not to fix.
EOF
)

echo "[run-integration-tester] phase: ${PHASE}"
echo "[run-integration-tester] log:   ${LOG}"
echo "[run-integration-tester] starting in 3s..."
sleep 3

UCIL_PHASE="${PHASE}" CLAUDE_CODE_ENABLE_TELEMETRY=1 \
CLAUDE_SUBAGENT_NAME=integration-tester \
exec claude -p "$PROMPT" \
  --model "${CLAUDE_CODE_MODEL:-claude-opus-4-7}" \
  --dangerously-skip-permissions \
  --append-system-prompt "$(cat .claude/agents/integration-tester.md)" \
  2>&1 | tee "$LOG"
