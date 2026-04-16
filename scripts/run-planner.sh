#!/usr/bin/env bash
# Launch the planner subagent to emit the next work-order for a phase.
# Read-only. Writes a new ucil-build/work-orders/NNNN-<slug>.json and
# (optionally) ucil-build/decisions/*.md + ucil-build/phase-log/*.md.
#
# Usage: scripts/run-planner.sh <phase-number>
#   e.g. scripts/run-planner.sh 0
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

PHASE="${1:-}"
if [[ -z "$PHASE" ]]; then
  PHASE=$(jq -r '.phase // empty' ucil-build/progress.json 2>/dev/null)
fi
if [[ -z "$PHASE" ]]; then
  echo "Usage: $0 <phase-number>  (e.g. 0)" >&2
  exit 2
fi

if ! command -v claude >/dev/null 2>&1; then
  echo "ERROR: claude CLI not in PATH" >&2
  exit 3
fi

# shellcheck source=scripts/_load-auth.sh
source "$(dirname "$0")/_load-auth.sh"

LOG="/tmp/ucil-planner-phase-${PHASE}.log"

# List of existing work-orders so planner avoids duplicating
EXISTING_WOS=$(ls ucil-build/work-orders/*.json 2>/dev/null | xargs -I{} basename {} .json | paste -sd, - || echo "none")

# List of already-passing feature IDs in this phase
PASSING=$(jq -r --arg p "$PHASE" '[.features[] | select((.phase|tostring)==$p) | select(.passes==true) | .id] | join(",")' ucil-build/feature-list.json 2>/dev/null || echo "")

PROMPT=$(cat <<EOF
You are the UCIL planner. Emit the NEXT work-order for phase ${PHASE}.

Current state:
- Phase: ${PHASE}
- Existing work-orders: ${EXISTING_WOS}
- Features already passing in phase ${PHASE}: ${PASSING:-none}

Your job:
1. Read ucil-build/feature-list.json and ucil-build/progress.json.
2. Read ucil-build/phase-log/$(printf '%02d' ${PHASE})-phase-${PHASE}/CLAUDE.md if it exists.
3. Check ucil-build/work-orders/ for existing work-orders — do NOT create a duplicate.
4. Select the next 1–5 features from phase ${PHASE} that:
   - have passes=false
   - have all dependencies (deps with passes=true)
   - are NOT already covered by an open/recent work-order
5. Emit a new work-order at ucil-build/work-orders/NNNN-<slug>.json with schema:
   {
     "id": "WO-NNNN",
     "slug": "short-kebab-slug",
     "phase": ${PHASE},
     "week": <from master plan>,
     "features": ["<id>", ...],
     "feature_ids": ["<id>", ...],
     "branch": "feat/WO-NNNN-<slug>",
     "worktree_branch": "feat/WO-NNNN-<slug>",
     "executor_agent": "executor",
     "goal": "<one sentence>",
     "plan_summary": "<1-3 sentences of implementation plan>",
     "scope_in": ["<bullets>"],
     "scope_out": ["<bullets>"],
     "acceptance": ["<bullets>"],
     "acceptance_criteria": ["<shell-testable assertions>"],
     "forbidden_paths": [
       "ucil-build/feature-list.json",
       "ucil-build/feature-list.schema.json",
       "ucil-master-plan-v2.1-final.md",
       "tests/fixtures/**",
       "scripts/gate/**",
       "scripts/flip-feature.sh"
     ],
     "context_refs": ["<master-plan section>", "<rules file>", ...],
     "dependencies_met": true,
     "estimated_commits": <int>,
     "estimated_complexity": "low|medium|high",
     "created_at": "<ISO-8601>",
     "created_by": "planner"
   }
6. If the spec is ambiguous, write an ADR at ucil-build/decisions/DEC-NNNN-<slug>.md first and reference it.
7. Commit the new work-order (and any ADRs) and push.

You may NOT edit source code, flip features, or mutate feature-list.json beyond what flip-feature.sh does (you don't run it). You must end cleanly.
EOF
)

echo "[run-planner] phase: ${PHASE}"
echo "[run-planner] existing WOs: ${EXISTING_WOS}"
echo "[run-planner] already-passing in phase: ${PASSING:-none}"
echo "[run-planner] log: ${LOG}"
echo "[run-planner] starting in 3s..."
sleep 3

CLAUDE_CODE_ENABLE_TELEMETRY=1 \
CLAUDE_SUBAGENT_NAME=planner \
exec claude -p "$PROMPT" \
  --model "${CLAUDE_CODE_MODEL:-opus-4-7}" \
  --dangerously-skip-permissions \
  --append-system-prompt "$(cat .claude/agents/planner.md)" \
  2>&1 | tee "$LOG"
