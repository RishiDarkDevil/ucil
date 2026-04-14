#!/usr/bin/env bash
# Sole codepath that flips passes=true on a feature.
# Asserts:
#   - Caller subagent name is "verifier"
#   - Verifier session marker matches caller's session id
#   - Verifier session differs from the commit's author session (no self-verification)
#
# Usage: scripts/flip-feature.sh <feature-id> pass|fail [<commit-sha>]
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

FEATURE_ID="${1:-}"
VERDICT="${2:-}"
COMMIT_SHA="${3:-$(git rev-parse HEAD)}"

if [[ -z "$FEATURE_ID" || -z "$VERDICT" ]]; then
  echo "Usage: $0 <feature-id> pass|fail [commit-sha]" >&2
  exit 2
fi

if [[ "$VERDICT" != "pass" && "$VERDICT" != "fail" ]]; then
  echo "Verdict must be 'pass' or 'fail' (got: $VERDICT)" >&2
  exit 2
fi

ROLE="${CLAUDE_SUBAGENT_NAME:-}"
SESSION="${CLAUDE_SESSION_ID:-}"
MARKER_FILE="ucil-build/.verifier-lock"

# 1. Role check
if [[ "$ROLE" != "verifier" ]]; then
  echo "ERROR: flip-feature.sh called by role='$ROLE' (must be 'verifier')." >&2
  exit 3
fi

# 2. Session marker check
if [[ ! -f "$MARKER_FILE" ]]; then
  echo "ERROR: no verifier session marker at $MARKER_FILE. Were you spawned via scripts/spawn-verifier.sh?" >&2
  exit 3
fi
MARKER=$(cat "$MARKER_FILE")
if [[ "$MARKER" != "$SESSION" ]]; then
  echo "ERROR: session marker mismatch (marker=$MARKER, session=$SESSION)." >&2
  exit 3
fi

# 3. Self-verification check: commit author trailer must not reference this session
if git cat-file -e "$COMMIT_SHA" 2>/dev/null; then
  COMMIT_SESSION=$(git log -1 --format='%b' "$COMMIT_SHA" | grep -oE 'Claude-Session-Id: .+' | head -1 | awk '{print $2}' || true)
  if [[ -n "$COMMIT_SESSION" && "$COMMIT_SESSION" == "$SESSION" ]]; then
    echo "ERROR: self-verification detected. Commit session ($COMMIT_SESSION) == verifier session ($SESSION)." >&2
    exit 3
  fi
fi

# 4. Feature-list integrity
if [[ ! -f ucil-build/feature-list.json ]]; then
  echo "ERROR: ucil-build/feature-list.json missing." >&2
  exit 4
fi

if ! jq -e --arg id "$FEATURE_ID" '.features[] | select(.id == $id)' ucil-build/feature-list.json >/dev/null; then
  echo "ERROR: feature $FEATURE_ID not found in feature-list.json." >&2
  exit 4
fi

# 5. Perform the flip
NOW=$(date -u +'%Y-%m-%dT%H:%M:%SZ')

TMP=$(mktemp)
if [[ "$VERDICT" == "pass" ]]; then
  jq --arg id "$FEATURE_ID" \
     --arg ts "$NOW" \
     --arg by "verifier-$SESSION" \
     --arg sha "$COMMIT_SHA" '
    .features |= map(if .id == $id then
      .passes = true
      | .last_verified_ts = $ts
      | .last_verified_by = $by
      | .last_verified_commit = $sha
      | .attempts = (.attempts // 0) + 0
      | .blocked_reason = null
    else . end)
  ' ucil-build/feature-list.json > "$TMP"
else
  jq --arg id "$FEATURE_ID" \
     --arg ts "$NOW" \
     --arg by "verifier-$SESSION" \
     --arg sha "$COMMIT_SHA" '
    .features |= map(if .id == $id then
      .passes = false
      | .last_verified_ts = $ts
      | .last_verified_by = $by
      | .last_verified_commit = $sha
      | .attempts = (.attempts // 0) + 1
    else . end)
  ' ucil-build/feature-list.json > "$TMP"
fi

mv "$TMP" ucil-build/feature-list.json

echo "[flip-feature] $FEATURE_ID -> $VERDICT (by verifier-$SESSION)"
