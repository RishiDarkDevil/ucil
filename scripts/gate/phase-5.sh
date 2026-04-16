#!/usr/bin/env bash
# Phase 5 — Knowledge evolution + security + compaction
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"
FAIL=0
check() { local n="$1"; shift; if "$@"; then echo "  [OK]   $n"; else echo "  [FAIL] $n"; FAIL=1; fi; }
echo "-- Phase 5 checks --"
check "cargo test --workspace"                cargo nextest run --workspace --no-fail-fast 2>/dev/null || cargo test --workspace --no-fail-fast
[[ -x scripts/verify/post-commit-reindex.sh ]]  && check "post-commit re-index works"   scripts/verify/post-commit-reindex.sh
[[ -x scripts/verify/security-scan-known.sh ]]  && check "security_scan flags known-bad" scripts/verify/security-scan-known.sh
[[ -x scripts/verify/compaction.sh ]]           && check "compaction rules"             scripts/verify/compaction.sh
[[ -x scripts/verify/review-changes.sh ]]       && check "review_changes tool"          scripts/verify/review-changes.sh
check "effectiveness (phase 5 scenarios)"  scripts/verify/effectiveness-gate.sh 5
check "privacy / data-locality scan"       scripts/verify/privacy-scan.sh 5

# Anti-laziness quality gates on all live Rust crates.
for crate in ucil-core ucil-daemon ucil-treesitter ucil-lsp-diagnostics ucil-embeddings ucil-agents ucil-cli; do
  check "mutation gate: ${crate}"          scripts/verify/mutation-gate.sh "${crate}" 70
  check "coverage gate: ${crate}"          scripts/verify/coverage-gate.sh "${crate}" 85 75
done
exit $FAIL
