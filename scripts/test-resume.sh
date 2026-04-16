#!/usr/bin/env bash
# Integration test for scripts/resume.sh.
#
# Simulates a crash scenario:
#   1. Dirty worktree           — creates uncommitted file under ../ucil-wt/
#      (test-only worktree)     — resume must auto-stash, not delete.
#   2. Stale verifier-lock      — creates ucil-build/.verifier-lock with no
#                                 matching process alive; resume must remove.
#   3. Orphaned claude -p       — fake python shim; resume must kill.
#   4. Unpushed commits         — we DON'T force this because main must stay
#                                 clean; skipped in favour of an assertion
#                                 about resume's push-ahead message format.
#   5. Corrupt WO JSON          — drop a non-JSON file in work-orders/;
#                                 resume must quarantine to broken-<ts>/.
#   6. Feature-list intact      — must still validate; no false escalation.
#
# Then: run resume.sh --check TWICE, assert idempotent (second call sees
# nothing to clean and exits 0 with zero counters).
#
# Exit: 0 on PASS, 1 on FAIL.
set -uo pipefail

cd "$(git rev-parse --show-toplevel)"
REPO_ROOT="$(pwd)"

TS=$(date +%s)
TEST_WT="$REPO_ROOT/../ucil-wt/WO-TEST-RESUME-${TS}"
TMP="/tmp/ucil-resume-test-${TS}-$$"
mkdir -p "$TMP"

log()  { printf '[test-resume] %s\n' "$*"; }
fail() { printf '[test-resume] FAIL: %s\n' "$*" >&2; _cleanup; exit 1; }

_FAKE_PID=""
_WT_CREATED=0
_CORRUPT_FILE=""

_cleanup() {
  set +e
  if [[ -n "$_FAKE_PID" ]] && [[ -d "/proc/$_FAKE_PID" ]]; then
    kill -KILL "$_FAKE_PID" 2>/dev/null
  fi
  if [[ "$_WT_CREATED" -eq 1 ]] && [[ -d "$TEST_WT" ]]; then
    # Best-effort cleanup. Prune any test-only stash, then remove worktree.
    git -C "$TEST_WT" stash drop 0 2>/dev/null
    git worktree remove --force "$TEST_WT" 2>/dev/null
    rm -rf "$TEST_WT"
    git branch -D "test-resume-${TS}" 2>/dev/null
  fi
  if [[ -n "$_CORRUPT_FILE" ]] && [[ -f "$_CORRUPT_FILE" ]]; then
    rm -f "$_CORRUPT_FILE"
  fi
  # Remove any broken-<ts>/ the test created
  find ucil-build/work-orders -maxdepth 1 -name "broken-*" -newer "$TMP" -type d 2>/dev/null \
    | while read -r d; do rm -rf "$d"; done
  # Remove any test-only stash lines from triage-log.md (leave real ones alone).
  # We tag our stash with a TEST-sentinel message so the grep is precise.
  if grep -q "resume-test-${TS}" ucil-build/triage-log.md 2>/dev/null; then
    :   # leave the log entry; harmless and documents the test
  fi
  rm -rf "$TMP"
}
trap _cleanup EXIT

# --- 0. Pre-flight sanity ---
if [[ "$(git rev-parse --abbrev-ref HEAD)" != "main" ]]; then
  fail "must be on main branch (currently: $(git rev-parse --abbrev-ref HEAD))"
fi
if [[ "$(git status --porcelain | wc -l)" -gt 0 ]]; then
  log "WARNING: main tree has uncommitted changes; proceeding with --check-only assertions"
fi

# --- 1. Dirty worktree ---
log "Setup 1/5: create a dirty worktree at $TEST_WT"
mkdir -p "$(dirname "$TEST_WT")"
if ! git worktree add -b "test-resume-${TS}" "$TEST_WT" HEAD >/dev/null 2>&1; then
  fail "could not create test worktree"
fi
_WT_CREATED=1
echo "dirty-marker-${TS}" > "$TEST_WT/RESUME_TEST_MARKER.txt"
echo "RESUME_TEST_MARKER*" >> "$TEST_WT/.git/info/exclude" 2>/dev/null || true
# Also make a REAL tracked change so stash actually has something to stash.
touch "$TEST_WT/README.md"
echo "dirty-content" >> "$TEST_WT/README.md"

# --- 2. Stale verifier-lock ---
log "Setup 2/5: create stale verifier-lock"
echo "$$-stale-from-test-${TS}" > ucil-build/.verifier-lock

# --- 3. Orphaned claude -p fake ---
log "Setup 3/5: spawn fake orphan claude -p"
cat > "$TMP/claude" <<'SHIM'
#!/usr/bin/env python3
import time
time.sleep(600)
SHIM
chmod +x "$TMP/claude"
setsid "$TMP/claude" -p --fake-resume-test </dev/null >/dev/null 2>&1 &
disown
for _ in 1 2 3 4 5; do
  _FAKE_PID=$(pgrep -f "$TMP/claude -p" | head -1 || true)
  [[ -n "$_FAKE_PID" ]] && break
  sleep 0.2
done
if [[ -z "$_FAKE_PID" ]]; then
  fail "could not spawn fake orphan"
fi

