#!/usr/bin/env bash
# serena-live.sh — Live handshake against the real Serena MCP server.
#
# Usage: scripts/verify/serena-live.sh
#
# Purpose:
#   Phase-1 claims Serena is "wired into G1 structural group"
#   (P1-W5-F02). That claim is a lie if Serena itself doesn't respond
#   over MCP in the project's actual runtime environment. This script
#   spawns the real Serena MCP server via uvx (pinned to v1.0.0 per
#   plugins/structural/serena/plugin.toml) and verifies it answers the
#   MCP handshake and advertises at least the core Serena tools
#   (find_symbol, find_referencing_symbols, get_symbols_overview).
#
# No mocks, no docker — Phase 1 runs Serena locally via uvx as declared
# in the plugin manifest (master-plan §13). Docker harness lands when
# a later phase needs heavier services.
#
# Exit codes:
#   0 — Serena spawned and returned a valid tools/list.
#   1 — handshake failed / required tools missing / Serena crashed.
#   2 — prereq missing (uvx not installed).
set -uo pipefail

cd "$(git rev-parse --show-toplevel)"

REQUIRED_TOOLS=(find_symbol find_referencing_symbols get_symbols_overview)

if ! command -v uvx >/dev/null 2>&1; then
  echo "[serena-live] FAIL: uvx not on PATH (install via: curl -LsSf https://astral.sh/uv/install.sh | sh)" >&2
  exit 2
fi

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

SERENA_CMD=(
  uvx --from "git+https://github.com/oraios/serena@v1.0.0"
  serena-mcp-server
  --context ide-assistant
  --project "$PWD"
)

REQ_INIT='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"serena-live","version":"1.0.0"}}}'
REQ_INITIALIZED='{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}'
REQ_LIST='{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}'

echo "[serena-live] spawning Serena via uvx (pinned v1.0.0)..."
{
  printf '%s\n' "$REQ_INIT"
  sleep 1
  printf '%s\n' "$REQ_INITIALIZED"
  printf '%s\n' "$REQ_LIST"
  sleep 2
} | timeout 90 "${SERENA_CMD[@]}" > "$TMP/out.jsonl" 2>"$TMP/err.log"
rc=$?
if [[ $rc -ne 0 && $rc -ne 124 ]]; then
  echo "[serena-live] FAIL: Serena exited with $rc" >&2
  echo "-- stderr (last 30 lines) --" >&2
  tail -30 "$TMP/err.log" >&2
  exit 1
fi

if [[ ! -s "$TMP/out.jsonl" ]]; then
  echo "[serena-live] FAIL: Serena produced no MCP responses" >&2
  echo "-- stderr (last 30 lines) --" >&2
  tail -30 "$TMP/err.log" >&2
  exit 1
fi

LIST_RESP=$(jq -c 'select(.id == 2)' < "$TMP/out.jsonl" 2>/dev/null | head -1)
if [[ -z "$LIST_RESP" ]]; then
  echo "[serena-live] FAIL: no response to tools/list (id=2)" >&2
  echo "-- stdout (last 30 lines) --" >&2
  tail -30 "$TMP/out.jsonl" >&2
  exit 1
fi

# Serena advertises many tools; we only require the three that feed G1 structural.
MISSING=()
for t in "${REQUIRED_TOOLS[@]}"; do
  if ! jq -e --arg n "$t" '.result.tools[] | select(.name == $n)' <<<"$LIST_RESP" >/dev/null 2>&1; then
    MISSING+=("$t")
  fi
done
if (( ${#MISSING[@]} > 0 )); then
  echo "[serena-live] FAIL: Serena missing required tools: ${MISSING[*]}" >&2
  echo "-- advertised tools --" >&2
  jq -r '.result.tools[] | .name' <<<"$LIST_RESP" | sort >&2
  exit 1
fi

TOTAL=$(jq '.result.tools | length' <<<"$LIST_RESP")
echo "[serena-live] OK — Serena v1.0.0 alive, advertises $TOTAL tools including ${REQUIRED_TOOLS[*]}."
exit 0
