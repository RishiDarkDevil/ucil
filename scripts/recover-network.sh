#!/usr/bin/env bash
# scripts/recover-network.sh
#
# Autonomous network recovery: if github.com unreachable, toggle wifi
# (SSID: RishiDarkDevil). Idempotent; safe to invoke when network is fine.
#
# Returns 0 if github reachable (either already, or after toggle).
# Returns 1 if github still unreachable after recovery attempt.
#
# Usage:
#   scripts/recover-network.sh                 # check + recover if needed
#   scripts/recover-network.sh --check-only    # exit code only, no recovery
#   scripts/recover-network.sh --force-toggle  # always toggle, even if up
#
# Hard limit: at most 2 toggle attempts per invocation.
set -uo pipefail

CHECK_ONLY=0
FORCE_TOGGLE=0
for arg in "$@"; do
  case "$arg" in
    --check-only)   CHECK_ONLY=1 ;;
    --force-toggle) FORCE_TOGGLE=1 ;;
  esac
done

log() { echo "[recover-network] $*"; }

is_reachable() {
  local code
  code=$(curl -s -o /dev/null -w "%{http_code}" --max-time 5 https://github.com 2>/dev/null || echo 000)
  [[ "$code" == "200" ]]
}

if (( FORCE_TOGGLE == 0 )) && is_reachable; then
  log "github reachable, no recovery needed"
  exit 0
fi

if (( CHECK_ONLY == 1 )); then
  log "github unreachable (check-only mode, no toggle)"
  exit 1
fi

if ! command -v nmcli >/dev/null 2>&1; then
  log "nmcli not available; cannot toggle wifi"
  exit 1
fi

WIFI_SSID="${UCIL_WIFI_SSID:-RishiDarkDevil}"

for attempt in 1 2; do
  log "toggle attempt $attempt: nmcli down/up '$WIFI_SSID'"
  nmcli connection down "$WIFI_SSID" 2>&1 | head -3
  sleep 3
  nmcli connection up "$WIFI_SSID" 2>&1 | head -3 || {
    log "connection-name form failed; falling back to radio toggle"
    nmcli radio wifi off 2>&1 | head -1
    sleep 3
    nmcli radio wifi on 2>&1 | head -1
  }

  log "waiting up to 60s for github reachability..."
  for i in $(seq 1 30); do
    if is_reachable; then
      log "github reachable after $((i*2))s; recovery successful (attempt $attempt)"
      exit 0
    fi
    sleep 2
  done

  log "still unreachable after attempt $attempt"
done

log "exhausted 2 toggle attempts; github still unreachable — likely upstream"
exit 1
