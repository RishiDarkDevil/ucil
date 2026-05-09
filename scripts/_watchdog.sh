#!/usr/bin/env bash
# UCIL watchdog — monitor the autonomous loop, auto-restart if it dies.
# Runs detached (e.g. via nohup or a systemd --user service).
#
# Policy:
# - Poll every 60s.
# - If no run-all.sh / run-phase.sh is alive AND no `claude -p` is alive
#   AND the current phase has not been shipped (no git tag `phase-<N>-complete`),
#   wait 5 minutes to rule out a pause-during-transition, then check again.
# - If still dead, log + invoke `scripts/resume.sh --yes`.
# - 3 consecutive restart failures inside a 1-hour window → escalate + exit.
# - SIGTERM / SIGINT → graceful shutdown (no kill loops).
#
# Usage:
#   nohup scripts/_watchdog.sh >/dev/null 2>&1 &
#   (or install via scripts/install-watchdog.sh)
set -uo pipefail

cd "$(git rev-parse --show-toplevel 2>/dev/null || echo /home/rishidarkdevil/Desktop/ucil)"

PIDFILE="ucil-build/.watchdog.pid"
LOGDIR="ucil-build/telemetry"
LOG="$LOGDIR/watchdog.log"
POLL_INTERVAL="${UCIL_WATCHDOG_POLL_S:-60}"
QUIESCE_WAIT="${UCIL_WATCHDOG_QUIESCE_S:-300}"   # 5 min
RESTART_WINDOW_S="${UCIL_WATCHDOG_WINDOW_S:-3600}"
MAX_RESTARTS="${UCIL_WATCHDOG_MAX_RESTARTS:-3}"

mkdir -p "$LOGDIR"

# Refuse to start if another watchdog is already alive.
if [[ -f "$PIDFILE" ]]; then
  existing=$(cat "$PIDFILE" 2>/dev/null || echo "")
  if [[ -n "$existing" ]] && [[ -d "/proc/$existing" ]]; then
    # Confirm it's actually a watchdog (not a reused pid)
    if tr '\0' ' ' < "/proc/$existing/cmdline" 2>/dev/null | grep -q '_watchdog.sh'; then
      echo "[watchdog] already running as pid $existing; exiting." >&2
      exit 0
    fi
  fi
  rm -f "$PIDFILE"
fi
echo $$ > "$PIDFILE"

STOP=0
on_signal() {
  log "received signal; shutting down gracefully"
  STOP=1
}
trap on_signal TERM INT HUP
trap 'rm -f "$PIDFILE"' EXIT

log() {
  printf '%s [watchdog] %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*" >> "$LOG"
}

# Returns 0 if the autonomous loop looks alive, 1 if it looks dead.
loop_alive() {
  # Any run-all.sh or run-phase.sh in the process table?
  if pgrep -f 'scripts/run-(all|phase)\.sh' >/dev/null 2>&1; then
    return 0
  fi
  # Any headless claude -p process?
  for pid in $(pgrep -f 'claude -p' 2>/dev/null); do
    if [[ -r "/proc/$pid/cmdline" ]]; then
      if tr '\0' '\n' < "/proc/$pid/cmdline" 2>/dev/null | grep -qx -- '-p'; then
        return 0
      fi
    fi
  done
  return 1
}

# Returns 0 if the current phase has shipped (tag exists), 1 otherwise.
phase_shipped() {
  local phase
  phase=$(jq -r '.phase // empty' ucil-build/progress.json 2>/dev/null || echo "")
  [[ -z "$phase" ]] && return 1
  git tag --list "phase-${phase}-complete" | grep -q . && return 0
  return 1
}

# Sleep that respects STOP quickly
nap() {
  local secs="$1"
  while (( secs > 0 )); do
    (( STOP == 1 )) && return
    sleep 1
    secs=$(( secs - 1 ))
  done
}

# Rolling restart window (array of unix timestamps)
RESTART_TIMES=()

