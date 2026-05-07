#!/usr/bin/env bash
# Idempotent helper that documents how to install the codebase-memory-mcp
# binary pinned by `plugins/knowledge/codebase-memory/plugin.toml`.
#
# This script does NOT auto-install — it reports the current state and
# prints the recommended install command. Operators install manually.
#
# Exit code is always 0; this is informational scaffolding.
set -euo pipefail

PINNED_NPM_VERSION="0.6.1"

if command -v codebase-memory-mcp >/dev/null 2>&1; then
    found_version="$(codebase-memory-mcp --version 2>/dev/null | awk '{print $NF}' || echo unknown)"
    printf '[OK] codebase-memory-mcp binary at %s reports version: %s\n' \
        "$(command -v codebase-memory-mcp)" "$found_version"
    printf '     plugin.toml pins npm package codebase-memory-mcp@%s.\n' \
        "$PINNED_NPM_VERSION"
    printf '     If the binary version drifts from the pin, reinstall via:\n'
    printf '       npm install -g codebase-memory-mcp@%s\n' "$PINNED_NPM_VERSION"
    exit 0
fi

if command -v npx >/dev/null 2>&1; then
    printf '[OK] npx is on PATH (%s); plugin.toml launches the server via\n' \
        "$(command -v npx)"
    printf '     `npx -y codebase-memory-mcp@%s` — no global install required.\n' \
        "$PINNED_NPM_VERSION"
    printf '     First run downloads the package (~12 KB) into the npx cache;\n'
    printf '     subsequent runs hit the cache and complete in well under a second.\n'
    printf '     For a global install (faster cold start, no npx fetch):\n'
    printf '       npm install -g codebase-memory-mcp@%s\n' "$PINNED_NPM_VERSION"
    exit 0
fi

printf '[MISSING] codebase-memory-mcp is not on PATH and npx is not on PATH.\n'
printf 'To install codebase-memory-mcp pinned by plugins/knowledge/codebase-memory/plugin.toml:\n'
printf '  npm install -g codebase-memory-mcp@%s\n' "$PINNED_NPM_VERSION"
printf '  # OR (run via npx without a global install — slower first run)\n'
printf '  alias codebase-memory-mcp="npx -y codebase-memory-mcp@%s"\n' "$PINNED_NPM_VERSION"
printf '  # OR (upstream curl|bash — installs a single static binary)\n'
printf '  curl -fsSL https://raw.githubusercontent.com/DeusData/codebase-memory-mcp/main/install.sh | bash\n'
printf '\n'
printf 'After install, verify with:\n'
printf '  codebase-memory-mcp --version\n'
printf '\n'
printf 'No API keys are required — codebase-memory-mcp processes the\n'
printf 'knowledge graph entirely locally. Optional environment variables:\n'
printf '  CBM_CACHE_DIR        Override database storage location\n'
printf '                       (defaults to ~/.cache/codebase-memory-mcp)\n'
printf '  CBM_DIAGNOSTICS      Enable diagnostics output\n'
exit 0
