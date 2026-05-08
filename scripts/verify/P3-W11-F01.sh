#!/usr/bin/env bash
# Acceptance test for P3-W11-F01 — G7 (Quality) parallel pipeline orchestrator
# (master-plan §3.2 row 14 `check_quality`, §5.7 lines 539-559, §15.2 line 1519
# `ucil.group.quality` span, §17 line 1636 `g7.rs` placement, §18 Phase 3 Week 11
# deliverable #1).
#
# Implemented by WO-0085. UCIL-internal G7Source trait + execute_g7
# orchestrator — production LspDiagnosticsG7Source impl deferred per
# DEC-0008 §4 dependency-inversion seam.

set -euo pipefail

cd "$(dirname "$0")/../.."

echo "[P3-W11-F01] cargo test g7::test_g7_parallel_pipeline …"
cargo test -p ucil-daemon g7::test_g7_parallel_pipeline 2>&1 \
  | tee /tmp/p3-w11-f01-test.log \
  | tail -5

if ! grep -qE 'test result: ok\. 1 passed; 0 failed' /tmp/p3-w11-f01-test.log; then
  echo "[P3-W11-F01] FAIL — see /tmp/p3-w11-f01-test.log"
  exit 1
fi

echo "[P3-W11-F01] grep pub trait G7Source …"
if ! grep -qE 'pub trait G7Source' crates/ucil-daemon/src/g7.rs; then
  echo "[P3-W11-F01] FAIL — pub trait G7Source missing"
  exit 1
fi

echo "[P3-W11-F01] grep pub async fn execute_g7 …"
if ! grep -qE 'pub async fn execute_g7' crates/ucil-daemon/src/g7.rs; then
  echo "[P3-W11-F01] FAIL — pub async fn execute_g7 missing"
  exit 1
fi

echo "[P3-W11-F01] grep tracing instrument ucil.group.quality …"
if ! grep -qE 'tracing::instrument' crates/ucil-daemon/src/g7.rs \
  || ! grep -qE 'ucil.group.quality' crates/ucil-daemon/src/g7.rs; then
  echo "[P3-W11-F01] FAIL — ucil.group.quality span missing"
  exit 1
fi

echo "[P3-W11-F01] cargo clippy -p ucil-daemon …"
cargo clippy -p ucil-daemon --all-targets -- -D warnings 2>&1 | tail -3

echo "[P3-W11-F01] cargo fmt -p ucil-daemon --check …"
cargo fmt -p ucil-daemon --check

echo "[P3-W11-F01] PASS"
exit 0
