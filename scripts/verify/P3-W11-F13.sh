#!/usr/bin/env bash
# Acceptance test for P3-W11-F13 — quality-pipeline MCP integration
# test (`check_quality` severity-classified issues).
#
# Master-plan §3.2 row 14 (`check_quality` MCP tool — runs lint /
# type-check / security scan), §5.7 lines 539-559 (G7 parallel
# fan-out), §5.7 line 555 + §12.1 (severity ladder + lowercase
# vocabulary), §15.2 line 1519 (`ucil.group.quality` span), §17.2
# line 1693 (`tests/integration/test_quality_pipeline.rs`
# placement), §18 Phase 3 Week 11 (G7/G8 Quality + Testing).
#
# Implemented by WO-0094. The frozen test at module ROOT is the
# load-bearing acceptance signal per DEC-0007.

set -euo pipefail

cd "$(dirname "$0")/../.."

echo "[P3-W11-F13] sanity test: tests/integration/test_quality_pipeline.rs exists ..."
test -f tests/integration/test_quality_pipeline.rs

echo "[P3-W11-F13] sanity grep: [[test]] entry for test_quality_pipeline in Cargo.toml ..."
grep -qE 'name = "test_quality_pipeline"' tests/integration/Cargo.toml

echo "[P3-W11-F13] cargo test --test test_quality_pipeline ..."
cargo test --test test_quality_pipeline --no-fail-fast 2>&1 \
  | tee /tmp/p3-w11-f13-test.log \
  | tail -10

if grep -qE 'test result: ok\.' /tmp/p3-w11-f13-test.log; then
  echo "[P3-W11-F13] PASS"
  exit 0
fi

echo "[P3-W11-F13] FAIL — see /tmp/p3-w11-f13-test.log"
exit 1
