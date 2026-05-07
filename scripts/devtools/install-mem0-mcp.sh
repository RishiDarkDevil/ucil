#!/usr/bin/env bash
# Idempotent helper that documents how to install the mem0-mcp-server
# binary pinned by `plugins/knowledge/mem0/plugin.toml`.
#
# This script does NOT auto-install — it reports the current state and
# prints the recommended install command. Operators install manually.
#
# Exit code is always 0; this is informational scaffolding.
set -euo pipefail

PINNED_PYPI_VERSION="0.2.1"
OPTIONAL_SDK_VERSION="0.1.116"

if command -v mem0-mcp-server >/dev/null 2>&1; then
    found_version="$(mem0-mcp-server --version 2>/dev/null | awk '{print $NF}' || echo unknown)"
    printf '[OK] mem0-mcp-server binary at %s reports version: %s\n' \
        "$(command -v mem0-mcp-server)" "$found_version"
    printf '     plugin.toml pins pypi package mem0-mcp-server@%s.\n' \
        "$PINNED_PYPI_VERSION"
    printf '     If the binary version drifts from the pin, reinstall via:\n'
    printf '       uv tool install mem0-mcp-server==%s\n' "$PINNED_PYPI_VERSION"
    printf '       # OR\n'
    printf '       pipx install mem0-mcp-server==%s\n' "$PINNED_PYPI_VERSION"
elif command -v uvx >/dev/null 2>&1; then
    printf '[OK] uvx is on PATH (%s); plugin.toml launches the server via\n' \
        "$(command -v uvx)"
    printf '     `uvx mem0-mcp-server@%s` — no global install required.\n' \
        "$PINNED_PYPI_VERSION"
    printf '     First run resolves and downloads the pypi package + its\n'
    printf '     transitive deps (~70 packages, mostly mem0ai + mcp + openai)\n'
    printf '     into an isolated env; subsequent runs are cached.\n'
    printf '     For a system-wide install (faster cold start):\n'
    printf '       uv tool install mem0-mcp-server==%s\n' "$PINNED_PYPI_VERSION"
else
    printf '[MISSING] mem0-mcp-server is not on PATH and uvx is not on PATH.\n'
    printf 'To install mem0-mcp-server pinned by plugins/knowledge/mem0/plugin.toml:\n'
    printf '  uv tool install mem0-mcp-server==%s\n' "$PINNED_PYPI_VERSION"
    printf '  # OR (run via uvx without a system-wide install — slower first run)\n'
    printf '  alias mem0-mcp-server="uvx mem0-mcp-server@%s"\n' "$PINNED_PYPI_VERSION"
    printf '  # OR (pipx if uv is unavailable)\n'
    printf '  pipx install mem0-mcp-server==%s\n' "$PINNED_PYPI_VERSION"
    printf '\n'
    printf 'After install, verify with:\n'
    printf '  mem0-mcp-server --help\n'
fi

# Optional Python SDK install hint — master-plan §3.1 line 312 lists Mem0
# as "Python SDK + MCP server"; this manifest only declares the MCP
# transport, but the SDK is the lower-level alternative for direct
# Python embedding consumers.
printf '\n'
printf '[HINT] Optional Python SDK (mem0ai) — separate from the MCP server:\n'
printf '       The Mem0 Python SDK is the direct-import alternative to the\n'
printf '       MCP server transport. UCIL itself talks to mem0 only over\n'
printf '       MCP per the manifest above; the SDK is documented here for\n'
printf '       operators that want to script against Mem0 from Python:\n'
printf '         pip install mem0ai==%s\n' "$OPTIONAL_SDK_VERSION"
printf '         # OR\n'
printf '         uv pip install mem0ai==%s\n' "$OPTIONAL_SDK_VERSION"
printf '\n'
printf '[NOTE] mem0-mcp-server requires MEM0_API_KEY for tool invocations\n'
printf '       (semantic embedding + storage). The MCP `tools/list` round-trip\n'
printf '       works without the key — UCIL`s health-check therefore succeeds\n'
printf '       offline, but full F06 store/retrieve/list smoke requires:\n'
printf '         export MEM0_API_KEY=<your-key>\n'
printf '       Sign up at https://mem0.ai for a free-tier key.\n'
exit 0
