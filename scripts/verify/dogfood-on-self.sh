#!/usr/bin/env bash
# Dogfood test — UCIL indexes the ucil repo itself and answers a query
# that we know the correct answer to (because we built the repo).
#
# Contract (implement by Phase 3):
#   1. In a fresh tempdir, `git clone /home/rishidarkdevil/Desktop/ucil`.
#   2. `ucil init` there.
#   3. Query: "find the function that flips a feature's passes=true".
#   4. Assert the response identifies scripts/flip-feature.sh (or the
#      future Rust equivalent) at line ≈90 where the jq `--arg id` block
#      lives.
#   5. Query: "explain how the verifier session marker works".
#   6. Assert the response cites ucil-build/.verifier-lock,
#      scripts/spawn-verifier.sh, and scripts/flip-feature.sh's identity
#      check.
#
# This is the meta-test: UCIL built itself, now it explains itself.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
PHASE="${1:-$(jq -r .phase ucil-build/progress.json)}"

case "$PHASE" in
  0|1|2) echo "[dogfood] phase $PHASE: not required (needs Phase 3 agents)"; exit 0 ;;
  *) ;;
esac

echo "[dogfood] phase=$PHASE"
echo "[dogfood] TODO: UCIL-on-UCIL dogfood; required by Phase 3 gate."
exit 1
