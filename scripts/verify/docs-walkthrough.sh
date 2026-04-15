#!/usr/bin/env bash
# Docs walkthrough — a simulated new user reads only the docs/ and tries
# to install+use UCIL. Where do they get stuck?
#
# Contract (implement by Phase 8):
#   1. Spawn a FRESH claude -p session with no prior UCIL knowledge.
#   2. Mount docs/ as the only reference. Explicitly deny reading anything
#      outside docs/ (via a tool allowlist).
#   3. Give the task: "Install UCIL, set it up on a Python project, and
#      show me how to use the find_definition tool from Claude Code."
#   4. The session can only use what docs/ says. If it's unsure, it
#      reports "docs unclear: <topic>".
#   5. Agent must complete the task in <= 40 turns.
#   6. A judge session (fresh) reviews the transcript and scores:
#        - completion: did it work end-to-end? (0/1)
#        - clarity: did the session get stuck or wander?
#        - gaps: list every "docs unclear" the session reported
#   7. Fail the gate if:
#        - completion == 0
#        - OR number of `docs unclear` findings > 0
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
PHASE="${1:-$(jq -r .phase ucil-build/progress.json)}"

case "$PHASE" in
  8) ;;
  *) echo "[docs-walkthrough] phase $PHASE: not required"; exit 0 ;;
esac

echo "[docs-walkthrough] phase=$PHASE"
echo "[docs-walkthrough] TODO: simulated-new-user docs trial; Phase 8 gate."
exit 1
