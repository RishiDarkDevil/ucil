#!/usr/bin/env bash
# scripts/push-with-retry.sh
#
# git push with auto-retry on network failure. If push fails AND the
# failure looks network-related, invoke recover-network.sh and retry.
# After 3 total attempts (initial + 2 retries with recovery), give up
# and exit non-zero so the caller knows something durable is wrong.
#
# Usage: scripts/push-with-retry.sh [git-push-args...]
#   defaults to: git push origin <current-branch>
set -uo pipefail

cd "$(git rev-parse --show-toplevel 2>/dev/null)" || exit 1

REPO_ROOT="$(pwd)"
ARGS=("$@")
if [[ ${#ARGS[@]} -eq 0 ]]; then
  branch="$(git rev-parse --abbrev-ref HEAD 2>/dev/null)"
  ARGS=(origin "$branch")
fi

log() { echo "[push-with-retry] $*"; }

attempt_push() {
  git push "${ARGS[@]}" 2>&1
}

# Network-related error patterns to trigger wifi-toggle.
is_network_failure() {
  local out="$1"
  echo "$out" | grep -qiE "Could not resolve host|Failed to connect|Network is unreachable|fatal: unable to access|Connection refused|Operation timed out|TLS connection was non-properly terminated"
}

for attempt in 1 2 3; do
  log "push attempt $attempt: git push ${ARGS[*]}"
  out="$(attempt_push 2>&1)"
  rc=$?
  echo "$out"

  if (( rc == 0 )); then
    log "push succeeded on attempt $attempt"
    exit 0
  fi

  if ! is_network_failure "$out"; then
    log "push failed for non-network reason (rc=$rc); not retrying"
    exit "$rc"
  fi

  if (( attempt < 3 )) && [[ -x "$REPO_ROOT/scripts/recover-network.sh" ]]; then
    log "network failure detected; invoking recover-network.sh"
    "$REPO_ROOT/scripts/recover-network.sh" || log "recover-network.sh returned non-zero"
  fi
done

log "exhausted 3 push attempts; giving up"
exit 1
