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

  # 4. Verifier (FRESH SESSION) — with rejection retry loop.
  # The verifier may reject on first run. If it does and no feature's
  # attempts has hit 3, spawn root-cause-finder and re-run
  # executor → critic → verifier up to MAX_VERIFIER_ATTEMPTS times.
  WO_ID=$(jq -r .id "$LATEST_WO")
  MAX_VERIFIER_ATTEMPTS=3
  vattempt=1
  while true; do
    echo "[run-phase] Step 4/4: verifier (fresh session, attempt ${vattempt})"
    git pull --quiet 2>/dev/null || true  # stay current with any recent agent pushes
    scripts/spawn-verifier.sh "$WO_ID" >/tmp/ucil-verifier.log 2>&1 || true
    git pull --quiet 2>/dev/null || true  # pick up verifier's commits on main if any

    # Determine outcome: did all feature_ids in the WO flip to passes=true?
    WO_FEATURES=$(jq -r '.feature_ids // .features // [] | join(" ")' "$LATEST_WO")
    all_pass=1
    max_attempts=0
    for fid in $WO_FEATURES; do
      p=$(jq -r --arg id "$fid" '.features[] | select(.id==$id) | .passes' ucil-build/feature-list.json 2>/dev/null)
      a=$(jq -r --arg id "$fid" '.features[] | select(.id==$id) | .attempts // 0' ucil-build/feature-list.json 2>/dev/null)
      [[ "$p" != "true" ]] && all_pass=0
      [[ "$a" -gt "$max_attempts" ]] && max_attempts="$a"
    done

    if [[ "$all_pass" -eq 1 ]]; then
      echo "[run-phase] verifier PASS — all ${WO_FEATURES} features flipped."
      # Merge feat → main.
      echo "[run-phase] Step 5/5: merge ${WO_ID} → main"
      if ! scripts/merge-wo.sh "$WO_ID"; then
        echo "[run-phase] merge-wo failed (escalation filed). Halting loop."
        exit 1
      fi
      git pull --quiet 2>/dev/null || true
      break  # proceed to drift counter / next iteration
    fi

    # Verifier REJECTED. Decide whether to retry.
    if [[ "$vattempt" -ge "$MAX_VERIFIER_ATTEMPTS" ]] || [[ "$max_attempts" -ge 3 ]]; then
      echo "[run-phase] verifier rejected ${WO_ID} — attempts_cap reached (v=${vattempt}, feature_max=${max_attempts}). Escalating."
      mkdir -p ucil-build/escalations
      ESC="ucil-build/escalations/$(date -u +%Y%m%d-%H%M)-wo-${WO_ID}-attempts-exhausted.md"
      cat > "$ESC" <<EOF
---
timestamp: $(date -u +%Y-%m-%dT%H:%M:%SZ)
type: verifier-rejects-exhausted
work_order: ${WO_ID}
verifier_attempts: ${vattempt}
max_feature_attempts: ${max_attempts}
severity: high
blocks_loop: true
---

# ${WO_ID} hit verifier-reject cap

Verifier ran ${vattempt} times on ${WO_ID}; at least one feature has
attempts=${max_attempts}. Halting autonomous loop for human review.

Latest rejection: ucil-build/rejections/${WO_ID}.md
Latest root-cause: ucil-build/verification-reports/root-cause-${WO_ID}.md (if present)
EOF
      git add "$ESC" 2>/dev/null || true
      git commit -m "chore(escalation): ${WO_ID} verifier attempts exhausted" 2>/dev/null || true
      git push --quiet 2>/dev/null || true
      exit 1
    fi

    echo "[run-phase] verifier rejected ${WO_ID} (v=${vattempt}, feature_max=${max_attempts}). Spawning root-cause-finder."
    scripts/run-root-cause-finder.sh "$WO_ID" >/tmp/ucil-rcf.log 2>&1 || true
    git pull --quiet 2>/dev/null || true

    echo "[run-phase] Re-running executor with RCF context (attempt $((vattempt+1)))"
    RETRY_PROMPT="You are the UCIL executor. Implement work-order at $LATEST_WO.
A PRIOR verifier attempt rejected your work. Read:
  - ucil-build/rejections/${WO_ID}.md — the rejection itself
  - ucil-build/verification-reports/root-cause-${WO_ID}.md — root-cause-finder's diagnosis and recommended remediation
Apply the RCF's recommended remediation, commit + push incrementally, re-write
ucil-build/work-orders/$(basename "$LATEST_WO" .json)-ready-for-review.md when all
acceptance criteria pass locally, and end cleanly. Reuse the existing worktree
at ../ucil-wt/${WO_ID} (scripts/run-executor.sh cleans stale state already)."
    CLAUDE_SUBAGENT_NAME=executor claude -p "$RETRY_PROMPT" \
      --dangerously-skip-permissions \
      --append-system-prompt "$(cat .claude/agents/executor.md)" \
      >/tmp/ucil-executor-retry.log 2>&1 || {
        echo "[run-phase] executor retry failed — see /tmp/ucil-executor-retry.log"
      }
    git pull --quiet 2>/dev/null || true

    echo "[run-phase] Re-running critic on retried WO"
    RETRY_CRIT_PROMPT="You are the UCIL critic. Re-review the executor's diff for work-order $LATEST_WO
after retry attempt ${vattempt}. Apply every check in .claude/agents/critic.md.
Overwrite ucil-build/critic-reports/${WO_ID}.md with the fresh review, commit, push."
    CLAUDE_SUBAGENT_NAME=critic claude -p "$RETRY_CRIT_PROMPT" \
      --dangerously-skip-permissions \
      --append-system-prompt "$(cat .claude/agents/critic.md)" \
      >/tmp/ucil-critic-retry.log 2>&1 || true
    git pull --quiet 2>/dev/null || true

    vattempt=$((vattempt+1))
    # loop continues → re-spawns verifier
  done

  # Update drift counter — only after a successful merge (a rejected WO
  # counts as no flip for drift purposes).
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
