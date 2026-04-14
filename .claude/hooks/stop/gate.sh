#!/usr/bin/env bash
# Stop hook: refuse to end the turn if:
#   1. Working tree is dirty (uncommitted changes).
#   2. Current branch is ahead of its upstream (unpushed commits).
#   3. The phase gate script reports failure (scripts/gate-check.sh $PHASE).
#
# Planner / critic / docs-writer are exempt from (3) — they produce artifacts
# that don't build code.
set -uo pipefail

REPO_ROOT="${CLAUDE_PROJECT_DIR:-$PWD}"
cd "$REPO_ROOT" || exit 0

role="${CLAUDE_SUBAGENT_NAME:-main}"

# Allow "stop_hook_active" — prevent recursion if we've been called during a block
if [[ "${CLAUDE_STOP_HOOK_ACTIVE:-}" == "1" ]]; then
  exit 0
fi

# Bypass all Stop checks during one-shot seeding: the planner writes
# feature-list.json but leaves it uncommitted for human review.
if [[ "${UCIL_SEEDING:-}" == "1" ]]; then
  exit 0
fi

# --- Check 1: dirty tree ---
if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  DIRTY=$(git status --porcelain 2>/dev/null | wc -l)
  if [[ "$DIRTY" -gt 0 ]]; then
    jq -n --argjson n "$DIRTY" '{
      "decision": "block",
      "reason": ("Working tree has \($n) uncommitted change(s). Commit with Conventional Commits format and push before ending the session. If mid-feature, use a `wip:` prefix commit.")
    }'
    exit 2
  fi

  # --- Check 2: unpushed commits ---
  if git rev-parse --abbrev-ref '@{u}' >/dev/null 2>&1; then
    AHEAD=$(git rev-list '@{u}..HEAD' 2>/dev/null | wc -l)
    if [[ "$AHEAD" -gt 0 ]]; then
      jq -n --argjson n "$AHEAD" '{
        "decision": "block",
        "reason": ("Branch is \($n) commit(s) ahead of upstream. Push before ending the session: `git push`.")
      }'
      exit 2
    fi
  fi
fi

# --- Check 3: phase gate (skip for non-code roles) ---
case "$role" in
  planner|critic|docs-writer)
    exit 0
    ;;
esac

# Only enforce gate if progress.json exists and has a phase
if [[ ! -f ucil-build/progress.json ]] || ! command -v jq >/dev/null 2>&1; then
  exit 0
fi

PHASE=$(jq -r '.phase // empty' ucil-build/progress.json 2>/dev/null)
if [[ -z "$PHASE" || "$PHASE" == "null" ]]; then
  exit 0
fi

# Only run gate if the phase-specific check script exists (skip pre-Phase-0 bootstrap)
if [[ ! -x "scripts/gate/phase-${PHASE}.sh" && ! -f "scripts/gate/phase-${PHASE}.sh" ]]; then
  exit 0
fi

# If the orchestrator explicitly marks gate-passed this session, allow
if [[ "${UCIL_GATE_SKIP:-}" == "1" ]]; then
  exit 0
fi

if ! CLAUDE_STOP_HOOK_ACTIVE=1 scripts/gate-check.sh "$PHASE" > /tmp/ucil-gate-check.log 2>&1; then
  TAIL=$(tail -n 40 /tmp/ucil-gate-check.log 2>/dev/null | head -c 3500)
  jq -n --arg log "$TAIL" --arg p "$PHASE" '{
    "decision": "block",
    "reason": ("Phase \($p) gate failed. You cannot end this session until the gate is green.\nTail of /tmp/ucil-gate-check.log:\n"+$log+"\n\nOptions: continue fixing, or write an escalation to ucil-build/escalations/ and commit it (escalation files are tracked).")
  }'
  exit 2
fi

exit 0
