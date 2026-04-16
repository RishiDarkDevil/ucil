#!/usr/bin/env bash
# Spawn the root-cause-finder subagent for a rejected work-order.
#
# Invoked by scripts/run-phase.sh when the verifier writes a rejection for a
# work-order whose features still have attempts < 3. The RCF reads the
# rejection, diagnoses, and writes
# ucil-build/verification-reports/root-cause-<WO-ID>.md. The outer loop then
# re-runs the executor with the RCF's findings as supplementary context.
#
# Usage: scripts/run-root-cause-finder.sh <WO-ID>
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

WO_ARG="${1:-}"
if [[ -z "$WO_ARG" ]]; then
  echo "Usage: $0 <work-order-id>" >&2
  exit 2
fi
WO_ID="${WO_ARG#WO-}"
WO_ID="WO-${WO_ID}"

WO_FILE=$(ls ucil-build/work-orders/${WO_ID#WO-}-*.json 2>/dev/null | head -1)
if [[ -z "$WO_FILE" ]]; then
  echo "ERROR: no work-order JSON for $WO_ID" >&2
  exit 3
fi

REJECTION="ucil-build/rejections/${WO_ID}.md"
if [[ ! -f "$REJECTION" ]]; then
  echo "ERROR: no rejection file at $REJECTION" >&2
  exit 3
fi

if ! command -v claude >/dev/null 2>&1; then
  echo "ERROR: claude CLI not in PATH" >&2
  exit 3
fi
# shellcheck source=scripts/_load-auth.sh
source "$(dirname "$0")/_load-auth.sh"

LOG="/tmp/ucil-rcf-${WO_ID}.log"
SLUG=$(jq -r .slug "$WO_FILE")
BRANCH="feat/${WO_ID}-${SLUG}"
OUT="ucil-build/verification-reports/root-cause-${WO_ID}.md"

PROMPT=$(cat <<EOF
You are the UCIL root-cause-finder. A verifier rejected work-order ${WO_ID}.

Inputs:
- Work-order: ${WO_FILE}
- Rejection:  ${REJECTION}
- Branch to inspect: ${BRANCH} (read-only)
- Any prior root-cause report at ${OUT} (may be overwritten)

Read the rejection, then:
1. Reproduce the failure yourself if feasible (run the failing command in the worktree at ../ucil-wt/${WO_ID}).
2. Form a ranked hypothesis tree (most-likely-first) of what's causing the failure.
3. Choose the highest-confidence hypothesis.
4. Identify a specific, concrete remediation — either:
   (a) "Executor should change X in file Y because Z" (most common),
   (b) "Planner should split/rescope feature N", OR
   (c) "Spec ambiguity — ADR needed in ucil-build/decisions/".
5. Write your findings to ${OUT}. Follow the format in .claude/agents/root-cause-finder.md.

The outer loop will feed your output back into the executor as supplementary
context for its retry. Be actionable. Cite file:line.

Safety: you do NOT edit source code, the master plan, feature-list.json, or ADRs.
You MAY run read-only commands and may test hypotheses via stash/pop (but
restore state before exiting).

End cleanly. Commit + push ${OUT}.
EOF
)

echo "[run-rcf] work-order: ${WO_FILE}"
echo "[run-rcf] rejection:  ${REJECTION}"
echo "[run-rcf] output:     ${OUT}"
echo "[run-rcf] log:        ${LOG}"

UCIL_WO_ID="${WO_ID}" CLAUDE_CODE_ENABLE_TELEMETRY=1 \
CLAUDE_SUBAGENT_NAME=root-cause-finder \
exec claude -p "$PROMPT" \
  --dangerously-skip-permissions \
  --append-system-prompt "$(cat .claude/agents/root-cause-finder.md)" \
  2>&1 | tee "$LOG"
