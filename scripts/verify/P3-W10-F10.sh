#!/usr/bin/env bash
# Acceptance test for P3-W10-F10 — conflict resolution
# (master-plan §18 Phase 3 Week 10 deliverable #4, line 1811)
#
# Implemented by WO-0084. Pure-deterministic ucil-core surface — the
# frozen test at module root is the load-bearing acceptance signal per
# DEC-0007.

set -euo pipefail

cd "$(dirname "$0")/../.."

echo "[P3-W10-F10] cargo test fusion::test_conflict_resolution …"
cargo test -p ucil-core fusion::test_conflict_resolution 2>&1 \
  | tee /tmp/p3-w10-f10-test.log \
  | tail -5

if grep -qE 'test result: ok\. 1 passed; 0 failed' /tmp/p3-w10-f10-test.log; then
  echo "[P3-W10-F10] PASS"
  exit 0
fi

echo "[P3-W10-F10] FAIL — see /tmp/p3-w10-f10-test.log"
exit 1
