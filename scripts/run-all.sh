#!/usr/bin/env bash
# Run all phases 0 -> 8 sequentially. Between phases, pause for human confirmation
# unless --yes is passed.
set -uo pipefail

cd "$(git rev-parse --show-toplevel)"

# Optional daily cost-budget guard (worker-B deliverable). Source if present.
if [[ -f "$(dirname "$0")/_cost-budget.sh" ]]; then
  # shellcheck source=scripts/_cost-budget.sh
  source "$(dirname "$0")/_cost-budget.sh"
fi

AUTO_CONFIRM=0
for arg in "$@"; do
  case "$arg" in
    --yes|-y) AUTO_CONFIRM=1 ;;
  esac
done

PHASES=(0 1 2 3 3.5 4 5 6 7 8)

for P in "${PHASES[@]}"; do
  echo ""
  echo "=== Starting phase $P ==="

  # Budget guard (soft-fail: if cost-budget helper missing, just continue).
  if declare -f safe_check_daily_budget >/dev/null 2>&1; then
    if ! safe_check_daily_budget; then
      echo "=== Phase $P blocked: daily cost-budget exhausted ==="
      echo "Review ucil-build/escalations/ and retry tomorrow (or bump the cap via ADR)."
      exit 1
    fi
  fi

  # Ensure progress.json reflects the phase
  jq --arg p "$P" '.phase = ($p | tonumber? // $p) | .week = 1' ucil-build/progress.json > /tmp/prog.json
  mv /tmp/prog.json ucil-build/progress.json
  git add ucil-build/progress.json && git commit -m "chore: enter phase $P" --quiet || true
  git push --quiet || true

  # Run the phase loop
  if ! scripts/run-phase.sh "$P"; then
    echo ""
    echo "=== Phase $P halted (gate failed or escalation) ==="
    echo "Review ucil-build/escalations/ and fix before continuing."
    exit 1
  fi

  # Ship
  echo "=== Phase $P shipped ==="

  # Checkpoint tag: snapshot progress.json + feature-list.json state on main
  # so we can always rewind if a later phase goes bad. The `phase-N-complete`
  # tag covers the same commit; `checkpoint-phase-N` is a pure rewind anchor
  # that survives even if the completion tag is later re-scoped.
  _checkpoint_tag="checkpoint-phase-${P}"
  if ! git rev-parse --verify "$_checkpoint_tag" >/dev/null 2>&1; then
    git tag -a "$_checkpoint_tag" -m "Checkpoint at end of phase $P — progress.json + feature-list.json snapshot." 2>/dev/null || true
    git push origin "$_checkpoint_tag" 2>/dev/null || true
  fi

  git tag -a "phase-${P}-complete" -m "Phase $P of UCIL complete." --force
  git push origin "phase-${P}-complete" || true

  # Docs-writer: post-mortem
  PROMPT="You are the UCIL docs-writer. Phase $P shipped. Draft ucil-build/post-mortems/phase-${P}.md from verification reports and git log. Commit and push."
  CLAUDE_SUBAGENT_NAME=docs-writer claude -p "$PROMPT" \
    --dangerously-skip-permissions \
    --append-system-prompt "$(cat .claude/agents/docs-writer.md)" >/tmp/ucil-postmortem.log 2>&1 || true

  # Advance phase counter
  NEXT_IDX=$(printf '%s\n' "${PHASES[@]}" | grep -n "^$P$" | cut -d: -f1)
  NEXT=$((NEXT_IDX))
  if [[ "$NEXT" -lt "${#PHASES[@]}" ]]; then
    NEXT_PHASE="${PHASES[$NEXT]}"
  else
    echo "=== All phases shipped. UCIL v0.1.0 is complete. ==="
    break
  fi

  if [[ "$AUTO_CONFIRM" -eq 0 ]]; then
    echo ""
    echo "Review ucil-build/post-mortems/phase-${P}.md."
    read -r -p "Proceed to phase $NEXT_PHASE? [y/N] " ANS
    [[ "$ANS" == "y" || "$ANS" == "Y" ]] || { echo "Stopped at user request."; exit 0; }
  fi
done
