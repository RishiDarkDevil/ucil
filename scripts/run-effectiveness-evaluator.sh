#!/usr/bin/env bash
# Spawn the effectiveness-evaluator subagent for a specific phase.
# Invoked by scripts/verify/effectiveness-gate.sh from a phase gate script.
#
# Usage: scripts/run-effectiveness-evaluator.sh <phase>
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

PHASE="${1:-}"
if [[ -z "$PHASE" ]]; then
  echo "Usage: $0 <phase>" >&2
  exit 2
fi

if ! command -v claude >/dev/null 2>&1; then
  echo "ERROR: claude CLI not in PATH" >&2
  exit 3
fi
# shellcheck source=scripts/_load-auth.sh
source "$(dirname "$0")/_load-auth.sh"

LOG="/tmp/ucil-effectiveness-phase-${PHASE}.log"
REPORT="ucil-build/verification-reports/effectiveness-phase-${PHASE}.md"

PROMPT=$(cat <<EOF
You are the UCIL effectiveness-evaluator. Current phase: ${PHASE}.

Discover every tests/scenarios/*.yaml that has phases containing ${PHASE}.
For each scenario:
  1. Check requires_tools are all operational (tools/list probe to ucil-mcp).
     If any missing, mark skipped_tool_not_ready.
  2. Copy the fixture to /tmp/ucil-eval-<scenario-id>/ucil and
     /tmp/ucil-eval-<scenario-id>/baseline.
  3. Run the task twice (UCIL then baseline), capture diffs + token counts.
  4. Run acceptance_checks in each tempdir.
  5. Judge both outputs via a fresh claude -p session using the rubric.
  6. Verdict per rubric in .claude/agents/effectiveness-evaluator.md.

Write ${REPORT}, commit + push. Exit 0 if gate passes, 1 if any FAIL,
2 on internal error.
EOF
)

echo "[run-effectiveness] phase=${PHASE} log=${LOG}"

CLAUDE_CODE_ENABLE_TELEMETRY=1 \
CLAUDE_SUBAGENT_NAME=effectiveness-evaluator \
exec claude -p "$PROMPT" \
  --dangerously-skip-permissions \
  --append-system-prompt "$(cat .claude/agents/effectiveness-evaluator.md)" \
  2>&1 | tee "$LOG"
