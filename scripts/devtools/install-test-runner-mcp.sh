#!/usr/bin/env bash
# Idempotent helper that documents how to install / warm the
# test-runner MCP server pinned by
# `plugins/testing/test-runner/plugin.toml` (the third-party
# scoped npm mirror `@iflow-mcp/mcp-test-runner@0.2.1` per
# DEC-0025 — bare `test-runner-mcp` is NOT on npm; see DEC-0025
# §Context Error 1 for the full chain-of-corrections narrative).
#
# This script does NOT auto-install — it reports the current state
# and prints the recommended install command. The optional warm-up
# tries `npx -y @iflow-mcp/mcp-test-runner@<pin> --help 2>&1 | head -1`
# so the npm tarball lands in the npx cache before the integration
# test pays the cold-cache cost; if `--help` is unsupported by the
# upstream binary the warm-up exits gracefully — the npm fetch still
# primes the cache as a side-effect of npx resolving the package
# spec.
#
# WO-0074 §executor #2 lesson applied: do NOT use `--mcp` as a
# warm-up flag — the test-runner MCP server IS an MCP server by
# default and invoking the binary in MCP-server mode would block on
# stdin instead of warming the cache.
#
# test-runner does NOT have an external Node-side CLI dep beyond
# Node + npm itself (the upstream package declares
# @modelcontextprotocol/sdk + transitive deps in its package.json;
# npx provisions a fresh module tree that auto-resolves everything
# from the npm registry). The wrapper INTERNALLY shells out to
# framework-specific test runner CLIs (cargo / pytest / vitest / go
# / bats / flutter / jest) at tool-call time only — those are
# SECONDARY operator-state CLI deps and are NOT initialize-time
# requirements. This install script does NOT verify their presence
# (per DEC-0025 §Decision point 2 + WO-0082 scope_in #6 + WO-0082
# scope_in #34: test-runner-mcp is SELF-RESOLVING at the MCP-server
# level).
#
# Exit code is always 0; this is informational scaffolding.
set -euo pipefail

PINNED_TEST_RUNNER_MCP_VERSION="0.2.1"
PINNED_TEST_RUNNER_MCP_PACKAGE="@iflow-mcp/mcp-test-runner"
PINNED_PKG_SPEC="${PINNED_TEST_RUNNER_MCP_PACKAGE}@${PINNED_TEST_RUNNER_MCP_VERSION}"

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
printf '[NOTE] test-runner is the multi-language unified-execution\n'
printf '       half of the G8 (Testing+CI) source group per master-\n'
printf '       plan §4.8 line 405 + §5.8 (the pytest-specialised half\n'
printf '       is mcp-pytest-runner from F08). It runs entirely\n'
printf '       locally — no network calls, no auth required. The\n'
printf '       upstream binary advertises a single tool `run_tests`\n'
printf '       (snake_case as emitted by tools/list) with a framework\n'
printf '       enum (7 values: bats, pytest, flutter, jest, go, rust,\n'
printf '       generic) — framework selection happens via the\n'
printf '       `framework` input arg at tool-call time, NOT via\n'
printf '       multiple per-framework tools.\n'
printf '\n'
printf '       The wrapper INTERNALLY shells out to the framework-\n'
printf '       specific test runner CLI (cargo / pytest / vitest /\n'
printf '       go / bats / flutter / jest) at tool-call time only —\n'
printf '       those are SECONDARY operator-state deps. The cargo-\n'
printf '       test (initialize-time tools/list handshake) requires\n'
printf '       NONE of them. The verify script optionally exercises\n'
printf '       a tools/call run_tests against a workspace crate but\n'
printf '       gates that on `cargo --version` to keep the test-\n'
printf '       runner-mcp manifest itself self-resolving at the\n'
printf '       MCP-server level (per DEC-0025 §Decision point 2 +\n'
printf '       WO-0082 scope_in #6).\n'
printf '\n'
printf '       Provenance: bare `test-runner-mcp` is NOT on npm\n'
printf '       (404); the only published surface is the third-party\n'
printf '       scoped mirror @iflow-mcp/mcp-test-runner published\n'
printf '       2026-01-08 by chatflowdev. The upstream privsim/\n'
printf '       mcp-test-runner GitHub repo has never published to\n'
printf '       npm under their own name. See DEC-0025 §Context for\n'
printf '       the full chain-of-corrections narrative (DEC-0021\n'
printf '       deferral → DEC-0024 revival → DEC-0025 corrected\n'
printf '       source data).\n'
exit 0
