#!/usr/bin/env bash
# Launch the executor subagent for a single work-order.
#
# Usage:
#   scripts/run-executor.sh <work-order-id>
#
# Example:
#   scripts/run-executor.sh WO-0001
#   scripts/run-executor.sh 0001          # accepts bare NNNN too
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

WO_ARG="${1:-}"
if [[ -z "$WO_ARG" ]]; then
  echo "Usage: $0 <work-order-id>   (e.g. WO-0001 or 0001)" >&2
  exit 2
fi

# Normalise: accept "WO-0001" or "0001"
WO_ID="${WO_ARG#WO-}"
WO_ID="WO-${WO_ID}"

# Locate the work-order JSON
WO_FILE=$(ls ucil-build/work-orders/${WO_ID#WO-}-*.json 2>/dev/null | head -1)
if [[ -z "$WO_FILE" ]]; then
  echo "ERROR: no work-order JSON matching ucil-build/work-orders/${WO_ID#WO-}-*.json" >&2
  echo "Existing work-orders:" >&2
  ls ucil-build/work-orders/ 2>/dev/null >&2
  exit 3
fi

if ! command -v claude >/dev/null 2>&1; then
  echo "ERROR: 'claude' CLI not in PATH." >&2
  exit 3
fi

# shellcheck source=scripts/_load-auth.sh
source "$(dirname "$0")/_load-auth.sh"

LOG="/tmp/ucil-executor-${WO_ID}.log"

PROMPT=$(cat <<EOF
You are the UCIL executor. Implement the work-order at ${WO_FILE}.

Rules of engagement (see .claude/agents/executor.md for the full contract):
1. Create a git worktree: git worktree add ../ucil-wt/${WO_ID} -b feat/${WO_ID}-\$(jq -r .slug ${WO_FILE}) main, then cd into it.
2. Read the work-order JSON. Memorize feature_ids, acceptance (or acceptance_criteria), scope_in, scope_out, forbidden_paths.
3. Implement iteratively. Commit every ~50 lines of diff with Conventional Commits format. Push after every commit.
4. Never stub with todo!()/unimplemented!()/NotImplementedError/pass-only. Never #[ignore] or skip tests. Never modify feature-list.json, master plan, or tests/fixtures/**.
5. When EVERY acceptance criterion passes locally (run each one yourself):
   - Write ucil-build/work-orders/${WO_ID#WO-}-ready-for-review.md with the final commit sha and a bullet list of "what I verified locally".
   - Commit and push that marker.
   - End the session cleanly.

Work-order path: ${WO_FILE}
Worktree target: ../ucil-wt/${WO_ID}
Branch: feat/${WO_ID}-<slug-from-work-order>

If you get blocked, write an escalation to ucil-build/escalations/ rather than stubbing. Do not fake progress.
EOF
)

# Clean up any stale worktree from a prior attempt. Git refuses `worktree add`
# if the target path already exists, which otherwise stalls a retry.
WT_PATH="../ucil-wt/${WO_ID}"
SLUG=$(jq -r .slug "$WO_FILE" 2>/dev/null || echo "")
BRANCH="feat/${WO_ID}-${SLUG}"
if [[ -d "$WT_PATH" ]]; then
  echo "[run-executor] stale worktree at $WT_PATH — removing before retry"
  git worktree remove --force "$WT_PATH" 2>/dev/null || true
  rm -rf "$WT_PATH"
fi
# Branch may still exist locally even if worktree is gone. For a clean retry,
# let the executor reuse the existing branch (keeps prior commits) rather than
# deleting it — executor's workflow will `git worktree add` and cd in.
# If the branch itself needs rebuilding (e.g. force-restart), delete manually.

echo "[run-executor] work-order: ${WO_FILE}"
echo "[run-executor] log: ${LOG}"
echo "[run-executor] starting in ~5s..."
sleep 5

UCIL_WO_ID="${WO_ID}" CLAUDE_CODE_ENABLE_TELEMETRY=1 \
CLAUDE_SUBAGENT_NAME=executor \
exec claude -p "$PROMPT" \
  --dangerously-skip-permissions \
  --append-system-prompt "$(cat .claude/agents/executor.md)" \
  2>&1 | tee "$LOG"
