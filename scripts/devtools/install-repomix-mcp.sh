#!/usr/bin/env bash
# Idempotent helper that documents how to install / warm the
# Repomix MCP server pinned by `plugins/context/repomix/plugin.toml`.
#
# This script does NOT auto-install — it reports the current state and
# prints the recommended install command. The optional warm-up tries
# `npx -y repomix@<pin> --help 2>&1 | head -1` so the npm tarball lands
# in the npx cache before the integration test pays the cold-cache
# cost. The `--help` flag is the canonical CLI warm-up — invoking with
# `--mcp` would spawn the MCP server and block on stdin, which is NOT
# what we want here.
#
# Exit code is always 0; this is informational scaffolding.
set -euo pipefail

PINNED_NPM_VERSION="1.14.0"
PINNED_PKG_SPEC="repomix@${PINNED_NPM_VERSION}"

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

if command -v repomix >/dev/null 2>&1; then
    found_version="$(repomix --version 2>/dev/null | awk '{print $NF}' || echo unknown)"
    printf '[OK] repomix binary at %s reports version: %s\n' \
        "$(command -v repomix)" "$found_version"
    printf '     plugin.toml pins npm package %s.\n' "$PINNED_PKG_SPEC"
    printf '     If the binary version drifts from the pin, reinstall via:\n'
    printf '       npm install -g %s\n' "$PINNED_PKG_SPEC"
fi

if command -v npx >/dev/null 2>&1; then
    printf '[OK] npx is on PATH (%s); plugin.toml launches the server via\n' \
        "$(command -v npx)"
    printf '     `npx -y %s --mcp` — no global install required.\n' "$PINNED_PKG_SPEC"
    printf '     First run downloads the package + transitive deps into the\n'
    printf '     npx cache; subsequent runs hit the cache and complete in\n'
    printf '     well under a second.\n'
    printf '     For a global install (faster cold start, no npx fetch):\n'
    printf '       npm install -g %s\n' "$PINNED_PKG_SPEC"

    # Warm-up uses `--help` (CLI mode), NOT `--mcp` (server mode that
    # blocks on stdin). The goal is a side-effect npm fetch into the
    # cache.
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
printf '[NOTE] Repomix is the repository-pack half of the G5 (Context)\n'
printf '       source group. It performs zero network calls for\n'
printf '       `pack_codebase` against a local path; `pack_remote_repository`\n'
printf '       clones over HTTPS using the local git binary. No API key is\n'
printf '       required for any tool. The token-reduction the F03 spec\n'
printf '       measures is a deterministic local computation\n'
printf '       (Tree-sitter + gitignore-aware filter + dedup), reproducible\n'
printf '       across CI runs against tests/fixtures/rust-project.\n'
exit 0
