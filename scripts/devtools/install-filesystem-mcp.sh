#!/usr/bin/env bash
# Idempotent helper that documents how to install / warm the
# Filesystem MCP server pinned by `plugins/platform/filesystem/plugin.toml`.
#
# This script does NOT auto-install — it reports the current state and
# prints the recommended install command. The optional warm-up tries
# `npx -y @modelcontextprotocol/server-filesystem@<pin> --help 2>&1 | head -1`
# so the npm tarball lands in the npx cache before the integration
# test pays the cold-cache cost; if `--help` is unsupported by the
# upstream binary the warm-up exits gracefully — the npm fetch still
# primes the cache as a side-effect of npx resolving the package spec.
#
# WO-0074 §executor #2 lesson applied: do NOT use `--mcp` as a
# warm-up flag — Filesystem MCP IS an MCP server by default
# (no `--mcp` flag exists) and invoking the binary in MCP-server
# mode would block on stdin instead of warming the cache.
#
# `${UCIL_FS_MCP_ALLOWED_PATH}` placeholder substitution: the on-disk
# manifest at plugins/platform/filesystem/plugin.toml carries a
# sentinel `${UCIL_FS_MCP_ALLOWED_PATH}` token in transport.args[2]
# that consumers (integration test + verify script) substitute
# per-invocation with an absolute path to a freshly-fabricated
# mktemp -d directory (NOT a fixture copy). The warm-up here uses
# `--help` and does NOT supply an allowed-path positional arg, so
# this placeholder is irrelevant to the warm-up step (the upstream
# binary's --help exits before parsing positional args).
#
# READ-ONLY-IN-TESTS INVARIANT: the integration test and verify
# script exercise ONLY the read-side surfaces (read_file,
# list_directory). The upstream catalog advertises write_file /
# edit_file / move_file / create_directory but those are out of
# scope for F07 acceptance. Any future write-side coverage requires
# a new feature ID + WO.
#
# Exit code is always 0; this is informational scaffolding.
set -euo pipefail

PINNED_NPM_VERSION="2026.1.14"
PINNED_PKG_SPEC="@modelcontextprotocol/server-filesystem@${PINNED_NPM_VERSION}"

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

if command -v mcp-server-filesystem >/dev/null 2>&1; then
    printf '[OK] mcp-server-filesystem binary at %s detected.\n' \
        "$(command -v mcp-server-filesystem)"
    printf '     plugin.toml pins npm package %s.\n' "$PINNED_PKG_SPEC"
    printf '     If the binary version drifts from the pin, reinstall via:\n'
    printf '       npm install -g %s\n' "$PINNED_PKG_SPEC"
fi

if command -v npx >/dev/null 2>&1; then
    printf '[OK] npx is on PATH (%s); plugin.toml launches the server via\n' \
        "$(command -v npx)"
    printf '     `npx -y %s <allowed-path>` — no global install required.\n' "$PINNED_PKG_SPEC"
    printf '     First run downloads the package + transitive deps into the\n'
    printf '     npx cache; subsequent runs hit the cache and complete in\n'
    printf '     well under a second.\n'
    printf '     For a global install (faster cold start, no npx fetch):\n'
    printf '       npm install -g %s\n' "$PINNED_PKG_SPEC"

    # Best-effort warm-up via `--help` (NOT --mcp / no positional path
    # — the upstream binary's --help exits before parsing positional
    # allowed-path args, so we can warm without supplying one). The
    # goal is a side-effect npm fetch into the cache.
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
printf '[NOTE] Filesystem MCP is the local-IO half of the G6 (Platform)\n'
printf '       source group. It performs ZERO network calls — every\n'
printf '       tool operates against the local filesystem within the\n'
printf '       allow-list paths supplied as positional args at spawn\n'
printf '       time. No API key required.\n'
printf '       The `${UCIL_FS_MCP_ALLOWED_PATH}` placeholder in the\n'
printf '       manifest transport.args is substituted per-invocation\n'
printf '       by the integration test (with a freshly-fabricated\n'
printf '       mktemp -d tmpdir) and the verify script (same pattern,\n'
printf '       NOT a fixture copy — the tmpdir is populated from\n'
printf '       scratch with known small text files for hermeticity).\n'
printf '       Substitution is the consumer'\''s responsibility;\n'
printf '       PluginManager passes args verbatim.\n'
printf '       READ-ONLY-IN-TESTS: the test smoke exercises ONLY\n'
printf '       read_file + list_directory; write_file / edit_file /\n'
printf '       move_file are out of scope for F07 acceptance.\n'
exit 0
