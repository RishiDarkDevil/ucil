#!/usr/bin/env bash
# Long-running stability / memory-leak check for the daemon.
#
# Contract (implement by Phase 6):
#   1. Start `ucild` against a fresh fixture.
#   2. For 30 minutes, drive mixed load:
#        - file changes every 5s (touch a .rs file)
#        - queries every 2s (random from a fixed 20-query pool)
#        - branch switches every 5 minutes
#   3. Sample RSS every 30s. Assert:
#        - RSS never exceeds 512MB (master plan §21)
#        - RSS at t=30min is within 20% of RSS at t=5min (no linear leak)
#        - no unhandled panics in daemon.log
#        - no file-descriptor leak (daemon open FDs stable)
#        - P95 query latency at t=30min within 10% of t=1min (no
#          slowdown from accumulated state)
#   4. Graceful shutdown (SIGTERM): daemon exits cleanly within 5s.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
PHASE="${1:-$(jq -r .phase ucil-build/progress.json)}"

case "$PHASE" in
  6|7|8) ;;
  *) echo "[stability] phase $PHASE: not required"; exit 0 ;;
esac

echo "[stability] phase=$PHASE"
echo "[stability] TODO: 30-min mixed-load stability test; required by Phase 6 gate."
exit 1
