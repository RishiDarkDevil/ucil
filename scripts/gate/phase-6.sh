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
check "effectiveness (phase 6 scenarios)"  scripts/verify/effectiveness-gate.sh 6
check "bonus-context usage rate >= 0.30"   scripts/verify/bonus-usage-rate.sh 6
check "stability (30-min mixed load)"      scripts/verify/stability.sh 6
check "host-agnostic UCIL verification"    scripts/verify/host-agnostic.sh 6

# Anti-laziness quality gates on all live Rust crates (regression gate).
for crate in ucil-core ucil-daemon ucil-treesitter ucil-lsp-diagnostics ucil-embeddings ucil-agents ucil-cli; do
  check "coverage gate: ${crate}"          scripts/verify/coverage-gate.sh "${crate}" 85 75
done
exit $FAIL
