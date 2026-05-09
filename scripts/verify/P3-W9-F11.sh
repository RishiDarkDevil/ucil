#!/usr/bin/env bash
# Acceptance test for P3-W9-F11 — Salsa engine early-cutoff
# integration test (`test_incremental` covering whitespace-only
# contents change + semantic-change control case).
#
# Master-plan §10 (Salsa incremental computation), §17.2 line
# 1693 (`tests/integration/test_incremental.rs` placement), §18
# Phase 3 Week 9 (incremental computation integration test suite).
#
# Implemented by WO-0095. The frozen tests at module ROOT are the
# load-bearing acceptance signal per DEC-0007.

set -euo pipefail

cd "$(dirname "$0")/../.."

echo "[P3-W9-F11] sanity test: tests/integration/test_incremental.rs exists ..."
test -f tests/integration/test_incremental.rs

echo "[P3-W9-F11] sanity grep: [[test]] entry for test_incremental in Cargo.toml ..."
grep -qE 'name = "test_incremental"' tests/integration/Cargo.toml

echo "[P3-W9-F11] cargo test --test test_incremental ..."
cargo test --test test_incremental --no-fail-fast 2>&1 \
  | tee /tmp/p3-w9-f11-test.log \
  | tail -10

if grep -qE 'test result: ok\.' /tmp/p3-w9-f11-test.log; then
  echo "[P3-W9-F11] PASS"
  exit 0
fi

echo "[P3-W9-F11] FAIL — see /tmp/p3-w9-f11-test.log"
exit 1
