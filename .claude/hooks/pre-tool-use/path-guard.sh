#!/usr/bin/env bash
# PreToolUse hook for Write/Edit: block writes to the master plan and to fixtures
# when executor is active.
set -euo pipefail

payload=$(cat)
file=$(echo "$payload" | jq -r '.tool_input.file_path // .tool_input.path // empty')

if [[ -z "$file" ]]; then
  exit 0
fi

REPO_ROOT="${CLAUDE_PROJECT_DIR:-$PWD}"
# Normalize to relative path from repo root when possible
case "$file" in
  "$REPO_ROOT"/*) rel="${file#"$REPO_ROOT"/}" ;;
  /*)             rel="$file" ;;
  *)              rel="$file" ;;
esac

# 1. Master plan is read-only for everyone
if [[ "$rel" == "ucil-master-plan-v2.1-final.md" ]]; then
  jq -n --arg f "$file" '{
    "decision": "block",
    "reason": "The master plan (\($f)) is immutable. If you need to refine it, write an ADR at ucil-build/decisions/proposed-*.md instead."
  }'
  exit 2
fi

# 2. Fixtures are locked for executor role (but allowed for initial seeding by planner/tester)
if [[ "$rel" == tests/fixtures/* ]]; then
  role="${CLAUDE_SUBAGENT_NAME:-main}"
  if [[ "$role" == "executor" ]]; then
    jq -n --arg f "$file" '{
      "decision": "block",
      "reason": "Fixtures (\($f)) are part of the spec. Executor agents MUST NOT modify them to make tests pass. If a fixture is wrong, write an ADR and escalate."
    }'
    exit 2
  fi
fi

# 3. Feature list is verifier-only (belt-and-suspenders; post-tool-use also checks)
if [[ "$rel" == "ucil-build/feature-list.json" ]]; then
  role="${CLAUDE_SUBAGENT_NAME:-main}"
  if [[ "$role" != "verifier" && "${UCIL_SEEDING:-}" != "1" ]]; then
    jq -n '{
      "decision": "block",
      "reason": "feature-list.json is verifier-only. Spawn the verifier subagent via scripts/spawn-verifier.sh to flip passes=true."
    }'
    exit 2
  fi
fi

exit 0
