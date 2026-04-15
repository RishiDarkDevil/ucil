#!/usr/bin/env bash
# Shared retry helper. Source this from any script that wants
# retry-with-exponential-backoff semantics on a flaky command.
#
# Usage:
#   source "$(dirname "$0")/_retry.sh"
#   retry_with_backoff <max_attempts> <base_delay_s> -- <command...>
#   OR (convenience wrappers below):
#   retry_git <max> <base> <git-args...>
#
# Backoff: attempt N (1-indexed) waits base * 2^(N-2) seconds after
# failure. So for max=3, base=2: wait 2s after attempt 1, 4s after
# attempt 2, give up after attempt 3. Total wall time cap: base * (2^(max-1) - 1).
#
# Exit code: returns the LAST attempt's exit code on final failure, 0 on any
# successful attempt.

retry_with_backoff() {
  local max="$1" base="$2"
  shift 2
  # Consume optional "--" separator for readability
  [[ "${1:-}" == "--" ]] && shift

  local attempt=1 rc=0
  while (( attempt <= max )); do
    "$@"
    rc=$?
    if (( rc == 0 )); then
      return 0
    fi
    if (( attempt < max )); then
      local delay=$(( base * (2 ** (attempt - 1)) ))
      echo "[_retry] '$*' failed (rc=$rc, attempt $attempt/$max); retrying in ${delay}s..." >&2
      sleep "$delay"
    else
      echo "[_retry] '$*' failed (rc=$rc, attempt $attempt/$max); giving up." >&2
    fi
    attempt=$(( attempt + 1 ))
  done
  return "$rc"
}

# Convenience wrapper for git operations that tend to flake on GitHub 5xx.
# retry_git <max> <base> <git-subcommand-and-args...>
retry_git() {
  local max="$1" base="$2"
  shift 2
  retry_with_backoff "$max" "$base" -- git "$@"
}

# Safe git pull: 3 attempts with 2s/4s backoff, then continues with stale
# state (like the original `git pull ... || true` pattern but noisy on
# actual transient failures so we notice pathological cases).
safe_git_pull() {
  retry_git 3 2 pull --quiet "$@" || {
    echo "[_retry] git pull failed after 3 tries — continuing with stale local state. Investigate if this recurs." >&2
    return 0
  }
}

# Safe git push: 3 attempts with 2s/4s backoff. Unlike pull, we DO NOT
# swallow a final failure — an unpushed commit is load-bearing.
safe_git_push() {
  retry_git 3 2 push "$@"
}

# Safe git fetch: 3 attempts with 2s/4s backoff; continues on final failure
# (fetch is advisory, never data-losing).
safe_git_fetch() {
  retry_git 3 2 fetch "$@" || {
    echo "[_retry] git fetch failed after 3 tries — continuing." >&2
    return 0
  }
}
