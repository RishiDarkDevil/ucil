#!/usr/bin/env bash
# SessionStart hook: auto-prune merged worktrees + warn if disk getting full.
# Runs on every Claude Code session start/resume so disk doesn't fill
# between WO merges (where the merge-wo path normally calls prune).
set -uo pipefail

REPO_ROOT="${CLAUDE_PROJECT_DIR:-$PWD}"
cd "$REPO_ROOT" || exit 0

# Cheap disk check first — if we have plenty of space, skip the prune.
USE_PCT=$(df / 2>/dev/null | awk 'NR==2 {gsub("%",""); print $5}' || echo 0)
if [[ "$USE_PCT" -ge 70 ]]; then
  echo "[auto-prune] disk at ${USE_PCT}% — running prune-merged-worktrees..."
  if [[ -x scripts/prune-merged-worktrees.sh ]]; then
    scripts/prune-merged-worktrees.sh 2>&1 | tail -5 || true
    NEW_PCT=$(df / 2>/dev/null | awk 'NR==2 {gsub("%",""); print $5}' || echo "?")
    echo "[auto-prune] disk now at ${NEW_PCT}%"
  fi
fi

# Always print disk status as part of session dashboard.
df -h / 2>/dev/null | tail -1 | awk '{printf "  Disk: %s used / %s (%s)\n", $3, $2, $5}'

exit 0
