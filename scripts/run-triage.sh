#!/usr/bin/env bash
# Spawn the triage subagent for one pass over unresolved escalations.
#
# Usage: scripts/run-triage.sh [<phase-number>]
#
# Env:
#   UCIL_TRIAGE_PASS — 1-indexed count (orchestrator sets it). Defaults to 1.
#   UCIL_PHASE       — phase number (derived from progress.json if unset).
#
# Exit codes:
#   0 — all escalations resolved (orchestrator may continue)
#   1 — at least one escalation remains (orchestrator halts + pages user)
#   2 — triage itself crashed / failed
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

PHASE="${1:-${UCIL_PHASE:-$(jq -r '.phase // empty' ucil-build/progress.json 2>/dev/null)}}"
if [[ -z "$PHASE" ]]; then
  echo "ERROR: no phase" >&2
  exit 2
fi
TRIAGE_PASS="${UCIL_TRIAGE_PASS:-1}"

# Any unresolved files at all?
unresolved_count() {
  local n=0
  shopt -s nullglob
  for f in ucil-build/escalations/*.md; do
    if ! grep -qE '^resolved:[[:space:]]*true[[:space:]]*$' "$f"; then
      n=$((n+1))
    fi
  done
  shopt -u nullglob
  echo "$n"
}

INITIAL=$(unresolved_count)
if [[ "$INITIAL" -eq 0 ]]; then
  echo "[run-triage] no unresolved escalations; nothing to do."
  exit 0
fi

if ! command -v claude >/dev/null 2>&1; then
  echo "ERROR: claude CLI not in PATH" >&2
  exit 2
fi
# shellcheck source=scripts/_load-auth.sh
source "$(dirname "$0")/_load-auth.sh"

LOG="/tmp/ucil-triage-phase-${PHASE}-pass-${TRIAGE_PASS}.log"

PROMPT=$(cat <<EOF
You are the UCIL triage agent. Current phase: ${PHASE}. Triage pass: ${TRIAGE_PASS}.

Read every file in ucil-build/escalations/ that does NOT currently have
'resolved: true' in its frontmatter or as a trailing line. For each, apply
the Bucket A / B / C decision rubric from .claude/agents/triage.md.

Rules of engagement:
- Process one escalation at a time.
- Commit + push after EACH bucket-A or bucket-B action.
- Never touch UCIL source code, feature-list.json, the master plan, ADRs, or files on the bucket-B deny list.
- If ${TRIAGE_PASS} >= 3, default everything to bucket C.
- End your session cleanly after processing all escalations. Print the summary table.
EOF
)

echo "[run-triage] unresolved at start: ${INITIAL}"
echo "[run-triage] phase=${PHASE} pass=${TRIAGE_PASS}"
echo "[run-triage] log: ${LOG}"
echo "[run-triage] starting..."

UCIL_PHASE="$PHASE" \
UCIL_TRIAGE_PASS="$TRIAGE_PASS" \
CLAUDE_CODE_ENABLE_TELEMETRY=1 \
CLAUDE_SUBAGENT_NAME=triage \
claude -p "$PROMPT" \
  --model "${CLAUDE_CODE_MODEL:-opus-4-7}" \
  --dangerously-skip-permissions \
  --append-system-prompt "$(cat .claude/agents/triage.md)" \
  2>&1 | tee "$LOG" || true

REMAINING=$(unresolved_count)
echo ""
echo "[run-triage] pass ${TRIAGE_PASS} done. unresolved: ${INITIAL} -> ${REMAINING}"

if [[ "$REMAINING" -eq 0 ]]; then
  exit 0
else
  exit 1
fi
