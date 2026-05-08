#!/usr/bin/env bash
# Acceptance test for P3-W10-F12 — multi-tier query merging
# (master-plan §18 Phase 3 Week 10 deliverable #6, line 1813;
# §17 line 1636 directory entry; §12.3 hot/warm/cold tier semantics;
# §11.3 pull-based-relevance recency bias)
#
# Implemented by WO-0084. Pure-deterministic ucil-core surface — the
# frozen test at module root is the load-bearing acceptance signal per
# DEC-0007.

set -euo pipefail

cd "$(dirname "$0")/../.."

echo "[P3-W10-F12] cargo test tier_merger::test_multi_tier_query_merge …"
cargo test -p ucil-core tier_merger::test_multi_tier_query_merge 2>&1 \
  | tee /tmp/p3-w10-f12-test.log \
  | tail -5

if grep -qE 'test result: ok\. 1 passed; 0 failed' /tmp/p3-w10-f12-test.log; then
  echo "[P3-W10-F12] PASS"
  exit 0
fi

echo "[P3-W10-F12] FAIL — see /tmp/p3-w10-f12-test.log"
exit 1
