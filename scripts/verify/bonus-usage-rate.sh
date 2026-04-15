#!/usr/bin/env bash
# Bonus-context usage-rate gate — master plan §15.3 lists
# `ucil.bonus.usage_rate` as a named metric. This verifies bonus fields
# (conventions, pitfalls, related_code, etc.) are actually consumed by
# downstream agent calls, not just emitted and ignored.
#
# Contract (implement by Phase 6):
#   1. Run a synthetic session of N canned queries against UCIL.
#      Each query returns a response with bonus_context.
#   2. For each query, note which bonus fields were present.
#   3. In the same session, make a follow-up query and detect whether
#      the agent's behavior changed based on the earlier bonus context
#      (e.g., it applied a listed convention, avoided a listed pitfall,
#      invoked a suggested tool).
#   4. Compute: bonus_usage_rate = (fields-used / fields-offered).
#   5. Assert: bonus_usage_rate >= 0.30 (at least 30% of offered
#      bonuses are used). Master plan doesn't pin a number; 0.30 is our
#      chosen lower bound to detect "emitted but ignored".
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
PHASE="${1:-$(jq -r .phase ucil-build/progress.json)}"

case "$PHASE" in
  6|7|8) ;;
  *) echo "[bonus-usage] phase $PHASE: not required"; exit 0 ;;
esac

echo "[bonus-usage] phase=$PHASE"
echo "[bonus-usage] TODO: synthetic session + rate calculation; Phase 6 gate."
exit 1
