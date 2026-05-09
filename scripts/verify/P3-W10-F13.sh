#!/usr/bin/env bash
# Acceptance test for P3-W10-F13 — warm-tier promotion processors
# driven by AgentScheduler.
#
# Master-plan §10 [knowledge_tiering] config block lines 2015-2024
# (interval seconds + min-evidence + dedup threshold), §11 hot/warm
# schema lines 1213-1320 (hot_observations / hot_convention_signals /
# hot_architecture_deltas / hot_decision_material plus warm
# counterparts), §15.2 lines 1518-1522 (tracing span discipline),
# §17.2 line 1636 (warm-processor module placement; reinterpreted per
# DEC-0008 §4 to live in ucil-daemon), §18 Phase 3 Week 10 lines
# 1810-1815 (warm processors thread).
#
# Implemented by WO-0093. The frozen test at module root is the load-
# bearing acceptance signal per DEC-0007.

set -euo pipefail

cd "$(dirname "$0")/../.."

echo "[P3-W10-F13] sanity grep: pub mod agent_scheduler; in lib.rs …"
grep -qE '^pub mod agent_scheduler;' crates/ucil-daemon/src/lib.rs

echo "[P3-W10-F13] sanity grep: agent_scheduler.rs is a real source file …"
test -f crates/ucil-daemon/src/agent_scheduler.rs

echo "[P3-W10-F13] cargo test agent_scheduler::test_warm_processors -- --list …"
# Cargo 1.94+ requires the `-- --list` positional separator form per the
# WO-0089 §B verifier note carried into WO-0093.
test 1 -eq "$(cargo test -p ucil-daemon agent_scheduler::test_warm_processors -- --list 2>&1 \
  | grep -cE 'test_warm_processors')"

echo "[P3-W10-F13] cargo test agent_scheduler::test_warm_processors …"
cargo test -p ucil-daemon agent_scheduler::test_warm_processors --no-fail-fast 2>&1 \
  | tee /tmp/p3-w10-f13-test.log \
  | tail -10

if grep -qE 'test result: ok\. 1 passed' /tmp/p3-w10-f13-test.log; then
  echo "[P3-W10-F13] PASS"
  exit 0
fi

echo "[P3-W10-F13] FAIL — see /tmp/p3-w10-f13-test.log"
exit 1
