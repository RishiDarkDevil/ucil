#!/usr/bin/env bash
# Phase 2 — Plugins + G1/G2 + embeddings
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"
FAIL=0
check() { local n="$1"; shift; if "$@"; then echo "  [OK]   $n"; else echo "  [FAIL] $n"; FAIL=1; fi; }
echo "-- Phase 2 checks --"
check "cargo test --workspace"             cargo nextest run --workspace --no-fail-fast 2>/dev/null || cargo test --workspace --no-fail-fast
[[ -x scripts/verify/plugin-hot-cold.sh ]]    && check "plugin HOT/COLD lifecycle"     scripts/verify/plugin-hot-cold.sh
[[ -x scripts/verify/bench-embed.sh ]]        && check "CodeRankEmbed >50 chunks/sec" scripts/verify/bench-embed.sh
[[ -x scripts/verify/golden-fusion.sh ]]      && check "G1/G2 golden fusion"          scripts/verify/golden-fusion.sh
[[ -x scripts/verify/recall-at-10.sh ]]       && check "recall@10 >= 0.85"            scripts/verify/recall-at-10.sh
check "effectiveness (phase 2 scenarios)"  scripts/verify/effectiveness-gate.sh 2
check "multi-lang probes"                  scripts/verify/multi-lang-coverage.sh 2
check "real-repo smoke"                    scripts/verify/real-repo-smoke.sh 2

# Anti-laziness quality gates — Phase 2 lights up embeddings + agents crates
# on top of Phase 1's four. Auto-skip any crate dir not yet present.
for crate in ucil-core ucil-daemon ucil-treesitter ucil-lsp-diagnostics ucil-embeddings ucil-agents; do
  check "mutation gate: ${crate}"          scripts/verify/mutation-gate.sh "${crate}" 70
  check "coverage gate: ${crate}"          scripts/verify/coverage-gate.sh "${crate}" 85 75
done
exit $FAIL
