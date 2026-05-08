#!/usr/bin/env bash
# Idempotent helper that documents how to install the dependency-cruiser
# binary pinned by `plugins/architecture/dependency-cruiser/plugin.toml`
# (WO-0086 / P3-W10-F14; master-plan §4.4 line 328 — JS/TS dependency
# validation, P0 priority).
#
# This script does NOT auto-install — it reports the current state and
# prints recommended install commands. Operators install manually.
#
# Exit code is always 0; this is informational scaffolding matching the
# `install-ripgrep.sh` / `install-ast-grep.sh` shape.
set -euo pipefail

PINNED_DEPCRUISE_VERSION="17.4.0"

if command -v depcruise >/dev/null 2>&1; then
    found_version="$(depcruise --version 2>/dev/null | head -n1)"
    printf '[OK] depcruise binary at %s reports version: %s\n' \
        "$(command -v depcruise)" "$found_version"
    printf '     plugin.toml pins dependency-cruiser %s.\n' "$PINNED_DEPCRUISE_VERSION"
    if [[ "$found_version" != "$PINNED_DEPCRUISE_VERSION" ]]; then
        printf '     [WARN] installed version differs from pin; reinstall via:\n'
        printf '       npm install -g dependency-cruiser@%s\n' "$PINNED_DEPCRUISE_VERSION"
    fi
    exit 0
fi

printf '[MISSING] depcruise is not on PATH.\n'
printf 'To install dependency-cruiser pinned by plugins/architecture/dependency-cruiser/plugin.toml (%s):\n' \
    "$PINNED_DEPCRUISE_VERSION"
printf '  npm install -g dependency-cruiser@%s     # Tier-1 (canonical)\n' "$PINNED_DEPCRUISE_VERSION"
printf '  npx -y dependency-cruiser@%s             # Ephemeral one-shot\n' "$PINNED_DEPCRUISE_VERSION"
printf '\n'
printf 'After install, verify with:\n'
printf '  depcruise --version    # expected: %s\n' "$PINNED_DEPCRUISE_VERSION"
exit 0
