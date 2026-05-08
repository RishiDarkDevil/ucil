#!/usr/bin/env bash
# Idempotent helper that documents how to install / warm the
# Git MCP server pinned by `plugins/platform/git/plugin.toml`.
#
# This script does NOT auto-install — it reports the current state and
# prints the recommended install command. The optional warm-up tries
# `uvx mcp-server-git@<pin> --help 2>&1 | head -1` so the pypi tarball
# lands in the uv cache before the integration test pays the cold-cache
# cost; if `--help` is unsupported by the upstream binary the warm-up
# exits gracefully — the uv fetch still primes the cache as a side-
# effect of uvx resolving the package spec.
#
# Unlike install-github-mcp.sh / install-filesystem-mcp.sh, this script
# checks for python3 + uv (or uvx) — Git MCP is pypi-distributed, NOT
# npm-distributed. If `uv` / `uvx` is missing the script emits a
# `[MISSING]` notice and exits 0 gracefully (operator-state — does
# not auto-install uv).
#
# WO-0074 §executor #2 lesson applied: do NOT use `--mcp` as a
# warm-up flag — Git MCP IS an MCP server by default and would
# block on stdin. `--help` is the canonical CLI warm-up.
#
# `${UCIL_GIT_MCP_REPO}` placeholder substitution: the on-disk
# manifest at plugins/platform/git/plugin.toml carries a sentinel
# `${UCIL_GIT_MCP_REPO}` token in transport.args[2] that consumers
# (integration test + verify script) substitute per-invocation with
# an absolute path to a local git repository. The warm-up here
# uses `--help` and does NOT supply --repository, so this placeholder
# is irrelevant to the warm-up step (the upstream binary's --help
# exits before parsing --repository). Read-only-in-tests invariant
# applies to the test paths, not this warm-up.
#
# Exit code is always 0; this is informational scaffolding.
set -euo pipefail

PINNED_PYPI_VERSION="2026.1.14"
PINNED_PKG_SPEC="mcp-server-git@${PINNED_PYPI_VERSION}"

if ! command -v python3 >/dev/null 2>&1; then
    printf '[MISSING] python3 is not on PATH.\n'
    printf 'Install Python 3.11+ first (e.g. via your distro package\n'
    printf 'manager or pyenv: `pyenv install 3.13.0`), then re-run\n'
    printf 'this script.\n'
    exit 0
fi

if ! command -v uvx >/dev/null 2>&1; then
    printf '[MISSING] uvx is not on PATH (it ships with `uv`).\n'
    printf 'Install uv first via the official installer:\n'
    printf '  curl -LsSf https://astral.sh/uv/install.sh | sh\n'
    printf 'Or via pip: `pip install --user uv`\n'
    printf 'See https://docs.astral.sh/uv/getting-started/installation/\n'
    printf 'for additional install options. After install, re-run this\n'
    printf 'script to warm the uv cache for %s.\n' "$PINNED_PKG_SPEC"
    exit 0
fi

if command -v mcp-server-git >/dev/null 2>&1; then
    found_version="$(mcp-server-git --version 2>/dev/null | awk '{print $NF}' || echo unknown)"
    printf '[OK] mcp-server-git binary at %s reports version: %s\n' \
        "$(command -v mcp-server-git)" "$found_version"
    printf '     plugin.toml pins pypi package %s.\n' "$PINNED_PKG_SPEC"
    printf '     If the binary version drifts from the pin, reinstall via:\n'
    printf '       uv tool install %s\n' "$PINNED_PKG_SPEC"
fi

printf '[OK] uvx is on PATH (%s); plugin.toml launches the server via\n' \
    "$(command -v uvx)"
printf '     `uvx %s --repository <repo-path>` — no global install required.\n' "$PINNED_PKG_SPEC"
printf '     First run downloads the package + transitive deps into the\n'
printf '     uv cache; subsequent runs hit the cache and complete in\n'
printf '     well under a second.\n'
printf '     For a global install (faster cold start, no uvx fetch):\n'
printf '       uv tool install %s\n' "$PINNED_PKG_SPEC"

# Best-effort warm-up via `--help` (NOT --mcp / no --repository — the
# upstream binary's --help exits before parsing --repository, so we
# can warm without supplying a real repo path). The goal is a
# side-effect uv fetch into the cache.
printf '[INFO] warming uvx cache for %s ...\n' "$PINNED_PKG_SPEC"
if uvx "$PINNED_PKG_SPEC" --help >/dev/null 2>&1; then
    printf '[INFO] warm-up emitted --help; cache primed.\n'
else
    printf '[INFO] --help unsupported by upstream binary; warm-up exited\n'
    printf '       gracefully — the uv fetch still primed the cache as a\n'
    printf '       side-effect of uvx resolving the package spec.\n'
fi

printf '\n'
printf '[NOTE] Git MCP is the local-VCS half of the G6 (Platform)\n'
printf '       source group. It performs ZERO network calls — every\n'
printf '       tool operates against the local git repository specified\n'
printf '       via `--repository <path>`. No API key required.\n'
printf '       The `${UCIL_GIT_MCP_REPO}` placeholder in the manifest\n'
printf '       transport.args is substituted per-invocation by the\n'
printf '       integration test (with a tmpdir copy of\n'
printf '       tests/fixtures/rust-project) and the verify script\n'
printf '       (same pattern). Substitution is the consumer'\''s\n'
printf '       responsibility; PluginManager passes args verbatim.\n'
exit 0
