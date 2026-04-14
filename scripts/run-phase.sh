#!/usr/bin/env bash
# Outer autonomous loop for one phase.
# Repeatedly: planner -> executor -> critic -> verifier -> update progress.
# Halts on: gate pass, drift, escalation, attempt cap.
#
# Usage: scripts/run-phase.sh <N>
set -uo pipefail

cd "$(git rev-parse --show-toplevel)"

PHASE="${1:-}"
if [[ -z "$PHASE" ]]; then
  PHASE=$(jq -r '.phase // empty' ucil-build/progress.json)
fi
if [[ -z "$PHASE" ]]; then
  echo "ERROR: no phase specified" >&2
  exit 2
fi

if [[ -z "${ANTHROPIC_API_KEY:-}" && -f .env ]]; then
  set -a
  source .env
  set +a
fi

DRIFT_FILE="ucil-build/drift-counters.json"
if [[ ! -f "$DRIFT_FILE" ]]; then
  echo '{}' > "$DRIFT_FILE"
fi

MAX_ITERATIONS=200
iter=0

while true; do
  iter=$((iter+1))
  if [[ "$iter" -gt "$MAX_ITERATIONS" ]]; then
    echo "[run-phase] MAX_ITERATIONS=$MAX_ITERATIONS hit — escalating."
    mkdir -p ucil-build/escalations
    echo "# Max iterations reached on phase $PHASE
