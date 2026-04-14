#!/usr/bin/env bash
# PostToolUse hook: if a Write/Edit touched feature-list.json, confirm it was
# performed by the verifier subagent with the session marker. Otherwise, revert
# and block.
set -euo pipefail

payload=$(cat)
file=$(echo "$payload" | jq -r '.tool_input.file_path // .tool_input.path // empty')

if [[ "$file" != *"feature-list.json" ]]; then
  exit 0
fi

REPO_ROOT="${CLAUDE_PROJECT_DIR:-$PWD}"
cd "$REPO_ROOT" || exit 0

role="${CLAUDE_SUBAGENT_NAME:-main}"
session="${CLAUDE_SESSION_ID:-unknown}"
marker_file="ucil-build/.verifier-lock"

# Seeding bypass — only the one-shot seed-features.sh can set UCIL_SEEDING=1
if [[ "${UCIL_SEEDING:-}" == "1" ]]; then
  exit 0
fi

if [[ "$role" == "verifier" ]] && [[ -f "$marker_file" ]]; then
  lock_session=$(cat "$marker_file" 2>/dev/null || echo "")
  if [[ -n "$lock_session" && "$lock_session" == "$session" ]]; then
    exit 0
  fi
fi

# Violation — revert the change if possible
if git rev-parse --verify HEAD >/dev/null 2>&1; then
  git checkout HEAD -- "$file" 2>/dev/null || true
fi

jq -n --arg r "$role" --arg s "$session" '{
  "decision": "block",
  "reason": ("feature-list.json is verifier-only. Your role is \""+$r+"\" (session "+$s+"). "
    + "Revert attempted. To flip passes=true on a feature, have the orchestrator spawn the "
    + "verifier subagent via scripts/spawn-verifier.sh, which writes the session marker.")
}'
exit 2
