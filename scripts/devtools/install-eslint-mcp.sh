#!/usr/bin/env bash
# Idempotent helper that documents how to install / warm the
# ESLint MCP server pinned by `plugins/quality/eslint/plugin.toml`.
#
# This script does NOT auto-install — it reports the current state and
# prints the recommended install command. The optional warm-up tries
# `npx -y @eslint/mcp@<pin> --help 2>&1 | head -1` so the npm tarball
# lands in the npx cache before the integration test pays the cold-
# cache cost; if `--help` is unsupported by the upstream binary the
# warm-up exits gracefully — the npm fetch still primes the cache as
# a side-effect of npx resolving the package spec.
#
# WO-0074 §executor #2 lesson applied: do NOT use `--mcp` as a
# warm-up flag — the ESLint MCP server IS an MCP server by default
# (no `--mcp` flag exists) and invoking the binary in MCP-server mode
# would block on stdin instead of warming the cache.
#
# Exit code is always 0; this is informational scaffolding.
set -euo pipefail

PINNED_NPM_VERSION="0.3.5"
PINNED_PKG_SPEC="@eslint/mcp@${PINNED_NPM_VERSION}"

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
    # cache, NOT a successful help dump. WO-0074 §executor #2:
    # do NOT use `--mcp` for warm-up; the binary IS an MCP server
    # by default and would block on stdin.
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
printf '[NOTE] ESLint MCP is the JS/TS-lint half of the G7 (Quality)\n'
printf '       source group. It runs entirely locally — no network\n'
printf '       calls, no auth required. The upstream binary advertises\n'
printf '       a single tool `lint-files` (kebab-case as emitted by\n'
printf '       tools/list) that lints absolute file paths against the\n'
printf '       project-local ESLint config (eslint.config.js or\n'
printf '       legacy .eslintrc.* — operator-state). The integration\n'
printf '       test + verify script handle cwd themselves: each copies\n'
printf '       tests/fixtures/typescript-project into a `mktemp -d`\n'
printf '       tmpdir BEFORE invoking the upstream binary, optionally\n'
printf '       fabricates a minimal eslint.config.js in the tmpdir, and\n'
printf '       passes absolute file paths via the `filePaths` argument.\n'
printf '       This keeps the read-only fixture tree pristine even if\n'
printf '       upstream writes side-files (e.g., .eslintcache) into the\n'
printf '       working directory.\n'
exit 0
