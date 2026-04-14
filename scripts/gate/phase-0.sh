#!/usr/bin/env bash
# Phase 0 — Bootstrap
# Gate: Cargo workspace builds, TS adapters build, Python package compiles, ucil init works.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"

FAIL=0
check() { local n="$1"; shift; if "$@"; then echo "  [OK]   $n"; else echo "  [FAIL] $n"; FAIL=1; fi; }

echo "-- Phase 0 checks --"

# Cargo workspace builds (if it exists yet)
if [[ -f Cargo.toml ]]; then
  check "cargo build --workspace"           cargo build --workspace --quiet
  check "cargo clippy --workspace -D warn." cargo clippy --workspace --quiet -- -D warnings
  check "cargo fmt --check"                 cargo fmt --all --check
fi

# TS adapters (if present)
if [[ -f adapters/package.json ]]; then
  check "pnpm -C adapters build"            bash -c 'cd adapters && pnpm -s build'
fi

# Python ML pipeline (if present)
if [[ -f ml/pyproject.toml ]]; then
  check "uv build ml/"                      bash -c 'cd ml && uv build --quiet'
fi

# `ucil init` smoke test (if the binary was built)
if [[ -x target/debug/ucil ]]; then
  TMPDIR=$(mktemp -d)
  trap 'rm -rf "$TMPDIR"' EXIT
  check "ucil init on empty temp dir"       bash -c "cd '$TMPDIR' && $PWD/target/debug/ucil init --no-install-plugins >/dev/null 2>&1 && test -f .ucil/init_report.json"
fi

exit $FAIL
