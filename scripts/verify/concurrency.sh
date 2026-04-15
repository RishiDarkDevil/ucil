#!/usr/bin/env bash
# Concurrency test — master plan §11 promises 3–5 concurrent agents
# across branches/worktrees with the shared brain. Actually exercise it.
#
# Contract (implement by Phase 3):
#   1. In tests/fixtures/rust-project/, create 3 worktrees on different branches.
#   2. Start `ucild` (UCIL daemon) if not already running.
#   3. Simultaneously spawn 3 headless claude -p sessions, each with a
#      distinct query against its worktree.
#   4. Wait for all three. Capture:
#        - per-session success/failure on their query
#        - daemon RSS before/during/after
#        - any SQLITE_BUSY, lock timeouts, or log errors in daemon.log
#   5. Assert:
#        - all 3 sessions exit 0 with correct-looking output
#        - no SQLITE_BUSY in daemon.log
#        - RSS did not exceed 600MB (master-plan cap is 512MB, 20% headroom)
#        - knowledge.db integrity: `PRAGMA integrity_check` returns "ok"
#   6. Tear down.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
PHASE="${1:-$(jq -r .phase ucil-build/progress.json)}"

case "$PHASE" in
  0|1|2) echo "[concurrency] phase $PHASE: not required (needs daemon + orchestration)"; exit 0 ;;
  *) ;;
esac

echo "[concurrency] phase=$PHASE"
echo "[concurrency] TODO: 3-way concurrent agent test; required by Phase 3 gate."
exit 1
