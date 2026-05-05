#!/usr/bin/env bash
# Idempotent helper that documents how to install the ripgrep binary
# pinned by `plugins/search/ripgrep/plugin.toml` (WO-0051 / P2-W7-F07).
#
# This script does NOT auto-install — it reports the current state and
# prints recommended install commands. Operators install manually.
#
# Exit code is always 0; this is informational scaffolding matching the
# `install-ast-grep.sh` / `install-probe.sh` shape.
set -euo pipefail

PINNED_VERSION="14.1.1"

if command -v rg >/dev/null 2>&1; then
    # `rg --version` first line is `ripgrep 14.1.1`; strip the prefix.
    found_version="$(rg --version 2>/dev/null | head -n1 | awk '{print $2}')"
    printf '[OK] ripgrep binary at %s reports version: %s\n' \
        "$(command -v rg)" "$found_version"
    printf '     plugin.toml pins ripgrep %s.\n' "$PINNED_VERSION"
    if [[ "$found_version" != "$PINNED_VERSION" ]]; then
        printf '     [WARN] installed version differs from pin; reinstall via one of:\n'
    fi
    exit 0
fi

printf '[MISSING] ripgrep is not on PATH.\n'
printf 'To install ripgrep pinned by plugins/search/ripgrep/plugin.toml (%s):\n' \
    "$PINNED_VERSION"
printf '  cargo install ripgrep\n'
printf '  brew install ripgrep              # macOS\n'
printf '  apt-get install -y ripgrep        # Debian/Ubuntu\n'
printf '\n'
printf 'After install, verify with:\n'
printf '  rg --version    # expected: ripgrep %s\n' "$PINNED_VERSION"
exit 0
