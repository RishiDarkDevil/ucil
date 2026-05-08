#!/usr/bin/env bash
# Idempotent helper that documents how to launch the Graphiti MCP
# server pinned by `plugins/knowledge/graphiti/plugin.toml`.
#
# This script does NOT auto-install — it reports the current state and
# prints the recommended invocation commands. Operators install the
# upstream graph DB (FalkorDB or Neo4j) and the LLM API key separately
# per the manifest's operator-state requirements block.
#
# Exit code is always 0; this is informational scaffolding.
set -euo pipefail

PINNED_GRAPHITI_TAG="mcp-v1.0.2"
PINNED_GRAPHITI_COMMIT_SHA="19e44a97a929ebf121294f97f26966f0379d8e30"
PINNED_GIT_REF="git+https://github.com/getzep/graphiti.git@${PINNED_GRAPHITI_TAG}#subdirectory=mcp_server"

if command -v uvx >/dev/null 2>&1; then
    printf '[OK] uvx is on PATH (%s); plugin.toml launches the server via\n' \
        "$(command -v uvx)"
    printf '     `uvx --from "%s" \\\n' "${PINNED_GIT_REF}"
    printf '            python -m graphiti_mcp_server --transport stdio` —\n'
    printf '     no global install required.\n'
    printf '     First run resolves the git checkout at SHA %s\n' \
        "${PINNED_GRAPHITI_COMMIT_SHA}"
    printf '     into an ephemeral isolated env (~50 transitive Python\n'
    printf '     packages: graphiti-core, mcp, openai, pydantic, falkordb, etc.);\n'
    printf '     subsequent runs are cached.\n'
    printf '     Cache warm-up (informational; not strictly required):\n'
    printf '       uvx --from "%s" python -m graphiti_mcp_server --help\n' \
        "${PINNED_GIT_REF}"
else
    printf '[MISSING] uvx is not on PATH.\n'
    printf 'To install the Astral uv tool-runner (which provides `uvx`):\n'
    printf '  curl -LsSf https://astral.sh/uv/install.sh | sh\n'
    printf '\n'
    printf 'After uvx is on PATH, the plugin.toml-pinned launch command is:\n'
    printf '  uvx --from "%s" \\\n' "${PINNED_GIT_REF}"
    printf '        python -m graphiti_mcp_server --transport stdio\n'
    printf '\n'
    printf 'Pinned upstream commit: %s\n' "${PINNED_GRAPHITI_COMMIT_SHA}"
    printf '  https://github.com/getzep/graphiti/commit/%s\n' \
        "${PINNED_GRAPHITI_COMMIT_SHA}"
fi

# Operator-state requirements: graphiti is DUAL-DEPENDENT.
printf '\n'
printf '[NOTE] graphiti requires BOTH a graph DB connection AND an LLM API key:\n'
printf '\n'
printf '       1. Graph DB (one of):\n'
printf '          - FalkorDB (default): redis://127.0.0.1:6379\n'
printf '            export FALKORDB_URI=redis://127.0.0.1:6379\n'
printf '            # Start FalkorDB:\n'
printf '            docker run -p 6379:6379 falkordb/falkordb\n'
printf '          - Neo4j (alternative): bolt://127.0.0.1:7687\n'
printf '            export NEO4J_URI=bolt://127.0.0.1:7687\n'
printf '            export NEO4J_USER=neo4j\n'
printf '            export NEO4J_PASSWORD=<your-password>\n'
printf '\n'
printf '       2. LLM API key (one of, by --llm-provider):\n'
printf '          - OpenAI (default): export OPENAI_API_KEY=<your-key>\n'
printf '          - Anthropic:        export ANTHROPIC_API_KEY=<your-key>\n'
printf '          - Groq:             export GROQ_API_KEY=<your-key>\n'
printf '          - Gemini:           export GEMINI_API_KEY=<your-key>\n'
printf '\n'
printf '       The MCP `tools/list` round-trip succeeds with a placeholder\n'
printf '       LLM key (the LLM client is lazily instantiated); tool\n'
printf '       invocation requires a live key. The graph DB connection,\n'
printf '       however, is eagerly probed at startup — `tools/list` blocks\n'
printf '       until FalkorDB/Neo4j is reachable.\n'
printf '\n'
printf '[NOTE] Secondary docker install path (DOCUMENTED ONLY — NOT wired by\n'
printf '       plugins/knowledge/graphiti/plugin.toml; UCIL has zero\n'
printf '       docker-installed plugins; per DEC-0022 + WO-0079 scope_in #2):\n'
printf '         docker pull zepai/knowledge-graph-mcp:1.0.2\n'
printf '         docker run -i --rm zepai/knowledge-graph-mcp:1.0.2\n'
printf '       Tags: 1.0.2-graphiti-0.28.2, 1.0.2, 1.0.2-standalone, latest.\n'

exit 0
