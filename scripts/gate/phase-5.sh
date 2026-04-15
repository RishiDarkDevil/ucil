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
exit $FAIL
