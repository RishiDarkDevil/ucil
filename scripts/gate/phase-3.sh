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
check "effectiveness (phase 3 scenarios)"  scripts/verify/effectiveness-gate.sh 3
check "multi-lang probes"                  scripts/verify/multi-lang-coverage.sh 3
check "concurrency (3-agent)"              scripts/verify/concurrency.sh 3
check "dogfood on ucil repo"               scripts/verify/dogfood-on-self.sh 3

# Anti-laziness quality gates — Phase 3 adds ucil-cli (orchestration).
for crate in ucil-core ucil-daemon ucil-treesitter ucil-lsp-diagnostics ucil-embeddings ucil-agents ucil-cli; do
  check "coverage gate: ${crate}"          scripts/verify/coverage-gate.sh "${crate}" 85 75
done
exit $FAIL
