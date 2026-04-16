#!/usr/bin/env bash
# Phase 4 — Host adapters + Claude Code plugin
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"
FAIL=0
check() { local n="$1"; shift; if "$@"; then echo "  [OK]   $n"; else echo "  [FAIL] $n"; FAIL=1; fi; }
echo "-- Phase 4 checks --"
check "cargo test --workspace"                cargo nextest run --workspace --no-fail-fast 2>/dev/null || cargo test --workspace --no-fail-fast
[[ -x scripts/verify/install-claude-plugin.sh ]] && check "Claude Code plugin installs"  scripts/verify/install-claude-plugin.sh
[[ -x scripts/verify/codex-adapter.sh ]]         && check "Codex CLI adapter"            scripts/verify/codex-adapter.sh
[[ -x scripts/verify/cursor-adapter.sh ]]        && check "Cursor adapter"               scripts/verify/cursor-adapter.sh
[[ -x scripts/verify/cline-adapter.sh ]]         && check "Cline adapter"                scripts/verify/cline-adapter.sh
[[ -x scripts/verify/aider-adapter.sh ]]         && check "Aider adapter"                scripts/verify/aider-adapter.sh
[[ -x scripts/verify/post-tool-hook-timing.sh ]] && check "PostToolUse hook <200ms"      scripts/verify/post-tool-hook-timing.sh
check "effectiveness (phase 4 scenarios)"  scripts/verify/effectiveness-gate.sh 4
check "host-adapter conformance (all)"     scripts/verify/host-adapter-conformance.sh 4

# Anti-laziness quality gates — still enforce on all live Rust crates even
# though Phase 4 is adapter-heavy (TypeScript). Regression guard.
for crate in ucil-core ucil-daemon ucil-treesitter ucil-lsp-diagnostics ucil-embeddings ucil-agents ucil-cli; do
  check "mutation gate: ${crate}"          scripts/verify/mutation-gate.sh "${crate}" 70
  check "coverage gate: ${crate}"          scripts/verify/coverage-gate.sh "${crate}" 85 75
done
exit $FAIL
