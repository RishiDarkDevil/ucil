#!/usr/bin/env bash
# Run all phases 0 -> 8 sequentially. Between phases, pause for human confirmation
# unless --yes is passed.
set -uo pipefail

cd "$(git rev-parse --show-toplevel)"

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
  git tag -a "phase-${P}-complete" -m "Phase $P of UCIL complete." --force
  git push origin "phase-${P}-complete" || true

  # Docs-writer: post-mortem
  PROMPT="You are the UCIL docs-writer. Phase $P shipped. Draft ucil-build/post-mortems/phase-${P}.md from verification reports and git log. Commit and push."
  CLAUDE_SUBAGENT_NAME=docs-writer claude -p "$PROMPT" \
    --no-resume \
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
