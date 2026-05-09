#!/usr/bin/env bash
# Acceptance test for P3-W11-F12 — post-hoc feedback-loop analyser
# (master-plan §6.3 lines 626-639 [Feedback Analyzer] step;
#  §8.7 lines 824-844 4-signal taxonomy;
#  §12.1 lines 1295-1303 feedback_signals SQLite schema;
#  §17 line 1637 feedback.rs directory entry;
#  §18 Phase 3 Week 11 deliverable #7 line 1823)
#
# Implemented by WO-0096.  Pure-deterministic ucil-core surface — the
# frozen test at module root is the load-bearing acceptance signal per
# DEC-0007.  The §15.2 tracing carve-out applies (no async, no IO,
# no spans) — same shape as P3-W10-F12 / P3-W10-F11 verify scripts.

set -euo pipefail

cd "$(dirname "$0")/../.."

echo "[P3-W11-F12] cargo test feedback::test_post_hoc_analyser …"
cargo test -p ucil-core feedback::test_post_hoc_analyser 2>&1 \
  | tee /tmp/p3-w11-f12-test.log \
  | tail -5

if grep -qE 'test result: ok\. 1 passed; 0 failed' /tmp/p3-w11-f12-test.log; then
  echo "[P3-W11-F12] PASS"
  exit 0
fi

echo "[P3-W11-F12] FAIL — see /tmp/p3-w11-f12-test.log"
exit 1
