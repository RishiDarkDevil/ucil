#!/usr/bin/env bash
# Idempotent helper that documents how to install the `scip-rust` indexer
# pinned by `plugins/structural/scip/plugin.toml`.
#
# This script does NOT auto-install — it reports the current state and
# prints the recommended install command so operators choose how to
# install (cargo, cargo-binstall, or a direct release-binary download).
#
# Exit code is always 0; this is informational scaffolding per the
# WO-0044 install-helper convention (see `install-ast-grep.sh`,
# `install-probe.sh`).
set -euo pipefail

PINNED_VERSION="0.0.5"

if command -v scip-rust >/dev/null 2>&1; then
    found_version="$(scip-rust --version 2>/dev/null | awk '{print $NF}' || true)"
    if [[ -n "$found_version" ]]; then
        printf '[OK] scip-rust %s found at: %s (plugin.toml pins %s)\n' \
            "$found_version" "$(command -v scip-rust)" "$PINNED_VERSION"
    else
        printf '[OK] scip-rust found at: %s (plugin.toml pins %s)\n' \
            "$(command -v scip-rust)" "$PINNED_VERSION"
    fi
    exit 0
fi

printf '[MISSING] scip-rust is not on PATH.\n'
printf 'To install scip-rust %s pinned by plugins/structural/scip/plugin.toml:\n' \
    "$PINNED_VERSION"
printf '  cargo install --git https://github.com/sourcegraph/scip-rust --tag v%s scip-rust\n' \
    "$PINNED_VERSION"
printf '  # OR (faster pre-built binary install via cargo-binstall):\n'
printf '  cargo binstall scip-rust@%s\n' "$PINNED_VERSION"
printf '  # OR (direct release binary):\n'
printf '  # https://github.com/sourcegraph/scip-rust/releases/tag/v%s\n' \
    "$PINNED_VERSION"
printf '\n'
printf 'After install, verify with:\n'
printf '  scip-rust --version\n'
exit 0
