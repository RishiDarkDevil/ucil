#!/usr/bin/env bash
# UserPromptSubmit hook: surface open escalations as context
set -euo pipefail

REPO_ROOT="${CLAUDE_PROJECT_DIR:-$PWD}"
cd "$REPO_ROOT" || exit 0

if [[ ! -d ucil-build/escalations ]]; then
  exit 0
fi

ESC_COUNT=$(ls -1 ucil-build/escalations/ 2>/dev/null | wc -l)
if [[ "$ESC_COUNT" -eq 0 ]]; then
  exit 0
fi

# Emit as "additionalContext" that Claude will read
# Protocol: print to stdout, exit 0 = allow prompt
echo "⚠️  $ESC_COUNT open escalation(s) in ucil-build/escalations/:"
for f in ucil-build/escalations/*; do
  [[ -f "$f" ]] && echo "  - $(basename "$f"): $(head -1 "$f" | head -c 100)"
done

exit 0
