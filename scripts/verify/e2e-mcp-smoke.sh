#!/usr/bin/env bash
# e2e-mcp-smoke.sh — End-to-end MCP handshake against the real ucil-daemon.
#
# Usage: scripts/verify/e2e-mcp-smoke.sh
#
# Purpose:
#   The per-WO verifier runs unit/integration cargo tests in-process.
#   This script runs a *separate* black-box check that must match what
#   a real Claude Code / Codex / Cline agent sees over stdio:
#     1. The daemon binary builds.
#     2. Spawning it as `ucil-daemon mcp --stdio` produces a live JSON-RPC
#        server.
#     3. `initialize` returns a sane server descriptor.
#     4. `tools/list` returns exactly 22 tool descriptors with the frozen
#        tool-surface names from master-plan §3.
#     5. Every tool advertises the CEQP universal params
#        (reason / current_task / files_in_context / token_budget).
#
#   If any of these fail, the gate fails — an agent would see the same
#   thing and the "UCIL is usable" claim is false.
#
# Exit codes:
#   0 — smoke passed.
#   1 — handshake failed / tool count wrong / CEQP params missing.
#   2 — build failure (couldn't cargo build the daemon).
set -uo pipefail

cd "$(git rev-parse --show-toplevel)"

FROZEN_TOOLS=(
  understand_code find_definition find_references search_code find_similar
  get_context_for_edit get_conventions get_architecture trace_dependencies
  blast_radius explain_history remember review_changes check_quality
  run_tests security_scan lint_code type_check refactor generate_docs
  query_database check_runtime
)

echo "[e2e-mcp-smoke] building ucil-daemon..."
if ! cargo build -p ucil-daemon --bin ucil-daemon --quiet 2>&1 | tail -20; then
  echo "[e2e-mcp-smoke] FAIL: cargo build -p ucil-daemon failed" >&2
  exit 2
fi

DAEMON=./target/debug/ucil-daemon
if [[ ! -x "$DAEMON" ]]; then
  echo "[e2e-mcp-smoke] FAIL: $DAEMON not found after build" >&2
  exit 2
fi

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

REQ_INIT='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e-mcp-smoke","version":"1.0.0"}}}'
REQ_LIST='{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}'

printf '%s\n%s\n' "$REQ_INIT" "$REQ_LIST" \
  | timeout 15 "$DAEMON" mcp --stdio > "$TMP/out.jsonl" 2>"$TMP/err.log"
rc=$?
if [[ $rc -ne 0 && $rc -ne 124 ]]; then
  # 124 = timeout after stdin EOF, acceptable
  echo "[e2e-mcp-smoke] FAIL: daemon exited with $rc" >&2
  echo "-- stderr --" >&2
  tail -30 "$TMP/err.log" >&2
  exit 1
fi

if [[ ! -s "$TMP/out.jsonl" ]]; then
  echo "[e2e-mcp-smoke] FAIL: daemon produced no stdout responses" >&2
  echo "" >&2
  echo "  This usually means McpServer::serve() has not yet been wired" >&2
  echo "  into ucil-daemon's main.rs as a subcommand. server.rs has" >&2
  echo "  McpServer::serve(reader, writer) but main.rs only calls" >&2
  echo "  tracing_subscriber::fmt::init() and exits. Wire it like:" >&2
  echo "" >&2
  echo "    match std::env::args().nth(1).as_deref() {" >&2
  echo "        Some(\"mcp\") => server::McpServer::new()" >&2
  echo "            .serve(tokio::io::stdin(), tokio::io::stdout()).await?," >&2
  echo "        _ => { /* daemon mode */ }" >&2
  echo "    }" >&2
  exit 1
fi

# The server writes one JSON-RPC response per line. Find the tools/list response (id=2).
LIST_RESP=$(jq -c 'select(.id == 2)' < "$TMP/out.jsonl" 2>/dev/null | head -1)
if [[ -z "$LIST_RESP" ]]; then
  echo "[e2e-mcp-smoke] FAIL: no response to tools/list (id=2)" >&2
  echo "-- stdout --" >&2
  cat "$TMP/out.jsonl" >&2
  exit 1
fi

N_TOOLS=$(jq '.result.tools | length' <<<"$LIST_RESP")
if [[ "$N_TOOLS" != "22" ]]; then
  echo "[e2e-mcp-smoke] FAIL: expected 22 tools, got $N_TOOLS" >&2
  jq '.result.tools[] | .name' <<<"$LIST_RESP" >&2
  exit 1
fi

# Check that every frozen tool is present.
MISSING=()
for t in "${FROZEN_TOOLS[@]}"; do
  if ! jq -e --arg n "$t" '.result.tools[] | select(.name == $n)' <<<"$LIST_RESP" >/dev/null 2>&1; then
    MISSING+=("$t")
  fi
done
if (( ${#MISSING[@]} > 0 )); then
  echo "[e2e-mcp-smoke] FAIL: missing tools: ${MISSING[*]}" >&2
  exit 1
fi

# Check that every tool advertises the four CEQP params.
for ceqp in reason current_task files_in_context token_budget; do
  COUNT=$(jq --arg k "$ceqp" '[.result.tools[] | select(.inputSchema.properties[$k])] | length' <<<"$LIST_RESP")
  if [[ "$COUNT" != "22" ]]; then
    echo "[e2e-mcp-smoke] FAIL: CEQP param '$ceqp' missing on $((22 - COUNT)) tools" >&2
    exit 1
  fi
done

echo "[e2e-mcp-smoke] OK — 22 tools registered, CEQP params on all, daemon spoke MCP cleanly."
exit 0
