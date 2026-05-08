#!/usr/bin/env bash
# Idempotent helper that documents how to install / warm the
# Context7 MCP server pinned by `plugins/context/context7/plugin.toml`.
#
# This script does NOT auto-install — it reports the current state and
# prints the recommended install command. The optional warm-up tries
# `npx -y @upstash/context7-mcp@<pin> --help 2>&1 | head -1` so the npm
# tarball lands in the npx cache before the integration test pays the
# cold-cache cost; if `--help` is unsupported by the upstream binary
# that's not fatal — the warm-up exits gracefully and operators can
# rely on the spawn-time download in the integration-test path.
#
# Exit code is always 0; this is informational scaffolding.
set -euo pipefail

PINNED_NPM_VERSION="2.2.4"
PINNED_PKG_SPEC="@upstash/context7-mcp@${PINNED_NPM_VERSION}"

if ! command -v node >/dev/null 2>&1; then
    printf '[MISSING] node is not on PATH.\n'
    printf 'Install Node.js 20+ LTS first (e.g. via nvm: `nvm install 20`),\n'
    printf 'then re-run this script.\n'
    exit 0
fi

if ! command -v npm >/dev/null 2>&1; then
    printf '[MISSING] npm is not on PATH (it usually ships with node).\n'
    printf 'Reinstall Node.js 20+ LTS to recover npm.\n'
    exit 0
fi

if command -v context7-mcp >/dev/null 2>&1; then
    printf '[OK] context7-mcp binary at %s detected.\n' \
        "$(command -v context7-mcp)"
    printf '     plugin.toml pins npm package %s.\n' "$PINNED_PKG_SPEC"
    printf '     If the binary version drifts from the pin, reinstall via:\n'
    printf '       npm install -g %s\n' "$PINNED_PKG_SPEC"
fi

if command -v npx >/dev/null 2>&1; then
    printf '[OK] npx is on PATH (%s); plugin.toml launches the server via\n' \
        "$(command -v npx)"
    printf '     `npx -y %s` — no global install required.\n' "$PINNED_PKG_SPEC"
    printf '     First run downloads the package + transitive deps into the\n'
    printf '     npx cache; subsequent runs hit the cache and complete in\n'
    printf '     well under a second.\n'
    printf '     For a global install (faster cold start, no npx fetch):\n'
    printf '       npm install -g %s\n' "$PINNED_PKG_SPEC"

    # Best-effort warm-up — the upstream binary may not advertise
    # `--help`, in which case this exits non-zero quickly and we
    # ignore the error. The goal is a side-effect npm fetch into the
    # cache, NOT a successful help dump.
    printf '[INFO] warming npx cache for %s ...\n' "$PINNED_PKG_SPEC"
    if npx -y "$PINNED_PKG_SPEC" --help >/dev/null 2>&1; then
        printf '[INFO] warm-up emitted --help; cache primed.\n'
    else
        printf '[INFO] --help unsupported by upstream binary; warm-up exited\n'
        printf '       gracefully — the npm fetch still primed the cache as a\n'
        printf '       side-effect of npx resolving the package spec.\n'
    fi
else
    printf '[MISSING] npx is not on PATH despite npm being present.\n'
    printf 'Reinstall Node.js 20+ LTS to recover npx, OR install globally:\n'
    printf '  npm install -g %s\n' "$PINNED_PKG_SPEC"
fi

printf '\n'
printf '[NOTE] Context7 is the live-library-docs lookup half of the G5\n'
printf '       (Context) source group. It reaches context7.com over HTTPS\n'
printf '       for free-tier reads; no API key is required for tools/list\n'
printf '       round-trips or default-rate-limited tool invocations.\n'
printf '       Optional CONTEXT7_API_KEY env var raises the upstream rate\n'
printf '       limit but is operator-state and out of scope for this manifest.\n'
exit 0
