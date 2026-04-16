#!/usr/bin/env bash
# Install the UCIL watchdog to run on boot.
#
# Tries (in order):
#   1. systemd --user service (preferred on modern Linux desktops)
#   2. crontab @reboot entry  (fallback)
#
# Usage:
#   scripts/install-watchdog.sh            # auto-detect
#   scripts/install-watchdog.sh --systemd  # force systemd --user
#   scripts/install-watchdog.sh --cron     # force cron @reboot
#   scripts/install-watchdog.sh --uninstall
#
# Idempotent. Running twice is a no-op.
set -uo pipefail

cd "$(git rev-parse --show-toplevel)"
REPO_ROOT="$(pwd)"
WATCHDOG="$REPO_ROOT/scripts/_watchdog.sh"

if [[ ! -x "$WATCHDOG" ]]; then
  echo "ERROR: $WATCHDOG missing or not executable" >&2
  exit 1
fi

MODE="${UCIL_WATCHDOG_INSTALL_MODE:-auto}"
UNINSTALL=0
for arg in "$@"; do
  case "$arg" in
    --systemd)  MODE=systemd ;;
    --cron)     MODE=cron ;;
    --auto)     MODE=auto ;;
    --uninstall|-u) UNINSTALL=1 ;;
    -*) echo "unknown flag: $arg" >&2; exit 2 ;;
  esac
done

SYSTEMD_UNIT_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
SYSTEMD_UNIT="$SYSTEMD_UNIT_DIR/ucil-watchdog.service"
CRON_TAG='# UCIL watchdog (ucil-watchdog)'

systemd_available() {
  # systemd --user viable when:
  # - systemctl --user works (user bus is reachable)
  # - XDG_RUNTIME_DIR is set (or loginctl has linger)
  command -v systemctl >/dev/null 2>&1 || return 1
  systemctl --user status --no-pager >/dev/null 2>&1 || return 1
  return 0
}

cron_available() {
  command -v crontab >/dev/null 2>&1
}

install_systemd() {
  mkdir -p "$SYSTEMD_UNIT_DIR"
  cat > "$SYSTEMD_UNIT" <<EOF
[Unit]
Description=UCIL autonomous-build watchdog
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
WorkingDirectory=$REPO_ROOT
ExecStart=/usr/bin/env bash $WATCHDOG
Restart=on-failure
RestartSec=30
# Give resume.sh time to finish cleanup on shutdown
TimeoutStopSec=60
KillMode=mixed
KillSignal=SIGTERM
# Plenty of FDs for subshells and claude launches
LimitNOFILE=4096

[Install]
WantedBy=default.target
EOF
  systemctl --user daemon-reload
  systemctl --user enable ucil-watchdog.service >/dev/null 2>&1
  systemctl --user restart ucil-watchdog.service
  echo "[install-watchdog] systemd --user service installed: $SYSTEMD_UNIT"
  echo "[install-watchdog]   status:  systemctl --user status ucil-watchdog"
  echo "[install-watchdog]   logs:    journalctl --user -u ucil-watchdog -f"
  echo "[install-watchdog]   stop:    systemctl --user stop ucil-watchdog"
  echo "[install-watchdog]   disable: $0 --uninstall"
  # Hint about user-lingering for true on-boot
  if command -v loginctl >/dev/null 2>&1 && ! loginctl show-user "$(id -un)" 2>/dev/null | grep -q 'Linger=yes'; then
    echo ""
    echo "NOTE: user-services only run while you're logged in. For true"
    echo "      boot-time activation, enable lingering as root:"
    echo "        sudo loginctl enable-linger $(id -un)"
  fi
}

uninstall_systemd() {
  if [[ -f "$SYSTEMD_UNIT" ]]; then
    systemctl --user stop ucil-watchdog.service 2>/dev/null || true
    systemctl --user disable ucil-watchdog.service 2>/dev/null || true
    rm -f "$SYSTEMD_UNIT"
    systemctl --user daemon-reload 2>/dev/null || true
    echo "[install-watchdog] systemd unit removed"
  fi
}

install_cron() {
  local line
  line="@reboot cd $REPO_ROOT && nohup $WATCHDOG >> $REPO_ROOT/ucil-build/telemetry/watchdog-boot.log 2>&1 &  $CRON_TAG"
  # Fetch current crontab (or empty) — avoid wiping anything the user has.
  local current
  current="$(crontab -l 2>/dev/null || echo '')"
  if echo "$current" | grep -qF "$CRON_TAG"; then
    echo "[install-watchdog] cron entry already present; leaving alone"
    return 0
  fi
  { echo "$current"; echo "$line"; } | crontab -
  echo "[install-watchdog] cron @reboot entry added:"
  echo "  $line"
  echo ""
  echo "NOTE: cron will only run this on the NEXT reboot. To start now:"
  echo "  nohup $WATCHDOG >/dev/null 2>&1 &"
}

uninstall_cron() {
  local current
  current="$(crontab -l 2>/dev/null || echo '')"
  if ! echo "$current" | grep -qF "$CRON_TAG"; then
    echo "[install-watchdog] no cron entry to remove"
    return 0
  fi
  echo "$current" | grep -vF "$CRON_TAG" | crontab -
  echo "[install-watchdog] cron entry removed"
}

manual_install_hint() {
  cat <<EOF
Manual install (neither systemd --user nor cron was available):

  # Foreground (to test):
  $WATCHDOG

  # Background (detached):
  nohup $WATCHDOG >> $REPO_ROOT/ucil-build/telemetry/watchdog.log 2>&1 &

  # On boot via a minimal shell-profile hook (~/.profile or ~/.bashrc):
  (cd $REPO_ROOT && nohup $WATCHDOG >/dev/null 2>&1 & disown) 2>/dev/null
EOF
}

if [[ "$UNINSTALL" -eq 1 ]]; then
  uninstall_systemd
  uninstall_cron
  # Also kill a running watchdog
  pidfile="$REPO_ROOT/ucil-build/.watchdog.pid"
  if [[ -f "$pidfile" ]]; then
    pid=$(cat "$pidfile" 2>/dev/null || echo "")
    if [[ -n "$pid" ]] && [[ -d "/proc/$pid" ]]; then
      kill -TERM "$pid" 2>/dev/null || true
      echo "[install-watchdog] signalled running watchdog (pid $pid)"
    fi
    rm -f "$pidfile"
  fi
  exit 0
fi

case "$MODE" in
  systemd)
    if systemd_available; then
      install_systemd
    else
      echo "ERROR: systemd --user not available on this host" >&2
      manual_install_hint
      exit 1
    fi
    ;;
  cron)
    if cron_available; then
      install_cron
    else
      echo "ERROR: crontab not available on this host" >&2
      manual_install_hint
      exit 1
    fi
    ;;
  auto)
    if systemd_available; then
      install_systemd
    elif cron_available; then
      install_cron
    else
      echo "[install-watchdog] neither systemd --user nor cron is available"
      manual_install_hint
      exit 1
    fi
    ;;
  *) echo "bad mode: $MODE" >&2; exit 2 ;;
esac
