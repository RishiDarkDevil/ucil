#!/usr/bin/env bash
# Idempotent helper that documents how to install the probe binary pinned
# by `plugins/search/probe/plugin.toml`.
#
# This script does NOT auto-install — it reports the current state and
# prints the recommended install command. Operators install manually.
#
# Exit code is always 0; this is informational scaffolding.
set -euo pipefail

PINNED_NPM_VERSION="0.6.0-rc315"

if command -v probe >/dev/null 2>&1; then
    # `probe --version` prints `probe-code 0.6.0` (the binary version);
    # the pinned tag is the npm package version. Both are surfaced so the
    # operator can correlate the two.
    found_version="$(probe --version 2>/dev/null | awk '{print $NF}')"
    printf '[OK] probe binary at %s reports version: %s\n' \
        "$(command -v probe)" "$found_version"
    printf '     plugin.toml pins npm package @probelabs/probe@%s.\n' \
        "$PINNED_NPM_VERSION"
    printf '     If the binary version drifts from the pin, reinstall via:\n'
    printf '       npm install -g @probelabs/probe@%s\n' "$PINNED_NPM_VERSION"
    exit 0
fi

printf '[MISSING] probe is not on PATH.\n'
printf 'To install probe pinned by plugins/search/probe/plugin.toml (npm @probelabs/probe@%s):\n' \
    "$PINNED_NPM_VERSION"
printf '  npm install -g @probelabs/probe@%s\n' "$PINNED_NPM_VERSION"
printf '  # OR (run via npx without a global install — slower first run)\n'
printf '  alias probe="npx -y @probelabs/probe@%s"\n' "$PINNED_NPM_VERSION"
printf '\n'
printf 'After install, verify with:\n'
printf '  probe --version   # expected: probe-code 0.6.0\n'
exit 0
