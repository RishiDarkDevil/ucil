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

# shellcheck source=scripts/_retry.sh
source "$(dirname "$0")/_retry.sh"

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
TS="$(date -u +%Y%m%dT%H%M%SZ)"

log() { printf '\n[resume] %s\n' "$*"; }
warn() { printf '\n[resume][WARN] %s\n' "$*" >&2; }
loud() { printf '\n[resume][!!!] %s\n' "$*" >&2; }

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

# --- 4b. Orphaned claude -p processes ---
# Kill headless (`claude -p`) processes not parented to a live run-phase.sh /
# run-all.sh / run-*.sh launcher. Never touch interactive (non -p) claude.
orphan_pids=()
for pid in $(pgrep -f 'claude -p' 2>/dev/null); do
  [[ ! -d "/proc/$pid" ]] && continue
  # Double-check this is headless (has ` -p ` in cmdline, not just a path with
  # "-p" in it). The cmdline has NUL separators; awk on the args.
  if ! tr '\0' ' ' < "/proc/$pid/cmdline" 2>/dev/null | grep -qE '(^| )claude( |$)|/claude($| )'; then
    continue
  fi
  if ! tr '\0' '\n' < "/proc/$pid/cmdline" 2>/dev/null | grep -qx -- '-p'; then
    # not a `claude -p …` invocation; leave interactive sessions alone
    continue
  fi
  # Walk up ppid chain; if any ancestor is a running run-*.sh, this is NOT an orphan.
  parented=0
  cur="$pid"
  for _depth in 1 2 3 4 5 6 7 8; do
    ppid=$(awk '/^PPid:/ {print $2}' "/proc/$cur/status" 2>/dev/null)
    [[ -z "$ppid" || "$ppid" == "0" || "$ppid" == "1" ]] && break
    if [[ -r "/proc/$ppid/cmdline" ]]; then
      if tr '\0' ' ' < "/proc/$ppid/cmdline" 2>/dev/null \
           | grep -qE 'scripts/(run-phase|run-all|run-executor|run-planner|run-critic|run-triage|run-root-cause-finder|spawn-verifier|run-effectiveness-evaluator)\.sh'; then
        parented=1
        break
      fi
    fi
    cur="$ppid"
  done
  [[ "$parented" -eq 1 ]] && continue
  orphan_pids+=("$pid")
done

if [[ "${#orphan_pids[@]}" -gt 0 ]]; then
  log "Killing ${#orphan_pids[@]} orphaned 'claude -p' process(es): ${orphan_pids[*]}"
  for pid in "${orphan_pids[@]}"; do
    kill -TERM "$pid" 2>/dev/null || true
  done
  # Give them 10s to exit gracefully
  for _ in $(seq 1 10); do
    still=0
    for pid in "${orphan_pids[@]}"; do
      [[ -d "/proc/$pid" ]] && still=1
    done
    [[ "$still" -eq 0 ]] && break
    sleep 1
  done
  # Escalate to SIGKILL for stragglers
  for pid in "${orphan_pids[@]}"; do
    if [[ -d "/proc/$pid" ]]; then
      warn "pid $pid did not exit on SIGTERM; sending SIGKILL"
      kill -KILL "$pid" 2>/dev/null || true
    fi
  done
fi

# --- 5. Worktree uncommitted changes — stash them (don't delete) ---
dirty_wts=()
stashed_wts=()
for wt in ../ucil-wt/WO-*/ ../ucil-wt/WO-*; do
  [[ ! -d "$wt" ]] && continue
  n=$(git -C "$wt" status --porcelain 2>/dev/null | wc -l || echo 0)
  if [[ "$n" -gt 0 ]]; then
    dirty_wts+=("$wt ($n files)")
    stash_msg="auto-stash-on-resume-${TS}"
    if git -C "$wt" stash push -u -m "$stash_msg" >/dev/null 2>&1; then
      stashed_wts+=("$wt :: $stash_msg")
    else
      warn "could not stash $wt — inspect manually"
    fi
  fi
done
if [[ "${#stashed_wts[@]}" -gt 0 ]]; then
  mkdir -p ucil-build
  {
    echo ""
    echo "## $(date -u +%Y-%m-%dT%H:%M:%SZ) resume auto-stash"
    echo ""
    for w in "${stashed_wts[@]}"; do
      echo "- $w"
    done
    echo ""
    echo "Inspect with: \`git -C <wt> stash list\` — pop or drop per executor's judgement."
  } >> ucil-build/triage-log.md
  log "Stashed ${#stashed_wts[@]} dirty worktree(s); see ucil-build/triage-log.md"
fi
if [[ "${#dirty_wts[@]}" -gt 0 ]]; then
  log "Worktrees had uncommitted changes (now stashed):"
  for w in "${dirty_wts[@]}"; do echo "  $w"; done
fi

