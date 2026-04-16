#!/usr/bin/env bash
# Smoke test for resume.sh's orphan-claude kill logic.
#
# Strategy: we can't spawn a REAL `claude -p` (expensive + needs API key), so
# we spawn a fake shim process named `claude` (via a python3 script written
# to /tmp/<name>/claude) with `-p` as its own argv[1]. /proc/<pid>/cmdline
# will then contain:
#   python3\0/tmp/…/claude\0-p\0--fake-orphan-test\0
# which is:
#   - visible to `pgrep -f 'claude -p'`
#   - matched by resume.sh's "/claude" regex on the second argv slot
#   - has `-p` as a standalone argv entry (what resume.sh checks for)
#
# The fake process is detached via `setsid` + `disown`, so its parent chain
# does NOT include any `scripts/run-*.sh` — it's a true orphan from the
# resume.sh detector's perspective.
#
# Exit code: 0 on pass, 1 on fail.
set -uo pipefail

cd "$(git rev-parse --show-toplevel)"

TS=$(date +%s)
TMP="/tmp/ucil-orphan-test-${TS}-$$"
mkdir -p "$TMP"
# Write the shim
cat > "$TMP/claude" <<'SHIM'
#!/usr/bin/env python3
# Fake headless claude for test purposes. Sleeps up to 10 minutes then exits.
# The shim name `claude` and the -p argv[1] are the load-bearing pieces:
# resume.sh uses them to identify a headless orphan.
import time, sys
# argv: ['/tmp/…/claude', '-p', '--fake-orphan-test', …]
time.sleep(600)
SHIM
chmod +x "$TMP/claude"

FAKE_PID=""
cleanup() {
  if [[ -n "$FAKE_PID" ]] && [[ -d "/proc/$FAKE_PID" ]]; then
    kill -KILL "$FAKE_PID" 2>/dev/null || true
  fi
  rm -rf "$TMP"
}
trap cleanup EXIT

log() { printf '[test-orphan] %s\n' "$*"; }
fail() { printf '[test-orphan] FAIL: %s\n' "$*" >&2; cleanup; exit 1; }

# Launch detached fake orphan
setsid "$TMP/claude" -p --fake-orphan-test </dev/null >/dev/null 2>&1 &
disown
# Give the kernel a beat and locate the actual child pid
for _ in 1 2 3 4 5 6 7 8 9 10; do
  FAKE_PID=$(pgrep -f "$TMP/claude -p" | head -1 || true)
  [[ -n "$FAKE_PID" ]] && [[ -d "/proc/$FAKE_PID" ]] && break
  sleep 0.1
done
if [[ -z "$FAKE_PID" ]] || [[ ! -d "/proc/$FAKE_PID" ]]; then
  fail "could not launch fake orphan"
fi
log "fake orphan launched as pid $FAKE_PID"

# Sanity-check: resume's detector will match this
if ! pgrep -f 'claude -p' | grep -qx "$FAKE_PID"; then
  fail "pgrep -f 'claude -p' did not match pid $FAKE_PID"
fi
if ! tr '\0' '\n' < "/proc/$FAKE_PID/cmdline" 2>/dev/null | grep -qx -- '-p'; then
  log "orphan cmdline (NUL-split):"
  tr '\0' '\n' < "/proc/$FAKE_PID/cmdline" | sed 's/^/  /'
  fail "no standalone -p argv entry"
fi
if ! tr '\0' ' ' < "/proc/$FAKE_PID/cmdline" 2>/dev/null | grep -qE '(^| )claude( |$)|/claude($| )'; then
  fail "cmdline does not match resume.sh's claude regex"
fi
# Ancestor chain sanity: no scripts/run-*.sh should be an ancestor
cur="$FAKE_PID"
matched_launcher=0
for _ in 1 2 3 4 5 6 7 8; do
  ppid=$(awk '/^PPid:/ {print $2}' "/proc/$cur/status" 2>/dev/null || echo "")
  [[ -z "$ppid" || "$ppid" == "0" || "$ppid" == "1" ]] && break
  if [[ -r "/proc/$ppid/cmdline" ]]; then
    if tr '\0' ' ' < "/proc/$ppid/cmdline" 2>/dev/null \
         | grep -qE 'scripts/(run-phase|run-all|run-executor|run-planner|run-critic|run-triage|run-root-cause-finder|spawn-verifier|run-effectiveness-evaluator)\.sh'; then
      matched_launcher=1
      break
    fi
  fi
  cur="$ppid"
done
if [[ "$matched_launcher" -eq 1 ]]; then
  fail "test setup leaked an ancestor matching a run-*.sh launcher"
fi

# Run resume.sh --check (invokes orphan cleanup path)
log "invoking scripts/resume.sh --check ..."
if ! timeout 30 scripts/resume.sh --check >"$TMP/resume.log" 2>&1; then
  # resume may return non-zero on dirty main tree; we only care that the orphan is gone.
  :
fi
grep -E 'orphan|Orphan' "$TMP/resume.log" | head -5 | sed 's/^/  resume> /' || true

# Assert the orphan is dead (SIGTERM → SIGKILL within 15s).
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15; do
  if [[ ! -d "/proc/$FAKE_PID" ]]; then
    log "PASS: fake orphan ($FAKE_PID) was killed by resume.sh"
    exit 0
  fi
  sleep 1
done

log "resume stdout/stderr:"
sed 's/^/    /' < "$TMP/resume.log" | head -40
fail "fake orphan ($FAKE_PID) still alive after cleanup window"
