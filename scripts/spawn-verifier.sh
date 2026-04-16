#!/usr/bin/env bash
# Spawn a FRESH Claude Code session as the verifier subagent.
# - Generates a new session ID distinct from the caller
# - Writes ucil-build/.verifier-lock with the session ID so feature-list-guard
#   and flip-feature.sh accept its writes
# - Launches `claude -p <prompt> --session-id=<new> ` with the verifier
#   subagent prompt loaded via --append-system-prompt
#
# Usage:
#   scripts/spawn-verifier.sh <work-order-id-or-feature-id> [extra args]
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

TARGET="${1:-}"
if [[ -z "$TARGET" ]]; then
  echo "Usage: $0 <work-order-id|feature-id>" >&2
  exit 2
fi
shift

if ! command -v claude >/dev/null 2>&1; then
  echo "ERROR: 'claude' CLI not found in PATH. Install Claude Code first." >&2
  exit 3
fi

# shellcheck source=scripts/_load-auth.sh
source "$(dirname "$0")/_load-auth.sh"

# Generate a fresh session id — Claude CLI requires a valid UUID.
if command -v uuidgen >/dev/null 2>&1; then
  NEW_SESSION=$(uuidgen)
else
  # Fallback: RFC 4122 v4 UUID from /proc/sys/kernel/random/uuid
  NEW_SESSION=$(cat /proc/sys/kernel/random/uuid 2>/dev/null)
fi
if [[ -z "$NEW_SESSION" ]]; then
  echo "ERROR: could not generate a UUID (install uuid-runtime)" >&2
  exit 4
fi
MARKER_FILE="ucil-build/.verifier-lock"

echo "$NEW_SESSION" > "$MARKER_FILE"
trap 'rm -f "$MARKER_FILE"' EXIT

# Assert caller is not the same session as the new one (belt-and-suspenders)
CALLER_SESSION="${CLAUDE_SESSION_ID:-parent}"
if [[ "$CALLER_SESSION" == "$NEW_SESSION" ]]; then
  echo "ERROR: session-id collision (caller=$CALLER_SESSION, new=$NEW_SESSION)" >&2
  exit 4
fi

PROMPT="You are the UCIL verifier. Target to verify: $TARGET.
Read .claude/agents/verifier.md for your full instructions.
Read ucil-build/work-orders/ or search for feature $TARGET in feature-list.json to find context.
Run all acceptance tests from a clean slate. Flip passes=true only if everything is green and the mutation check confirms.
DO NOT edit any source code. Write your verification report and/or rejection, commit, push, end session."

echo "[spawn-verifier] new session: $NEW_SESSION"
echo "[spawn-verifier] target: $TARGET"

UCIL_WO_ID="${TARGET}" CLAUDE_CODE_ENABLE_TELEMETRY=1 \
CLAUDE_SUBAGENT_NAME=verifier \
CLAUDE_SESSION_ID="$NEW_SESSION" \
exec claude -p "$PROMPT" \
  --model "${CLAUDE_CODE_MODEL:-claude-opus-4-7}" \
  --session-id "$NEW_SESSION" \
  --dangerously-skip-permissions \
  --append-system-prompt "$(cat .claude/agents/verifier.md)" \
  "$@"
