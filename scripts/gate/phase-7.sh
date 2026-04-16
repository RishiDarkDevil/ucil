#!/usr/bin/env bash
# Phase 7 — Database + infrastructure integration
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"
FAIL=0
check() { local n="$1"; shift; if "$@"; then echo "  [OK]   $n"; else echo "  [FAIL] $n"; FAIL=1; fi; }
echo "-- Phase 7 checks --"
check "cargo test --workspace"                cargo nextest run --workspace --no-fail-fast 2>/dev/null || cargo test --workspace --no-fail-fast
[[ -x scripts/verify/dbhub.sh ]]                && check "DBHub schema query"           scripts/verify/dbhub.sh
[[ -x scripts/verify/prisma.sh ]]               && check "Prisma migration round-trip"  scripts/verify/prisma.sh
[[ -x scripts/verify/sentry.sh ]]               && check "Sentry error payload parse"   scripts/verify/sentry.sh
[[ -x scripts/verify/query-database-tool.sh ]]  && check "query_database tool"          scripts/verify/query-database-tool.sh
[[ -x scripts/verify/check-runtime-tool.sh ]]   && check "check_runtime tool"           scripts/verify/check-runtime-tool.sh
check "effectiveness (phase 7 scenarios)"       scripts/verify/effectiveness-gate.sh 7
check "host-agnostic UCIL verification"         scripts/verify/host-agnostic.sh 7

# Anti-laziness quality gates on all live Rust crates.
for crate in ucil-core ucil-daemon ucil-treesitter ucil-lsp-diagnostics ucil-embeddings ucil-agents ucil-cli; do
  check "mutation gate: ${crate}"               scripts/verify/mutation-gate.sh "${crate}" 70
  check "coverage gate: ${crate}"               scripts/verify/coverage-gate.sh "${crate}" 85 75
done
exit $FAIL
