#!/usr/bin/env bash
# Phase 3 — Orchestration + all 8 tool groups + warm processors
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"
FAIL=0
check() { local n="$1"; shift; if "$@"; then echo "  [OK]   $n"; else echo "  [FAIL] $n"; FAIL=1; fi; }
echo "-- Phase 3 checks --"
check "cargo test --workspace"                cargo nextest run --workspace --no-fail-fast 2>/dev/null || cargo test --workspace --no-fail-fast
[[ -x scripts/verify/all-groups-live.sh ]]       && check "all 8 groups respond"         scripts/verify/all-groups-live.sh
[[ -x scripts/verify/rrf-determinism.sh ]]       && check "RRF fusion deterministic"     scripts/verify/rrf-determinism.sh
[[ -x scripts/verify/bench-e2e.sh ]]             && check "P95 end-to-end query <1s"     scripts/verify/bench-e2e.sh
[[ -x scripts/verify/conflict-resolution.sh ]]   && check "conflict resolution works"    scripts/verify/conflict-resolution.sh
exit $FAIL