# --- 5b. Corrupt JSON in work-orders/ — quarantine, don't delete ---
corrupt=()
shopt -s nullglob
for f in ucil-build/work-orders/*.json; do
  if ! jq -e . "$f" >/dev/null 2>&1; then
    corrupt+=("$f")
  fi
done
shopt -u nullglob
if [[ "${#corrupt[@]}" -gt 0 ]]; then
  qdir="ucil-build/work-orders/broken-${TS}"
  mkdir -p "$qdir"
  for f in "${corrupt[@]}"; do
    warn "corrupt JSON: $f — moving to $qdir/"
    mv "$f" "$qdir/" 2>/dev/null || true
  done
  mkdir -p ucil-build
  {
    echo ""
    echo "## $(date -u +%Y-%m-%dT%H:%M:%SZ) resume corrupt-JSON quarantine"
    echo ""
    echo "Moved ${#corrupt[@]} work-order(s) to \`$qdir/\`:"
    for f in "${corrupt[@]}"; do echo "- $(basename "$f")"; done
  } >> ucil-build/triage-log.md
fi

# --- 5c. Feature-list.json integrity — LOUD escalation if broken ---
FL="ucil-build/feature-list.json"
FL_SCHEMA="ucil-build/schema/feature-list.schema.json"
if [[ -f "$FL" ]]; then
  if ! jq -e . "$FL" >/dev/null 2>&1; then
    loud "feature-list.json is NOT valid JSON — this is load-bearing."
    mkdir -p ucil-build/escalations
    ESC="ucil-build/escalations/${TS}-feature-list-corrupt.md"
    cat > "$ESC" <<EOF
---
timestamp: $(date -u +%Y-%m-%dT%H:%M:%SZ)
type: feature-list-corrupt
severity: critical
blocks_loop: true
requires_planner_action: true
---

# feature-list.json failed JSON parse on resume

\`jq . ucil-build/feature-list.json\` returned non-zero. Autonomous loop
CANNOT continue; the feature oracle is load-bearing for every verifier
run. Restore from a prior commit (\`git show HEAD:ucil-build/feature-list.json > /tmp/fl.json\`)
and manually re-apply any legitimate flips.

jq error output:
\`\`\`
$(jq -e . "$FL" 2>&1 | head -20)
\`\`\`
EOF
    git add "$ESC" 2>/dev/null || true
    git commit -m "chore(escalation): feature-list.json corrupt on resume" 2>/dev/null || true
    safe_git_push --quiet || true
    loud "Halting resume: see $ESC"
    exit 1
  fi
  # Schema validation — python jsonschema is available on this host; fall back to jq-only if not.
  if command -v python3 >/dev/null 2>&1 && python3 -c 'import jsonschema' 2>/dev/null; then
    if ! python3 - "$FL" "$FL_SCHEMA" <<'PY' 2>/tmp/ucil-resume-schema.err
import json, sys
from jsonschema import Draft202012Validator
fl, schema = sys.argv[1], sys.argv[2]
with open(fl) as f: data = json.load(f)
with open(schema) as f: sch = json.load(f)
errs = sorted(Draft202012Validator(sch).iter_errors(data), key=lambda e: e.path)
if errs:
    for e in errs[:5]:
        print(f"{list(e.path)}: {e.message}", file=sys.stderr)
    sys.exit(1)
PY
    then
      loud "feature-list.json schema validation FAILED — load-bearing corruption."
      mkdir -p ucil-build/escalations
      ESC="ucil-build/escalations/${TS}-feature-list-schema-invalid.md"
      cat > "$ESC" <<EOF
---
timestamp: $(date -u +%Y-%m-%dT%H:%M:%SZ)
type: feature-list-schema-invalid
severity: critical
blocks_loop: true
requires_planner_action: true
---

# feature-list.json failed schema validation on resume

Schema: \`ucil-build/schema/feature-list.schema.json\`

First errors:
\`\`\`
$(cat /tmp/ucil-resume-schema.err 2>/dev/null | head -20)
\`\`\`
EOF
      git add "$ESC" 2>/dev/null || true
      git commit -m "chore(escalation): feature-list.json schema-invalid on resume" 2>/dev/null || true
      safe_git_push --quiet || true
      loud "Halting resume: see $ESC"
      exit 1
    fi
  fi
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
      safe_git_push origin "$ref" 2>&1 | tail -2 || true
    fi
  fi
done

# --- 8. Pull main so we have everything other agents may have pushed ---
if [[ "$(git rev-parse --abbrev-ref HEAD)" == "main" ]]; then
  retry_git 3 2 pull --ff-only 2>&1 | tail -2 || true
else
  git checkout main 2>&1 | tail -1 || true
  retry_git 3 2 pull --ff-only 2>&1 | tail -2 || true
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
echo "Orphans killed:         ${#orphan_pids[@]}"
echo "Worktrees auto-stashed: ${#stashed_wts[@]}"
echo "Corrupt WOs quarantined:${#corrupt[@]}"
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
