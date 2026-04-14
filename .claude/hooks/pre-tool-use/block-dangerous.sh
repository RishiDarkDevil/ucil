#!/usr/bin/env bash
# PreToolUse hook for Bash: block destructive / irreversible commands.
set -euo pipefail

payload=$(cat)
command=$(echo "$payload" | jq -r '.tool_input.command // empty')

if [[ -z "$command" ]]; then
  exit 0
fi

# Patterns to block. Keep list precise to avoid false positives.
BLOCK_PATTERNS=(
  'rm\s+(-[a-zA-Z]*r[a-zA-Z]*f|-[a-zA-Z]*f[a-zA-Z]*r|-rf|-fr)\s+(/|\$HOME|~|/home|/etc|/usr|/var|/tmp/\*|\*)'
  'sudo\s+rm\s'
  'dd\s+.*of=/dev/'
  'mkfs\.'
  'git\s+push\s+(.*\s)?(-f|--force|--force-with-lease)\b'
  'git\s+commit\s+(.*\s)?--amend'
  'git\s+reset\s+--hard\s+(HEAD[~^]|origin/)'
  'cargo\s+publish\b'
  'npm\s+publish\b'
  'pnpm\s+publish\b'
  'docker\s+system\s+prune\s+(.*\s)?-a\b'
  'chmod\s+777\s'
  '>\s*/dev/sd[a-z]'
  ':\(\)\s*\{'
  'curl\s+.*\|\s*(sudo\s+)?(ba)?sh\s*$'
)

for pat in "${BLOCK_PATTERNS[@]}"; do
  if echo "$command" | grep -qE "$pat"; then
    jq -n --arg cmd "$command" --arg pattern "$pat" '{
      "decision": "block",
      "reason": "Dangerous command blocked by pre-tool-use/block-dangerous.sh.\nCommand: \($cmd)\nMatched pattern: \($pattern)\n\nIf this is genuinely needed, escalate to the user."
    }'
    exit 2
  fi
done

exit 0
