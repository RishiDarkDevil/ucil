#!/usr/bin/env bash
# Optional: run from cron / systemd every hour to auto-commit ucil-build/ state.
# Provides recovery if the host crashes mid-build. Not required, but cheap insurance.
#
# Example cron line:
#   0 * * * * cd /home/rishidarkdevil/Desktop/ucil && scripts/auto-commit-build.sh >> /tmp/ucil-auto-commit.log 2>&1
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

# shellcheck source=scripts/_retry.sh
source "$(dirname "$0")/_retry.sh"

# Only commit ucil-build/ artifacts, not source.
# Skip if the tree is clean for ucil-build/.
if git diff --quiet -- ucil-build/ && git diff --cached --quiet -- ucil-build/; then
  exit 0
fi

NOW=$(date -u +%Y-%m-%dT%H:%M:%SZ)
git add ucil-build/ || true

# Use UCIL_SEEDING=1 so feature-list.json edits (if any) pass — but the whitelist
# check still applies.
git commit -m "snapshot(build): autosave $NOW" --quiet || exit 0
safe_git_push --quiet || true