attempt_restart() {
  local now
  now=$(date +%s)
  # Drop timestamps outside the 1-hour window
  local pruned=()
  local t
  for t in "${RESTART_TIMES[@]}"; do
    if (( now - t < RESTART_WINDOW_S )); then
      pruned+=("$t")
    fi
  done
  RESTART_TIMES=("${pruned[@]}")

  if (( ${#RESTART_TIMES[@]} >= MAX_RESTARTS )); then
    log "MAX_RESTARTS (${MAX_RESTARTS}) hit within ${RESTART_WINDOW_S}s — escalating and exiting"
    local ts esc
    ts=$(date -u +%Y%m%dT%H%M%SZ)
    mkdir -p ucil-build/escalations
    esc="ucil-build/escalations/${ts}-watchdog-flapping.md"
    cat > "$esc" <<EOF
---
timestamp: $(date -u +%Y-%m-%dT%H:%M:%SZ)
type: watchdog-flapping
severity: high
blocks_loop: true
requires_planner_action: true
---

# UCIL watchdog restart loop detected

The autonomous loop died and was restarted ${MAX_RESTARTS} times inside
${RESTART_WINDOW_S}s. Probable cause: a consistent crash (not a transient
kill). Watchdog has exited; fix the root cause and re-invoke via
\`scripts/install-watchdog.sh\` or \`scripts/_watchdog.sh &\` once the
loop runs clean for >1h.

Tail of \`$LOG\`:
\`\`\`
$(tail -30 "$LOG" 2>/dev/null || echo '(log missing)')
\`\`\`
EOF
    git add "$esc" 2>/dev/null || true
    git commit -m "chore(escalation): watchdog flapping — $MAX_RESTARTS restarts/${RESTART_WINDOW_S}s" 2>/dev/null || true
    git push --quiet 2>/dev/null || true
    return 2   # caller should exit
  fi

  log "invoking scripts/resume.sh --yes"
  RESTART_TIMES+=("$now")
  # --yes requires a clean main tree; if it refuses, surface in the log.
  # Run via setsid so the child survives if we get killed.
  setsid nohup bash scripts/resume.sh --yes >> "$LOG" 2>&1 &
  local rpid=$!
  log "spawned resume.sh (pid $rpid)"
  return 0
}

log "watchdog starting; poll=${POLL_INTERVAL}s quiesce=${QUIESCE_WAIT}s max_restarts=${MAX_RESTARTS}/${RESTART_WINDOW_S}s"

while (( STOP == 0 )); do
  if loop_alive; then
    nap "$POLL_INTERVAL"
    continue
  fi
  if phase_shipped; then
    # Loop exited after shipping a phase → legitimate quiescence.
    # Wait one poll cycle and re-check (maybe user will advance phase).
    nap "$POLL_INTERVAL"
    continue
  fi

  log "loop appears dead; entering ${QUIESCE_WAIT}s quiesce before restart"
  nap "$QUIESCE_WAIT"
  (( STOP == 1 )) && break

  # Disk-watermark prune: before restarting the loop, opportunistically
  # reclaim merged-worktree disk so a 95%-full disk doesn't crash claude.
  _USE_PCT=$(df / 2>/dev/null | awk 'NR==2 {gsub("%",""); print $5}' || echo 0)
  if [[ "$_USE_PCT" -ge 70 ]] && [[ -x "$REPO_ROOT/scripts/prune-merged-worktrees.sh" ]]; then
    log "disk at ${_USE_PCT}% — running prune-merged-worktrees pre-restart"
    "$REPO_ROOT/scripts/prune-merged-worktrees.sh" >>/tmp/ucil-watchdog-prune.log 2>&1 || true
  fi

  # Re-check after quiescence
  if loop_alive; then
    log "loop came back on its own; no restart needed"
    continue
  fi
  if phase_shipped; then
    log "phase shipped during quiesce; no restart needed"
    continue
  fi

  if ! attempt_restart; then
    rc=$?
    if (( rc == 2 )); then
      log "exiting due to restart cap"
      break
    fi
  fi
  # After a restart, give the loop some time to actually start claude.
  nap "$POLL_INTERVAL"
done

log "watchdog exiting"