Iter: $iter
Open for human review." > "ucil-build/escalations/$(date -u +%Y%m%dT%H%M%SZ)-max-iter-phase-${PHASE}.md"
    exit 1
  fi

  # Gate check
  if scripts/gate-check.sh "$PHASE" 2>/dev/null; then
    echo "[run-phase] Gate for phase $PHASE is GREEN — loop complete."
    exit 0
  fi

  # Escalation check — try triage before halting.
  # Count ONLY unresolved escalations (those missing `resolved: true`).
  unresolved_count=0
  shopt -s nullglob
  for _esc in ucil-build/escalations/*.md; do
    if ! grep -qE '^resolved:[[:space:]]*true[[:space:]]*$' "$_esc"; then
      unresolved_count=$((unresolved_count+1))
    fi
  done
  shopt -u nullglob

  if [[ "$unresolved_count" -gt 0 ]]; then
    TRIAGE_PASS_FILE=".ucil-triage-pass.phase-${PHASE}"
    TRIAGE_PASS=$(cat "$TRIAGE_PASS_FILE" 2>/dev/null || echo 0)
    TRIAGE_PASS=$((TRIAGE_PASS+1))
    echo "$TRIAGE_PASS" > "$TRIAGE_PASS_FILE"
    echo "[run-phase] ${unresolved_count} unresolved escalation(s); spawning triage (pass ${TRIAGE_PASS})..."
    UCIL_PHASE="$PHASE" UCIL_TRIAGE_PASS="$TRIAGE_PASS" scripts/run-triage.sh "$PHASE"
    triage_rc=$?
    if [[ "$triage_rc" -ne 0 ]]; then
      echo "[run-phase] triage could not resolve all escalations — halting for human review."
      ls -1 ucil-build/escalations/
      exit 1
    fi
    echo "[run-phase] triage resolved all escalations; continuing."
  fi

  # Drift check
  DRIFT=$(jq -r --arg p "$PHASE" '.[$p] // 0' "$DRIFT_FILE")
  if [[ "$DRIFT" -ge 4 ]]; then
    echo "[run-phase] Drift counter >= 4 — escalating."
    echo "# Drift detected on phase $PHASE
Consecutive no-flip turns: $DRIFT
Invoke /replan or root-cause-finder." > "ucil-build/escalations/$(date -u +%Y%m%dT%H%M%SZ)-drift-phase-${PHASE}.md"
    exit 1
  fi

  echo ""
  echo "==========================================="
  echo "[run-phase] Iteration $iter on phase $PHASE"
  echo "==========================================="

  # 1. Planner — emit work-order
  echo "[run-phase] Step 1/4: planner"
  # Build context the planner needs to avoid emitting duplicate WOs or
  # selecting features already handled by open/recent WOs.
  EXISTING_WOS=$(ls ucil-build/work-orders/*.json 2>/dev/null | xargs -I{} basename {} .json | paste -sd, - || echo "none")
  PASSING_IN_PHASE=$(jq -r --arg p "$PHASE" '[.features[] | select((.phase|tostring)==$p) | select(.passes==true) | .id] | join(",")' ucil-build/feature-list.json 2>/dev/null || echo "")
  # List feature_ids already claimed by any work-order (regardless of status)
  CLAIMED_FEATS=$(jq -rs '[.[] | (.feature_ids // .features // [])[]] | unique | join(",")' ucil-build/work-orders/*.json 2>/dev/null || echo "")

  PLAN_PROMPT="You are the UCIL planner. Phase: $PHASE.

Current state:
- Existing work-orders (do NOT create a duplicate): ${EXISTING_WOS}
- Features already passing in phase ${PHASE}: ${PASSING_IN_PHASE:-none}
- Feature IDs already claimed by any work-order (open or completed): ${CLAIMED_FEATS:-none}

Read ucil-build/feature-list.json and ucil-build/progress.json.
Emit the NEXT work-order in ucil-build/work-orders/NNNN-<slug>.json covering
1-5 phase-${PHASE} features that:
  (a) have passes=false,
  (b) have all dependencies satisfied (deps with passes=true), AND
  (c) are NOT in the claimed-features list above.
If no eligible features exist (every non-passing feature's deps are blocked
or every feature is already claimed by a pending WO), write an escalation
to ucil-build/escalations/ explaining the blocker and end. Otherwise commit
the new work-order + any ADRs and push. End your session cleanly."
  CLAUDE_SUBAGENT_NAME=planner claude -p "$PLAN_PROMPT" \
    --dangerously-skip-permissions \
    --append-system-prompt "$(cat .claude/agents/planner.md)" \
    >/tmp/ucil-planner.log 2>&1 || {
      echo "[run-phase] planner failed — see /tmp/ucil-planner.log"
      cat /tmp/ucil-planner.log | tail -20
      exit 1
    }

  # Discover the latest work-order
  LATEST_WO=$(ls -t ucil-build/work-orders/*.json 2>/dev/null | head -1 || true)
  if [[ -z "$LATEST_WO" ]]; then
    echo "[run-phase] planner emitted no work-order — escalating."
    exit 1
  fi
  echo "[run-phase] work-order: $LATEST_WO"

  # 2. Executor
  echo "[run-phase] Step 2/4: executor"
  EXEC_PROMPT="You are the UCIL executor. Implement the work-order at $LATEST_WO.
Work in a worktree; commit and push often; respect all anti-laziness rules.
When all acceptance criteria pass locally, write the ready-for-review marker and end."
  CLAUDE_SUBAGENT_NAME=executor claude -p "$EXEC_PROMPT" \
    --dangerously-skip-permissions \
    --append-system-prompt "$(cat .claude/agents/executor.md)" \
    >/tmp/ucil-executor.log 2>&1 || {
      echo "[run-phase] executor failed — see /tmp/ucil-executor.log"
      tail -30 /tmp/ucil-executor.log
      # Don't exit — let the next loop retry via planner/root-cause
    }

  # 3. Critic
  echo "[run-phase] Step 3/4: critic"
  CRIT_PROMPT="You are the UCIL critic. Review the executor's diff for work-order $LATEST_WO.
Apply every check in .claude/agents/critic.md. Write ucil-build/critic-reports/, commit, push."
  CLAUDE_SUBAGENT_NAME=critic claude -p "$CRIT_PROMPT" \
    --dangerously-skip-permissions \
    --append-system-prompt "$(cat .claude/agents/critic.md)" \
    >/tmp/ucil-critic.log 2>&1 || true

  # 4. Verifier (FRESH SESSION)
  echo "[run-phase] Step 4/4: verifier (fresh session)"
  WO_ID=$(jq -r .id "$LATEST_WO")
  scripts/spawn-verifier.sh "$WO_ID" >/tmp/ucil-verifier.log 2>&1 || true

  # Update drift counter
  FLIPPED_THIS_ITER=$(git log --since="5 minutes ago" --grep="flip-feature" --oneline 2>/dev/null | wc -l)
  if [[ "$FLIPPED_THIS_ITER" -eq 0 ]]; then
    NEW_DRIFT=$(jq -r --arg p "$PHASE" '.[$p] // 0 | tonumber + 1' "$DRIFT_FILE")
  else
    NEW_DRIFT=0
  fi
  jq --arg p "$PHASE" --argjson n "$NEW_DRIFT" '.[$p] = $n' "$DRIFT_FILE" > "${DRIFT_FILE}.tmp"
  mv "${DRIFT_FILE}.tmp" "$DRIFT_FILE"
  echo "[run-phase] drift counter for phase $PHASE: $NEW_DRIFT"
done
