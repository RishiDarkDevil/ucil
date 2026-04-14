#!/usr/bin/env bash
# Phase 6 — Performance + observability
# Note: all previous phase gates must still pass (regression).
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"
FAIL=0
check() { local n="$1"; shift; if "$@"; then echo "  [OK]   $n"; else echo "  [FAIL] $n"; FAIL=1; fi; }
echo "-- Phase 6 checks --"
check "cargo test --workspace"                cargo nextest run --workspace --no-fail-fast 2>/dev/null || cargo test --workspace --no-fail-fast

# Regression: phases 1-5 gates must still pass
for p in 1 2 3 4 5; do
  if [[ -x "scripts/gate/phase-${p}.sh" ]]; then
    check "regression: phase-${p} gate"      "scripts/gate/phase-${p}.sh"
  fi
done

[[ -x scripts/verify/bench-p95-500ms.sh ]]      && check "P95 query <500ms"             scripts/verify/bench-p95-500ms.sh
[[ -x scripts/verify/cache-hit-rates.sh ]]      && check "cache hit rates (L0>60%)"     scripts/verify/cache-hit-rates.sh
[[ -x scripts/verify/otel-exports.sh ]]         && check "OTel spans exported"          scripts/verify/otel-exports.sh
exit $FAIL
