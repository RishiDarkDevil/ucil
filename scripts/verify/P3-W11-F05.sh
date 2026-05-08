#!/usr/bin/env bash
# Acceptance test for P3-W11-F05 — G7 severity-weighted merge fusion
# (master-plan §5.7 lines 539-559, §18 Phase 3 Week 11 deliverable #2).
#
# Implemented by WO-0085. Pure-deterministic CPU-bound merger — the
# frozen test at module root is the load-bearing acceptance signal per
# DEC-0007.

set -euo pipefail

cd "$(dirname "$0")/../.."

echo "[P3-W11-F05] cargo test g7::test_g7_severity_merge …"
cargo test -p ucil-daemon g7::test_g7_severity_merge 2>&1 \
  | tee /tmp/p3-w11-f05-test.log \
  | tail -5

if ! grep -qE 'test result: ok\. 1 passed; 0 failed' /tmp/p3-w11-f05-test.log; then
  echo "[P3-W11-F05] FAIL — see /tmp/p3-w11-f05-test.log"
  exit 1
fi

echo "[P3-W11-F05] grep pub fn merge_g7_by_severity …"
if ! grep -qE 'pub fn merge_g7_by_severity' crates/ucil-daemon/src/g7.rs; then
  echo "[P3-W11-F05] FAIL — pub fn merge_g7_by_severity missing"
  exit 1
fi

echo "[P3-W11-F05] grep pub enum Severity …"
if ! grep -qE 'pub enum Severity' crates/ucil-daemon/src/g7.rs; then
  echo "[P3-W11-F05] FAIL — pub enum Severity missing"
  exit 1
fi

echo "[P3-W11-F05] cargo clippy -p ucil-daemon …"
cargo clippy -p ucil-daemon --all-targets -- -D warnings 2>&1 | tail -3

echo "[P3-W11-F05] cargo fmt -p ucil-daemon --check …"
cargo fmt -p ucil-daemon --check

echo "[P3-W11-F05] PASS"
exit 0