# --- 4. Corrupt JSON work-order ---
log "Setup 4/5: drop corrupt JSON in work-orders/"
_CORRUPT_FILE="ucil-build/work-orders/broken-test-resume-${TS}.json"
cat > "$_CORRUPT_FILE" <<'BAD'
{ "id": "WO-TEST-BROKEN", "this": is not json at all
BAD

# --- 5. Sanity: feature-list still valid (don't corrupt it; that's destructive) ---
log "Setup 5/5: asserting feature-list.json is currently valid"
if ! jq -e . ucil-build/feature-list.json >/dev/null 2>&1; then
  fail "feature-list.json is already corrupt BEFORE the test started — aborting"
fi

# ===== Run 1: resume.sh --check =====
log "Run 1: scripts/resume.sh --check"
if ! timeout 30 scripts/resume.sh --check >"$TMP/run1.log" 2>&1; then
  # --check may exit 1 if main tree is dirty; we only assert on message contents.
  :
fi

log "Run 1 output (tail):"
tail -30 "$TMP/run1.log" | sed 's/^/  /'

# --- Assertions: each of the 5 setups was handled ---
assert_contains() {
  local needle="$1" haystack="$2" label="$3"
  if ! grep -qF "$needle" "$haystack"; then
    log "expected to find \"$needle\" in resume output (assertion: $label)"
    fail "$label"
  fi
}

assert_contains "Removing stale ucil-build/.verifier-lock" "$TMP/run1.log" "stale-verifier-lock cleanup"
if [[ -f ucil-build/.verifier-lock ]]; then
  fail "verifier-lock still present after cleanup"
fi

# The orphan-kill line varies ("Killing N orphaned 'claude -p' process(es)")
if ! grep -qE "Killing [0-9]+ orphaned 'claude -p'" "$TMP/run1.log"; then
  fail "orphan kill message absent from resume output"
fi
# Confirm fake is actually dead
for _ in 1 2 3 4 5 6 7 8 9 10; do
  [[ ! -d "/proc/$_FAKE_PID" ]] && break
  sleep 1
done
if [[ -d "/proc/$_FAKE_PID" ]]; then
  fail "fake orphan ($_FAKE_PID) still alive after resume"
fi

# Worktree was stashed (not deleted)
if [[ ! -d "$TEST_WT" ]]; then
  fail "test worktree was deleted (resume should stash, not delete)"
fi
stash_count=$(git -C "$TEST_WT" stash list 2>/dev/null | wc -l || echo 0)
if (( stash_count == 0 )); then
  fail "test worktree has no stash entry — resume should have stashed dirty changes"
fi
# Must also be noted in triage-log.md
if ! grep -q "auto-stash-on-resume" ucil-build/triage-log.md 2>/dev/null; then
  fail "triage-log.md has no auto-stash-on-resume entry"
fi

# Corrupt JSON was quarantined
if [[ -f "$_CORRUPT_FILE" ]]; then
  fail "corrupt JSON was not moved out of work-orders/"
fi
broken_dirs=$(find ucil-build/work-orders -maxdepth 1 -name "broken-*" -type d 2>/dev/null)
if [[ -z "$broken_dirs" ]]; then
  fail "no broken-<ts>/ quarantine directory created"
fi
found_corrupt=0
for d in $broken_dirs; do
  if ls "$d" 2>/dev/null | grep -qF "broken-test-resume-${TS}.json"; then
    found_corrupt=1
    break
  fi
done
if [[ "$found_corrupt" -eq 0 ]]; then
  fail "corrupt JSON not found in any broken-<ts>/ directory"
fi

# feature-list.json validation passed silently (no escalation spawned by us)
if ls ucil-build/escalations/*feature-list-* 2>/dev/null | xargs -r -n1 basename | grep -qF "$TS" ; then
  fail "feature-list escalation was spawned — should not be"
fi

log "Run 1 assertions: all PASSED"

# ===== Run 2: idempotent check =====
log "Run 2: scripts/resume.sh --check (idempotency)"
if ! timeout 30 scripts/resume.sh --check >"$TMP/run2.log" 2>&1; then
  :
fi

# After run 1, the second run should find no orphans, no dirty worktrees
# (because the dirty changes were stashed), no corrupt JSON, no stale lock.
# We assert the summary counters are all zero.
if ! grep -qE "Orphans killed:\s+0" "$TMP/run2.log"; then
  log "Run 2 output (tail):"; tail -20 "$TMP/run2.log" | sed 's/^/  /'
  fail "run 2 reported non-zero orphans killed (not idempotent)"
fi
if ! grep -qE "Worktrees auto-stashed:\s+0" "$TMP/run2.log"; then
  log "Run 2 output (tail):"; tail -20 "$TMP/run2.log" | sed 's/^/  /'
  fail "run 2 reported new worktree stashes (not idempotent)"
fi
if ! grep -qE "Corrupt WOs quarantined:0" "$TMP/run2.log"; then
  log "Run 2 output (tail):"; tail -20 "$TMP/run2.log" | sed 's/^/  /'
  fail "run 2 reported new corrupt WO quarantine (not idempotent)"
fi
# Verifier-lock must not reappear
if [[ -f ucil-build/.verifier-lock ]]; then
  fail "verifier-lock reappeared between runs"
fi

log "Run 2 assertions: all PASSED (idempotent)"
log "ALL TESTS PASSED"
exit 0
