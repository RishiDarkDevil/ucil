#!/usr/bin/env bash
# Source this from any launcher that needs Claude auth. Idempotent.
#
# Priority for CLAUDE_CODE_OAUTH_TOKEN (first non-empty wins):
#   1. Existing env var (from parent shell — caller already set it)
#   2. `~/.claude/.credentials.json` (authoritative; updated when Claude Code
#      refreshes your OAuth session — avoids stale tokens in .env)
#   3. `.env` (fallback if credentials.json is absent/unparseable)
#
# Also loads `.env` for non-auth vars (GITHUB_TOKEN, repo settings).
#
# ANTHROPIC_API_KEY is loaded from .env only (no credentials.json equivalent).
#
# Usage:
#   source "$(dirname "$0")/_load-auth.sh"
#
# Exits 3 if no auth can be resolved.

_ucil_auth_log() { printf '[_load-auth] %s\n' "$*" >&2; }

# 1. Source .env for GITHUB_TOKEN / ANTHROPIC_API_KEY / repo settings.
if [[ -f .env ]]; then
  set -a
  # shellcheck disable=SC1091
  source .env
  set +a
fi

# 2. Always prefer credentials.json for OAuth token — read fresh each time.
if [[ -f "$HOME/.claude/.credentials.json" ]] && command -v jq >/dev/null 2>&1; then
  _ucil_fresh_token=$(jq -r '.claudeAiOauth.accessToken // empty' "$HOME/.claude/.credentials.json" 2>/dev/null)
  if [[ -n "$_ucil_fresh_token" ]]; then
    if [[ -n "${CLAUDE_CODE_OAUTH_TOKEN:-}" ]] && [[ "${CLAUDE_CODE_OAUTH_TOKEN}" != "$_ucil_fresh_token" ]]; then
      _ucil_auth_log "overriding .env CLAUDE_CODE_OAUTH_TOKEN with fresher token from ~/.claude/.credentials.json"
    fi
    export CLAUDE_CODE_OAUTH_TOKEN="$_ucil_fresh_token"
  fi
  unset _ucil_fresh_token
fi

# 3. Final check — at least one auth path must resolve.
if [[ -z "${CLAUDE_CODE_OAUTH_TOKEN:-}" && -z "${ANTHROPIC_API_KEY:-}" ]]; then
  _ucil_auth_log "ERROR: no CLAUDE_CODE_OAUTH_TOKEN (tried ~/.claude/.credentials.json and .env) and no ANTHROPIC_API_KEY in .env"
  return 3 2>/dev/null || exit 3
fi

unset -f _ucil_auth_log

# 4. W3C trace-context propagation + OTel env wiring for child claude -p
# sessions. Always attempted (idempotent + cheap). Only actually exports OTel
# bits if OTEL_ENABLED=1 in the environment.
# shellcheck source=scripts/_trace.sh
source "$(dirname "${BASH_SOURCE[0]}")/_trace.sh"
