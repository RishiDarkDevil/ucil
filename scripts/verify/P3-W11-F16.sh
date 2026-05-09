#!/usr/bin/env bash
# Acceptance test for P3-W11-F16 — full-pipeline cross-group MCP
# integration test bundle (test_query_pipeline.rs +
# test_fusion.rs).
#
# Master-plan §3.2 row 4 (`search_code` — hybrid search), §3.2 row
# 8 (`get_architecture`), §3.4 + §6.2 lines 643-658 (cross-group
# RRF fusion + query-type weight matrix), §5.1 lines 430-442 (G1
# fusion authority ladder), §5.2 line 457 (G2 weight table), §6.1
# line 606 (degraded groups + per-group timeout), §17.2 line 1693
# (`tests/integration/test_query_pipeline.rs` +
# `tests/integration/test_fusion.rs` placement), §18 Phase 3 Week
# 11.
#
# Implemented by WO-0094. Frozen tests at module ROOT are the load-
# bearing acceptance signal per DEC-0007. Per
# feature-list.json:P3-W11-F16.acceptance_tests[0] the selector is
# `--test test_query_pipeline --test test_fusion` (verbatim — both
# binaries must pass).

set -euo pipefail

cd "$(dirname "$0")/../.."

echo "[P3-W11-F16] sanity test: tests/integration/test_query_pipeline.rs exists ..."
test -f tests/integration/test_query_pipeline.rs

echo "[P3-W11-F16] sanity test: tests/integration/test_fusion.rs exists ..."
test -f tests/integration/test_fusion.rs

echo "[P3-W11-F16] sanity grep: [[test]] entries for test_query_pipeline + test_fusion in Cargo.toml ..."
grep -qE 'name = "test_query_pipeline"' tests/integration/Cargo.toml
grep -qE 'name = "test_fusion"' tests/integration/Cargo.toml

echo "[P3-W11-F16] cargo test --test test_query_pipeline --test test_fusion (verbatim feature-list selector) ..."
cargo test --test test_query_pipeline --test test_fusion --no-fail-fast 2>&1 \
  | tee /tmp/p3-w11-f16-test.log \
  | tail -20

if grep -qE 'test result: ok\.' /tmp/p3-w11-f16-test.log; then
  # Both test binaries must report `test result: ok.`; with two
  # `--test` arguments cargo prints one block per binary, so
  # confirm at least 2 hits before declaring PASS.
  hits=$(grep -cE 'test result: ok\.' /tmp/p3-w11-f16-test.log)
  if [ "$hits" -ge 2 ]; then
    echo "[P3-W11-F16] PASS"
    exit 0
  fi
fi

echo "[P3-W11-F16] FAIL — see /tmp/p3-w11-f16-test.log"
exit 1
