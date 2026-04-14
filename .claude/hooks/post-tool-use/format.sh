#!/usr/bin/env bash
# PostToolUse hook: auto-format + quick-lint on every file write.
# Best-effort: missing tools don't fail the hook (just skip).
set -uo pipefail

payload=$(cat)
file=$(echo "$payload" | jq -r '.tool_input.file_path // .tool_input.path // empty')

if [[ -z "$file" || ! -f "$file" ]]; then
  exit 0
fi

REPO_ROOT="${CLAUDE_PROJECT_DIR:-$PWD}"
cd "$REPO_ROOT" || exit 0

case "$file" in
  *.rs)
    if command -v rustfmt >/dev/null 2>&1; then
      rustfmt --edition 2021 --quiet "$file" 2>/dev/null || true
    fi
    ;;
  *.ts|*.tsx|*.js|*.jsx|*.json)
    if command -v biome >/dev/null 2>&1; then
      biome format --write --no-colors "$file" 2>/dev/null || true
    elif command -v npx >/dev/null 2>&1; then
      npx --quiet biome format --write --no-colors "$file" 2>/dev/null || true
    fi
    ;;
  *.py)
    if command -v ruff >/dev/null 2>&1; then
      ruff format --quiet "$file" 2>/dev/null || true
      ruff check --fix --quiet "$file" 2>/dev/null || true
    fi
    ;;
  *.sh)
    if command -v shfmt >/dev/null 2>&1; then
      shfmt -w "$file" 2>/dev/null || true
    fi
    ;;
esac

exit 0
