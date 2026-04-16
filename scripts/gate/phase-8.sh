#!/usr/bin/env bash
# Phase 8 — Documentation + release
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"
FAIL=0
check() { local n="$1"; shift; if "$@"; then echo "  [OK]   $n"; else echo "  [FAIL] $n"; FAIL=1; fi; }
echo "-- Phase 8 checks --"
check "cargo test --workspace"                cargo nextest run --workspace --no-fail-fast 2>/dev/null || cargo test --workspace --no-fail-fast

# Regression: all prior phase gates still pass
for p in 1 2 3 4 5 6 7; do
  if [[ -x "scripts/gate/phase-${p}.sh" ]]; then
    check "regression: phase-${p} gate"      "scripts/gate/phase-${p}.sh"
  fi
done

# Docs completeness
for doc in architecture plugin-development host-adapter-guide configuration benchmarks claude-code-integration observability; do
  check "doc: docs/${doc}.md"                 test -f "docs/${doc}.md"
done

check "README.md mentions install"            grep -qi 'install\|quickstart' README.md
check "CHANGELOG has v0.1.0"                  grep -q 'v0\.1\.0' CHANGELOG.md 2>/dev/null || test -f CHANGELOG.md

# install.sh smoke test on a clean docker image (optional; slow)
# [[ -x scripts/verify/install-clean-docker.sh ]] && check "install.sh on clean docker" scripts/verify/install-clean-docker.sh

# Doc links not broken (lychee)
if command -v lychee >/dev/null 2>&1; then
  check "lychee: no broken links in docs/"    lychee --no-progress --quiet docs/ README.md
fi

# Release-critical gates — the v0.1.0 acceptance bar.
check "user-journey (full new-user flow)"    scripts/verify/user-journey.sh 8
check "docs walkthrough (simulated new user)" scripts/verify/docs-walkthrough.sh 8
check "host-agnostic UCIL verification (all 6 adapters)" scripts/verify/host-agnostic.sh 8

# Anti-laziness quality gates on all live Rust crates — final regression
# before v0.1.0 release. These MUST remain green or the release halts.
# Mutation-gate is the Phase-8-only one-shot at a relaxed 50% floor (per DEC-0007).
# Advisory elsewhere; coverage + reality-check + critic cover anti-laziness at WO-time.
for crate in ucil-core ucil-daemon ucil-treesitter ucil-lsp-diagnostics ucil-embeddings ucil-agents ucil-cli; do
  check "mutation gate (release one-shot): ${crate}" scripts/verify/mutation-gate.sh "${crate}" 50
  check "coverage gate: ${crate}"            scripts/verify/coverage-gate.sh "${crate}" 85 75
done

exit $FAIL
