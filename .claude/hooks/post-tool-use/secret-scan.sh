#!/usr/bin/env bash
# PostToolUse hook: scan a freshly-written file for obvious secrets.
set -uo pipefail

payload=$(cat)
file=$(echo "$payload" | jq -r '.tool_input.file_path // .tool_input.path // empty')

if [[ -z "$file" || ! -f "$file" ]]; then
  exit 0
fi

# Cheap regex scan (gitleaks used in pre-commit for authoritative coverage)
PATTERNS=(
  'sk-ant-[a-zA-Z0-9_-]{20,}'
  'ghp_[a-zA-Z0-9]{36}'
  'github_pat_[a-zA-Z0-9_]{22,}'
  'AKIA[0-9A-Z]{16}'
  '-----BEGIN (RSA |OPENSSH |DSA |EC )?PRIVATE KEY-----'
  'xox[baprs]-[a-zA-Z0-9-]{10,}'
)

HITS=""
for p in "${PATTERNS[@]}"; do
  line=$(grep -nE -e "$p" "$file" 2>/dev/null | head -3 || true)
  [[ -n "$line" ]] && HITS+="Pattern '$p' in $file:"$'\n'"$line"$'\n'
done

if [[ -n "$HITS" ]]; then
  jq -n --arg details "$HITS" --arg f "$file" '{
    "decision": "block",
    "reason": ("Suspected secret(s) in just-written file \($f):\n"+$details+"\nReview and remove before continuing.")
  }'
  exit 2
fi

exit 0
