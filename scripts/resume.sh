#!/usr/bin/env bash
# Resume the UCIL autonomous build after a crash, PC shutdown, or manual
# pause. Cleans stale in-flight state, then (optionally) kicks off
# scripts/run-phase.sh from the current phase.
#
# Safe to run any time the loop is idle. Idempotent.
#
# Usage:
#   scripts/resume.sh            # interactive: prompts before starting the loop
#   scripts/resume.sh --yes      # clean up and auto-start the loop
#   scripts/resume.sh --check    # clean up only; don't start the loop
set -uo pipefail

cd "$(git rev-parse --show-toplevel)"

AUTO_START=0
CHECK_ONLY=0
for arg in "$@"; do
  case "$arg" in
    --yes|-y) AUTO_START=1 ;;
    --check|-c) CHECK_ONLY=1 ;;
    -*) echo "unknown flag: $arg" >&2; exit 2 ;;
  esac
done

PHASE=$(jq -r '.phase // 0' ucil-build/progress.json 2>/dev/null || echo 0)

log() { printf '\n[resume] %s\n' "$*"; }

# --- 1. Abort any in-progress merge in main ---
if [[ -f .git/MERGE_HEAD ]] || [[ -f .git/REBASE_HEAD ]]; then
  log "Aborting in-progress merge/rebase in main"
  git merge --abort 2>/dev/null || true
  git rebase --abort 2>/dev/null || true
fi

# --- 2. Same for every worktree under ../ucil-wt/ ---
for wt in ../ucil-wt/WO-*/ ../ucil-wt/WO-*; do
  [[ ! -d "$wt" ]] && continue
  wt_git=$(git -C "$wt" rev-parse --git-dir 2>/dev/null || echo "")
  [[ -z "$wt_git" ]] && continue
  if [[ -f "$wt_git/MERGE_HEAD" ]] || [[ -f "$wt_git/REBASE_HEAD" ]]; then
    log "Aborting in-progress merge/rebase in $wt"
    git -C "$wt" merge --abort 2>/dev/null || true
    git -C "$wt" rebase --abort 2>/dev/null || true
  fi
done

# --- 3. Stale verifier-lock cleanup ---
if [[ -f ucil-build/.verifier-lock ]]; then
  # Only remove if no verifier claude -p process is actually alive
  alive=0
  for p in $(pgrep -f 'claude -p' 2>/dev/null); do
    if grep -q 'CLAUDE_SUBAGENT_NAME=verifier' "/proc/$p/environ" 2>/dev/null; then
      alive=1
      break
    fi
  done
  if [[ "$alive" -eq 0 ]]; then
    log "Removing stale ucil-build/.verifier-lock (no verifier process alive)"
    rm -f ucil-build/.verifier-lock
  else
    log "verifier process is alive — leaving ucil-build/.verifier-lock in place"
  fi
fi

# --- 4. Reset triage-pass counters (give triage a fresh 3 passes) ---
if ls .ucil-triage-pass.phase-* >/dev/null 2>&1; then
  log "Resetting triage-pass counters"
  rm -f .ucil-triage-pass.phase-*
fi

# --- 5. Uncommitted changes in worktrees (warn only; don't auto-commit) ---
dirty_wts=()
for wt in ../ucil-wt/WO-*/ ../ucil-wt/WO-*; do
  [[ ! -d "$wt" ]] && continue
  n=$(git -C "$wt" status --porcelain 2>/dev/null | wc -l || echo 0)
  if [[ "$n" -gt 0 ]]; then
    dirty_wts+=("$wt ($n files)")
  fi
done
if [[ "${#dirty_wts[@]}" -gt 0 ]]; then
  log "Worktrees have uncommitted changes — review before resuming:"
  for w in "${dirty_wts[@]}"; do echo "  $w"; done
  echo ""
  echo "  To preserve: git -C <wt> commit -am 'wip: resume checkpoint' && git -C <wt> push"
  echo "  To discard:  git -C <wt> reset --hard && git -C <wt> clean -fd"
  echo ""
fi

# --- 6. Main tree dirty check (should almost always be clean) ---
if [[ "$(git status --porcelain | wc -l)" -gt 0 ]]; then
  log "Main tree has uncommitted changes:"
  git status --short
  echo ""
  echo "  Resolve before resuming (commit or reset)."
  if [[ "$AUTO_START" -eq 1 ]]; then
    echo "  Refusing --yes with dirty main tree."
    exit 1
  fi
fi

# --- 7. Push any unpushed commits on any tracked branch ---
for ref in $(git for-each-ref --format='%(refname:short)' refs/heads/ 2>/dev/null); do
  if git rev-parse --abbrev-ref "${ref}@{u}" >/dev/null 2>&1; then
    ahead=$(git rev-list "${ref}@{u}..${ref}" 2>/dev/null | wc -l)
    if [[ "$ahead" -gt 0 ]]; then
      log "Pushing $ref ($ahead commits ahead of upstream)"
      git push origin "$ref" 2>&1 | tail -2 || true
    fi
  fi
done

# --- 8. Pull main so we have everything other agents may have pushed ---
if [[ "$(git rev-parse --abbrev-ref HEAD)" == "main" ]]; then
  git pull --ff-only 2>&1 | tail -2 || true
else
  git checkout main 2>&1 | tail -1 || true
  git pull --ff-only 2>&1 | tail -2 || true
fi

# --- 9. Summary ---
UNRESOLVED=0
shopt -s nullglob
for f in ucil-build/escalations/*.md; do
  grep -qE '^resolved:[[:space:]]*true[[:space:]]*$' "$f" || UNRESOLVED=$((UNRESOLVED+1))
done
shopt -u nullglob

echo ""
echo "=== Resume summary ==="
echo "Phase:                  $PHASE"
echo "Features passing:       $(jq '[.features[] | select(.passes==true)] | length' ucil-build/feature-list.json 2>/dev/null || echo ?) / $(jq '.features | length' ucil-build/feature-list.json 2>/dev/null || echo ?)"
echo "Work-orders on disk:    $(ls ucil-build/work-orders/*.json 2>/dev/null | wc -l)"
echo "Unresolved escalations: $UNRESOLVED"
echo "Open rejections:        $(ls ucil-build/rejections/*.md 2>/dev/null | wc -l)"
echo "Main HEAD:              $(git rev-parse --short HEAD)"
echo ""

if [[ "$CHECK_ONLY" -eq 1 ]]; then
  echo "Cleanup done. --check only; not starting the loop."
  exit 0
fi

if [[ "$AUTO_START" -eq 1 ]]; then
  echo "Starting ./scripts/run-phase.sh $PHASE"
  exec scripts/run-phase.sh "$PHASE"
fi

read -r -p "Resume autonomous loop from phase $PHASE? [y/N] " ANS
case "$ANS" in
  y|Y|yes|Yes) exec scripts/run-phase.sh "$PHASE" ;;
  *) echo "Cleanup done. Run ./scripts/run-phase.sh $PHASE when ready." ;;
esac
