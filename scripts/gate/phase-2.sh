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
exit $FAIL
