#!/usr/bin/env bash
# Acceptance test for P3-W11-F14 — testing-pipeline MCP integration
# test (`run_tests` G8 discovery dispatch).
#
# Master-plan §3.2 row 15 (`run_tests` MCP tool routes to G8), §5.8
# lines 561-579 ("Discover ALL relevant tests via ALL methods: 1.
# Convention-based / 2. Import-based / 3. KG-based — concurrently,
# then merge"), §6.1 lines 605-608 (per-group timeout +
# degraded_groups surface), §15.2 line 1521 (`ucil.group.testing`
# span), §17.2 line 1693
# (`tests/integration/test_testing_pipeline.rs` placement), §18
# Phase 3 Week 11.
#
# Implemented by WO-0094. The frozen test at module ROOT is the
# load-bearing acceptance signal per DEC-0007.

set -euo pipefail

cd "$(dirname "$0")/../.."

echo "[P3-W11-F14] sanity test: tests/integration/test_testing_pipeline.rs exists ..."
test -f tests/integration/test_testing_pipeline.rs

echo "[P3-W11-F14] sanity grep: [[test]] entry for test_testing_pipeline in Cargo.toml ..."
grep -qE 'name = "test_testing_pipeline"' tests/integration/Cargo.toml

echo "[P3-W11-F14] cargo test --test test_testing_pipeline ..."
cargo test --test test_testing_pipeline --no-fail-fast 2>&1 \
  | tee /tmp/p3-w11-f14-test.log \
  | tail -10

if grep -qE 'test result: ok\.' /tmp/p3-w11-f14-test.log; then
  echo "[P3-W11-F14] PASS"
  exit 0
fi

echo "[P3-W11-F14] FAIL — see /tmp/p3-w11-f14-test.log"
exit 1
