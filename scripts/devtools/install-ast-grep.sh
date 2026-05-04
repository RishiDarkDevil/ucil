#!/usr/bin/env bash
# Idempotent helper that documents how to install the ast-grep CLI binary
# pinned by `plugins/structural/ast-grep/plugin.toml`.
#
# This script does NOT auto-install — it reports the current state and
# prints the recommended install command so operators choose how to
# install (cargo, npm, or a system package manager).
#
# Exit code is always 0; this is informational scaffolding.
set -euo pipefail

PINNED_VERSION="0.42.1"

if command -v ast-grep >/dev/null 2>&1; then
    found_version="$(ast-grep --version 2>/dev/null | awk '{print $NF}')"
    if [[ "$found_version" == "$PINNED_VERSION" ]]; then
        printf '[OK] ast-grep %s already installed at: %s\n' \
            "$found_version" "$(command -v ast-grep)"
    else
        printf '[WARN] ast-grep %s found at %s but plugin.toml pins %s.\n' \
            "$found_version" "$(command -v ast-grep)" "$PINNED_VERSION"
        printf '       To match the pinned version, run one of:\n'
        printf '         cargo install ast-grep --version %s\n' "$PINNED_VERSION"
        printf '         npm install -g @ast-grep/cli@%s\n' "$PINNED_VERSION"
    fi
    exit 0
fi

printf '[MISSING] ast-grep is not on PATH.\n'
printf 'To install ast-grep %s pinned by plugins/structural/ast-grep/plugin.toml:\n' \
    "$PINNED_VERSION"
printf '  cargo install ast-grep --version %s\n' "$PINNED_VERSION"
printf '  # OR\n'
printf '  npm install -g @ast-grep/cli@%s\n' "$PINNED_VERSION"
printf '  # OR (if your system package manager has it)\n'
printf '  brew install ast-grep   # ensure version matches pin\n'
printf '\n'
printf 'After install, verify with:\n'
printf '  ast-grep --version   # expected: ast-grep %s\n' "$PINNED_VERSION"
exit 0
