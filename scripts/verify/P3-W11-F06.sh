#!/usr/bin/env bash
# Acceptance test for P3-W11-F06 — quality_issues table tracking
# (master-plan §12.1 lines 1196-1211 columns first_seen/last_seen/resolved/
# resolved_by_session, §18 Phase 3 Week 11 deliverable #3).
#
# Implemented by WO-0085. SELECT-then-UPSERT in persist_diagnostics +
# soft_delete_resolved_quality_issues helper.

set -euo pipefail

cd "$(dirname "$0")/../.."

echo "[P3-W11-F06] cargo test quality_pipeline::test_quality_issues_tracking …"
cargo test -p ucil-lsp-diagnostics quality_pipeline::test_quality_issues_tracking 2>&1 \
  | tee /tmp/p3-w11-f06-test.log \
  | tail -5

if ! grep -qE 'test result: ok\. 1 passed; 0 failed' /tmp/p3-w11-f06-test.log; then
  echo "[P3-W11-F06] FAIL — see /tmp/p3-w11-f06-test.log"
  exit 1
fi

echo "[P3-W11-F06] grep pub async fn soft_delete_resolved_quality_issues …"
if ! grep -qE 'pub async fn soft_delete_resolved_quality_issues' \
       crates/ucil-lsp-diagnostics/src/quality_pipeline.rs; then
  echo "[P3-W11-F06] FAIL — pub async fn soft_delete_resolved_quality_issues missing"
  exit 1
fi

echo "[P3-W11-F06] cargo clippy -p ucil-lsp-diagnostics …"
cargo clippy -p ucil-lsp-diagnostics --all-targets -- -D warnings 2>&1 | tail -3

echo "[P3-W11-F06] cargo fmt -p ucil-lsp-diagnostics --check …"
cargo fmt -p ucil-lsp-diagnostics --check

echo "[P3-W11-F06] PASS"
exit 0
