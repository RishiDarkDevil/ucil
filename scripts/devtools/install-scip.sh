#!/usr/bin/env bash
# Idempotent helper that documents how to install the `scip` forensic CLI
# pinned by `plugins/structural/scip/plugin.toml`.
#
# This script does NOT auto-install — it reports the current state and
# prints the recommended install command so operators choose how to
# install (go install, direct release-binary download, or a system
# package manager).
#
# Exit code is always 0; this is informational scaffolding per the
# WO-0044 install-helper convention (see `install-ast-grep.sh`,
# `install-probe.sh`).
set -euo pipefail

PINNED_VERSION="0.7.1"

if command -v scip >/dev/null 2>&1; then
    found_version="$(scip --version 2>/dev/null | awk '{print $NF}' || true)"
    if [[ -n "$found_version" ]]; then
        printf '[OK] scip %s found at: %s (plugin.toml pins %s)\n' \
            "$found_version" "$(command -v scip)" "$PINNED_VERSION"
    else
        printf '[OK] scip found at: %s (plugin.toml pins %s)\n' \
            "$(command -v scip)" "$PINNED_VERSION"
    fi
    exit 0
fi

printf '[MISSING] scip is not on PATH.\n'
printf 'To install scip CLI %s pinned by plugins/structural/scip/plugin.toml:\n' \
    "$PINNED_VERSION"
printf '  go install github.com/sourcegraph/scip/cmd/scip@v%s\n' "$PINNED_VERSION"
printf '  # OR (direct release binary):\n'
printf '  # https://github.com/sourcegraph/scip/releases/tag/v%s\n' \
    "$PINNED_VERSION"
printf '\n'
printf 'After install, verify with:\n'
printf '  scip --version\n'
exit 0
