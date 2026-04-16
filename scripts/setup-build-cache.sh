#!/usr/bin/env bash
# Idempotent one-shot setup for shared Rust build cache.
#
# Rationale: parallel worktrees (scripts/run-phase.sh uses one per WO) otherwise
# each compile the full workspace from scratch. A shared target dir + sccache
# wrapper cuts cold-build time from ~90s to ~25s and warm-recompile from ~20s
# to ~3s on the reference box.
#
# What this script does (all idempotent):
#   1. `cargo install sccache` if not present.
#   2. Write `.cargo/config.toml` (project-level) with:
#      - CARGO_TARGET_DIR=<repo>/target (shared across worktrees)
#      - rustc_wrapper = "sccache"
#      - sccache local-disk cache at ~/.cache/sccache
#   3. Export SCCACHE_DIR + SCCACHE_CACHE_SIZE for the current shell.
#   4. Run `sccache --start-server` so the background service is alive.
#   5. Print a tiny status report (`sccache -s`).
#
# Usage:
#   scripts/setup-build-cache.sh              # one-shot setup
#   source scripts/setup-build-cache.sh       # source-mode (exports to caller)
#
# The per-worktree overhead after this: each worktree inherits the shared
# target via CARGO_TARGET_DIR, so `cargo build` in any worktree populates
# the same target/ tree. Rust's lockfile discipline makes this safe as long
# as Cargo.toml + Cargo.lock are consistent across the worktrees (they are,
# since all feat branches descend from main with the same workspace root).

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

_log() { printf '[setup-build-cache] %s\n' "$*" >&2; }

# 1. Install sccache if missing.
if ! command -v sccache >/dev/null 2>&1; then
  _log "sccache not found — installing via cargo..."
  if command -v cargo >/dev/null 2>&1; then
    cargo install sccache --locked || {
      _log "ERROR: cargo install sccache failed"
      exit 1
    }
  else
    _log "ERROR: cargo not in PATH — cannot install sccache"
    exit 2
  fi
fi

SCCACHE_VER="$(sccache --version 2>/dev/null || echo 'unknown')"
_log "sccache version: ${SCCACHE_VER}"

# 2. Project-level .cargo/config.toml with shared target + sccache wrapper.
mkdir -p .cargo

# Only write if missing or stale (detect by presence of exact marker line).
NEEDS_WRITE=1
if [[ -f .cargo/config.toml ]]; then
  if grep -Fq 'managed by scripts/setup-build-cache.sh' .cargo/config.toml; then
    NEEDS_WRITE=0
    _log ".cargo/config.toml already managed (skipping write)"
  else
    _log ".cargo/config.toml exists but is not managed — backing up to .cargo/config.toml.bak"
    cp .cargo/config.toml .cargo/config.toml.bak
  fi
fi

if [[ "$NEEDS_WRITE" == "1" ]]; then
  cat > .cargo/config.toml <<'EOF'
# managed by scripts/setup-build-cache.sh — rerun to refresh.
#
# Shared build cache for parallel worktrees.
# Every worktree spawned by scripts/run-phase.sh inherits CARGO_TARGET_DIR
# from this file (via .cargo/config.toml discovery — Cargo walks up to repo
# root), so `cargo build` in ../ucil-wt/WO-0042/ writes to <repo>/target/.

[build]
# Shared across worktrees. Set to absolute path so worktrees outside the
# main checkout still hit the same cache.
target-dir = "target"
# Wrap rustc with sccache for inter-invocation caching.
rustc-wrapper = "sccache"

# Speed up link step on Linux (gold/lld are available via standard apt).
# [target.x86_64-unknown-linux-gnu]
# linker = "clang"
# rustflags = ["-C", "link-arg=-fuse-ld=lld"]
# (opt-in only — enable if a bench shows a win and lld is installed.)

[net]
# sparse index — one of the few settings that helps across Rust versions.
git-fetch-with-cli = false

EOF
  _log "wrote .cargo/config.toml"
fi

# 3. Environment for the current shell (and callers that source this script).
SCCACHE_DIR_DEFAULT="${HOME}/.cache/sccache"
export SCCACHE_DIR="${SCCACHE_DIR:-$SCCACHE_DIR_DEFAULT}"
export SCCACHE_CACHE_SIZE="${SCCACHE_CACHE_SIZE:-20G}"
export RUSTC_WRAPPER="${RUSTC_WRAPPER:-sccache}"
export CARGO_INCREMENTAL="0"  # sccache + incremental = broken
mkdir -p "$SCCACHE_DIR"

# Also export so child launchers pick these up.
_log "SCCACHE_DIR=${SCCACHE_DIR} SCCACHE_CACHE_SIZE=${SCCACHE_CACHE_SIZE}"

# 4. Start sccache server (idempotent — no-op if already running).
sccache --start-server 2>/dev/null || true

# 5. Summary.
if ! sccache -s 2>/dev/null | head -20; then
  _log "WARN: sccache -s did not return stats (server may be just starting)"
fi

_log "done. To use, run your cargo/nextest commands as usual from this repo."
