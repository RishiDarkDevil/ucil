#!/usr/bin/env bash
# Idempotent helper that documents how to install the codegraphcontext
# binary pinned by `plugins/architecture/codegraphcontext/plugin.toml`.
#
# This script does NOT auto-install — it reports the current state and
# prints the recommended install command. Operators install manually.
#
# Exit code is always 0; this is informational scaffolding.
set -euo pipefail

PINNED_PYPI_VERSION="0.4.7"
PINNED_FALKORDBLITE_DEP="falkordblite"

if command -v codegraphcontext >/dev/null 2>&1; then
    found_version="$(codegraphcontext --version 2>/dev/null | awk '{print $NF}' || echo unknown)"
    printf '[OK] codegraphcontext binary at %s reports version: %s\n' \
        "$(command -v codegraphcontext)" "$found_version"
    printf '     plugin.toml pins pypi package codegraphcontext@%s.\n' \
        "$PINNED_PYPI_VERSION"
    printf '     If the binary version drifts from the pin, reinstall via:\n'
    printf '       uv tool install codegraphcontext==%s --with %s\n' \
        "$PINNED_PYPI_VERSION" "$PINNED_FALKORDBLITE_DEP"
    printf '       # OR\n'
    printf '       pipx install codegraphcontext==%s\n' "$PINNED_PYPI_VERSION"
    printf '       pipx inject codegraphcontext %s\n' "$PINNED_FALKORDBLITE_DEP"
elif command -v uvx >/dev/null 2>&1; then
    printf '[OK] uvx is on PATH (%s); plugin.toml launches the server via\n' \
        "$(command -v uvx)"
    printf '     `uvx --with %s codegraphcontext@%s mcp start` —\n' \
        "$PINNED_FALKORDBLITE_DEP" "$PINNED_PYPI_VERSION"
    printf '     no global install required.\n'
    printf '     First run resolves and downloads the pypi package + its\n'
    printf '     transitive deps (codegraphcontext + falkordblite + mcp +\n'
    printf '     graph-tools dependencies) into an isolated env;\n'
    printf '     subsequent runs are cached.\n'
    printf '     For a system-wide install (faster cold start):\n'
    printf '       uv tool install codegraphcontext==%s --with %s\n' \
        "$PINNED_PYPI_VERSION" "$PINNED_FALKORDBLITE_DEP"
else
    printf '[MISSING] codegraphcontext is not on PATH and uvx is not on PATH.\n'
    printf 'To install codegraphcontext pinned by plugins/architecture/codegraphcontext/plugin.toml:\n'
    printf '  uv tool install codegraphcontext==%s --with %s\n' \
        "$PINNED_PYPI_VERSION" "$PINNED_FALKORDBLITE_DEP"
    printf '  # OR (run via uvx without a system-wide install — slower first run)\n'
    printf '  alias codegraphcontext="uvx --with %s codegraphcontext@%s"\n' \
        "$PINNED_FALKORDBLITE_DEP" "$PINNED_PYPI_VERSION"
    printf '  # OR (pipx if uv is unavailable)\n'
    printf '  pipx install codegraphcontext==%s\n' "$PINNED_PYPI_VERSION"
    printf '  pipx inject codegraphcontext %s\n' "$PINNED_FALKORDBLITE_DEP"
    printf '\n'
    printf 'After install, verify with:\n'
    printf '  codegraphcontext --help\n'
fi

printf '\n'
printf '[NOTE] codegraphcontext requires FalkorDB-Lite (embedded mode) — the\n'
printf '       --with %s declaration in the manifest plugins/architecture/\n' "$PINNED_FALKORDBLITE_DEP"
printf '       codegraphcontext/plugin.toml ensures the dep is available in\n'
printf '       the uvx-isolated env. No external Redis/FalkorDB container\n'
printf '       is required; tools/list runs entirely local. The first invocation\n'
printf '       creates ~/.codegraphcontext/.env on disk for operator-scoped\n'
printf '       configuration; this is operator-state and not staged for commit.\n'
exit 0
