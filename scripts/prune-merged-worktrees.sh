#!/usr/bin/env bash
#
# scripts/prune-merged-worktrees.sh
#
# Idempotently prune worktrees whose feat branches have been merged into main.
# Each merged worktree consumes ~20GB (cargo target/ + node_modules), so without
# pruning the disk fills at ~200GB/day at typical P3 throughput.
#
# Behavior:
# - For each worktree under ../ucil-wt/WO-NNNN with branch feat/WO-NNNN-<slug>:
#   - Skip if WO-NNNN is the currently-active worktree per progress.json
#   - Skip if the branch is NOT merged into main (still in flight)
#   - Skip if a `<WO-NNNN>-ready-for-review.md` exists without merge — RFR
#     pending verifier verdict
#   - Otherwise: remove worktree, delete local branch, delete remote branch
#
# Idempotent: re-running with no merged worktrees is a no-op.
# Safe: never touches the active worktree or in-flight branches.
#
# Usage: scripts/prune-merged-worktrees.sh [--verbose]
# Exits: 0 always (cleanup is best-effort; failures logged but non-fatal)

set -uo pipefail

VERBOSE=0
[[ "${1:-}" == "--verbose" ]] && VERBOSE=1

log() { echo "[prune-worktrees] $*"; }
vlog() { [[ "$VERBOSE" -eq 1 ]] && log "$*"; return 0; }

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  log "not in a git repo, exiting"
  exit 0
}
cd "$REPO_ROOT"

WT_BASE="$(realpath ../ucil-wt 2>/dev/null)"
[[ -z "$WT_BASE" || ! -d "$WT_BASE" ]] && {
  vlog "no ../ucil-wt directory, nothing to prune"
  exit 0
}

# Active worktree from progress.json (if present). May be null when the loop
# is between WOs.
ACTIVE_WT="$(jq -r '.active_worktree // empty' ucil-build/progress.json 2>/dev/null || true)"
ACTIVE_WO=""
if [[ -n "$ACTIVE_WT" && "$ACTIVE_WT" != "null" ]]; then
  ACTIVE_WO="$(basename "$ACTIVE_WT")"
  vlog "active worktree: $ACTIVE_WO (will skip)"
fi

# Make sure we have up-to-date main locally for is-merged checks.
git fetch origin main --quiet 2>/dev/null || true
MAIN_SHA="$(git rev-parse origin/main 2>/dev/null || git rev-parse main)"

PRUNED=0
SKIPPED=0

# Iterate over each WO-NNNN worktree.
for wt_dir in "$WT_BASE"/WO-*; do
  [[ -d "$wt_dir" ]] || continue
  wo="$(basename "$wt_dir")"

  # Skip active worktree.
  if [[ -n "$ACTIVE_WO" && "$wo" == "$ACTIVE_WO" ]]; then
    vlog "$wo: active, skip"
    SKIPPED=$((SKIPPED + 1))
    continue
  fi

  # Find the branch this worktree is checked out on.
  branch="$(git -C "$wt_dir" rev-parse --abbrev-ref HEAD 2>/dev/null || echo "")"
  if [[ -z "$branch" || "$branch" == "HEAD" ]]; then
    vlog "$wo: no branch (detached or broken), skip"
    SKIPPED=$((SKIPPED + 1))
    continue
  fi

  # Skip main worktree (defensive — base repo is not under ucil-wt/ but
  # paranoid is cheap).
  [[ "$branch" == "main" ]] && {
    vlog "$wo: branch=main, skip"
    SKIPPED=$((SKIPPED + 1))
    continue
  }

  # Is this branch's tip an ancestor of main? (i.e., merged?)
  branch_sha="$(git -C "$wt_dir" rev-parse HEAD 2>/dev/null || echo "")"
  if [[ -z "$branch_sha" ]]; then
    vlog "$wo: cannot resolve HEAD, skip"
    SKIPPED=$((SKIPPED + 1))
    continue
  fi

  if ! git merge-base --is-ancestor "$branch_sha" "$MAIN_SHA" 2>/dev/null; then
    vlog "$wo: branch=$branch tip $branch_sha NOT in main, skip (in flight)"
    SKIPPED=$((SKIPPED + 1))
    continue
  fi

  # Belt-and-suspenders: if RFR exists but no merge commit, skip (verifier
  # may still be running on this WO).
  rfr="ucil-build/work-orders/${wo}-ready-for-review.md"
  rfr_lower="$(echo "$wo" | tr '[:upper:]' '[:lower:]')"
  rfr_alt="ucil-build/work-orders/${rfr_lower#wo-}-ready-for-review.md"
  if [[ -f "$rfr" || -f "$rfr_alt" ]]; then
    # RFR present — check that a merge commit referencing this WO exists.
    if ! git log --oneline "$MAIN_SHA" 2>/dev/null | grep -q "merge: ${wo}\|merge:.*${wo,,}\|${branch}"; then
      vlog "$wo: RFR present but no merge commit for $branch, skip"
      SKIPPED=$((SKIPPED + 1))
      continue
    fi
  fi

  # All checks passed — prune.
  log "$wo: pruning (branch $branch fully merged into main)"

  # Remove worktree (force in case dirty leftovers).
  if git worktree remove --force "$wt_dir" 2>/dev/null; then
    vlog "$wo: worktree removed"
  else
    log "$wo: worktree-remove failed (non-fatal); attempting filesystem cleanup"
    rm -rf -- "$wt_dir" 2>/dev/null || true
    git worktree prune 2>/dev/null || true
  fi

  # Delete local branch.
  git branch -D "$branch" 2>/dev/null && vlog "$wo: local branch $branch deleted" || vlog "$wo: local branch $branch absent"

  # Delete remote branch (best-effort; offline-tolerant).
  git push origin --delete "$branch" 2>/dev/null && vlog "$wo: remote branch $branch deleted" || vlog "$wo: remote branch $branch already gone or push failed"

  PRUNED=$((PRUNED + 1))
done

# Final prune of stale worktree refs.
git worktree prune 2>/dev/null || true

log "done. pruned=$PRUNED skipped=$SKIPPED"
exit 0
