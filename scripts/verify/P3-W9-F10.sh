#!/usr/bin/env bash
# Acceptance test for P3-W9-F10 — graphiti plugin manifest health-check
# + tool-level smoke (add_memory -> search_memory_facts) against an
# operator-provided graph DB.
#
# Behaviour:
#   1. cd to the repo root via `git rev-parse --show-toplevel`.
#   2. Assert `uvx` is on PATH; otherwise exit 1 with a clear hint
#      pointing at `scripts/devtools/install-graphiti-mcp.sh`.
#   3. Print uvx + the pinned graphiti commit SHA for the verifier log.
#   4. Run `cargo test -p ucil-daemon --test g3_plugin_manifests
#      g3_plugin_manifests::graphiti_manifest_health_check` and require
#      the cargo-test summary line "1 passed; 0 failed" or the
#      cargo-nextest equivalent — alternation regex per
#      WO-0042 / WO-0043 / WO-0044.
#   5. Tool-level smoke (DUAL-DEPENDENT short-circuit per WO-0079
#      scope_in #6 / #11; gated by ALL of:
#        - ${UCIL_SKIP_KNOWLEDGE_PLUGIN_E2E:-0} == 0
#        - one of FALKORDB_URI / NEO4J_URI is set
#        - OPENAI_API_KEY (or another --llm-provider key) is set):
#        a. Spawn the graphiti MCP server via the manifest's
#           `uvx --from <git+ref> python -m graphiti_mcp_server
#            --transport stdio` invocation.
#        b. STORE - `tools/call` add_memory with name="wo-0079
#           smoke episode" and episode_body referencing the smoke
#           run_id; group_id is the smoke namespace.
#        c. RETRIEVE - `tools/call` search_memory_facts with query=
#           "wo-0079 smoke" and group_ids=[smoke namespace]; the
#           returned facts list MAY be empty (the upstream queue-
#           based pipeline processes episodes asynchronously and the
#           smoke window is short - eventual-consistency disjunction
#           per WO-0079 scope_in #21).
#        d. CLEANUP - `tools/call` clear_graph with the smoke
#           namespace so the smoke run leaves no residue on the
#           operator's real graph.
#   6. On all-green prints `[OK] P3-W9-F10` and exits 0; on any
#      failure prints `[FAIL] P3-W9-F10: <reason>` and exits 1.
#
# This script never modifies tests/fixtures/** (forbidden_paths in
# WO-0079). The integration test (step 4) is the load-bearing
# assertion regardless of the smoke gates; the smoke step is a
# round-trip sanity check on top.
set -euo pipefail
IFS=$'\n\t'

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

PINNED_GRAPHITI_TAG="mcp-v1.0.2"
PINNED_GRAPHITI_COMMIT_SHA="19e44a97a929ebf121294f97f26966f0379d8e30"
PINNED_GIT_REF="git+https://github.com/getzep/graphiti.git@${PINNED_GRAPHITI_TAG}#subdirectory=mcp_server"

# Prereq: uvx on PATH
if ! command -v uvx >/dev/null 2>&1; then
    echo "[FAIL] P3-W9-F10: uvx not on PATH." >&2
    echo "  See scripts/devtools/install-graphiti-mcp.sh for install hints." >&2
    exit 1
fi
echo "[INFO] P3-W9-F10: uvx version: $(uvx --version)"
echo "[INFO] P3-W9-F10: graphiti pinned: ${PINNED_GRAPHITI_TAG} (SHA ${PINNED_GRAPHITI_COMMIT_SHA})"

# Step 1: integration test (real subprocess, real JSON-RPC)
CARGO_LOG="/tmp/wo-0079-f10-cargo.log"
echo "[INFO] P3-W9-F10: running cargo test g3_plugin_manifests::graphiti_manifest_health_check..."
if ! cargo test -p ucil-daemon --test g3_plugin_manifests \
        g3_plugin_manifests::graphiti_manifest_health_check 2>&1 | tee "${CARGO_LOG}" >/dev/null; then
    echo "[FAIL] P3-W9-F10: cargo test exited non-zero - see ${CARGO_LOG}" >&2
    tail -20 "${CARGO_LOG}" >&2 || true
    exit 1
fi
if ! grep -Eq 'test result: ok\. 1 passed; 0 failed|1 tests run: 1 passed' "${CARGO_LOG}"; then
    echo "[FAIL] P3-W9-F10: cargo test summary line missing in ${CARGO_LOG}" >&2
    tail -20 "${CARGO_LOG}" >&2 || true
    exit 1
fi
echo "[INFO] P3-W9-F10: integration test PASS."

# Step 2: tool-level CRUD smoke (dual-gated)
if [[ "${UCIL_SKIP_KNOWLEDGE_PLUGIN_E2E:-0}" == "1" ]]; then
    echo "[SKIP] P3-W9-F10: tool-level smoke (UCIL_SKIP_KNOWLEDGE_PLUGIN_E2E=1)."
    echo "[OK] P3-W9-F10"
    exit 0
fi
if [[ -z "${FALKORDB_URI:-}${NEO4J_URI:-}" ]]; then
    echo "[SKIP] P3-W9-F10: tool-level smoke (neither FALKORDB_URI nor NEO4J_URI set)."
    echo "  graphiti's lifespan handler eagerly connects to a graph DB on startup;"
    echo "  tools/list (the cargo-test path above) covers the no-DB skip case."
    echo "  Set FALKORDB_URI=redis://127.0.0.1:6379 or NEO4J_URI=bolt://127.0.0.1:7687"
    echo "  (plus credentials) to enable the tool-level smoke."
    echo "[OK] P3-W9-F10"
    exit 0
fi
if [[ -z "${OPENAI_API_KEY:-}${ANTHROPIC_API_KEY:-}${GROQ_API_KEY:-}${GEMINI_API_KEY:-}" ]]; then
    echo "[SKIP] P3-W9-F10: tool-level smoke (no LLM API key set in env)."
    echo "  graphiti requires OPENAI_API_KEY (default) or another"
    echo "  --llm-provider key for tool invocations. Set one of:"
    echo "    OPENAI_API_KEY / ANTHROPIC_API_KEY / GROQ_API_KEY / GEMINI_API_KEY"
    echo "  to enable the tool-level smoke."
    echo "[OK] P3-W9-F10"
    exit 0
fi

GRAPHITI_TMPDIR="$(mktemp -d -t wo-0079-f10-graphiti-XXXXXX)"
SMOKE_GROUP_ID="ucil-wo-0079-smoke-$(date +%s)-$$"
SMOKE_TEXT="wo-0079 smoke episode: graphiti G3 plugin manifest landed at SHA ${PINNED_GRAPHITI_COMMIT_SHA}."

# Drive the graphiti MCP server over a single stdio session (FIFO-fed
# stdin, captured stdout); send initialize -> notifications/initialized
# -> three tools/call requests in sequence. Mirrors the WO-0069 verify
# script's coproc shape adapted for graphiti's async-queue pipeline.
SMOKE_LOG="/tmp/wo-0079-f10-smoke.log"
SMOKE_INPUT="${GRAPHITI_TMPDIR}/jsonrpc-in"
SMOKE_OUTPUT="${GRAPHITI_TMPDIR}/jsonrpc-out"
mkfifo "${SMOKE_INPUT}"

echo "[INFO] P3-W9-F10: spawning graphiti for tool-level smoke (group_id=${SMOKE_GROUP_ID})..."
uvx --from "${PINNED_GIT_REF}" \
    python -m graphiti_mcp_server --transport stdio \
    <"${SMOKE_INPUT}" >"${SMOKE_OUTPUT}" 2>>"${SMOKE_LOG}" &
SERVER_PID=$!

# Hold the FIFO open for writes via fd 4 so the server does not see
# EOF after our first write.
exec 4>"${SMOKE_INPUT}"

cleanup_smoke() {
    exec 4>&-
    if kill -0 "${SERVER_PID}" 2>/dev/null; then
        kill "${SERVER_PID}" 2>/dev/null || true
        wait "${SERVER_PID}" 2>/dev/null || true
    fi
}
trap 'cleanup_smoke; rm -rf "${GRAPHITI_TMPDIR}"' EXIT

{
    printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"ucil-wo-0079","version":"0.1.0"}}}'
    sleep 2
    printf '%s\n' '{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}'
    sleep 2
} >&4

send_tools_call() {
    local id="$1"
    local payload="$2"
    printf '%s\n' "{\"jsonrpc\":\"2.0\",\"id\":${id},\"method\":\"tools/call\",\"params\":${payload}}" >&4
    sleep 5
}

ADD_PAYLOAD="{\"name\":\"add_memory\",\"arguments\":{\"name\":\"wo-0079 smoke episode\",\"episode_body\":\"${SMOKE_TEXT}\",\"source\":\"text\",\"source_description\":\"ucil-wo-0079 verify\",\"group_id\":\"${SMOKE_GROUP_ID}\"}}"
echo "[INFO] P3-W9-F10: STORE - tools/call add_memory..."
send_tools_call 2 "${ADD_PAYLOAD}"

# Allow upstream's async queue some processing budget before search.
sleep 8

SEARCH_PAYLOAD="{\"name\":\"search_memory_facts\",\"arguments\":{\"query\":\"wo-0079 smoke\",\"group_ids\":[\"${SMOKE_GROUP_ID}\"],\"max_facts\":5}}"
echo "[INFO] P3-W9-F10: RETRIEVE - tools/call search_memory_facts..."
send_tools_call 3 "${SEARCH_PAYLOAD}"

CLEANUP_PAYLOAD="{\"name\":\"clear_graph\",\"arguments\":{\"group_ids\":[\"${SMOKE_GROUP_ID}\"]}}"
echo "[INFO] P3-W9-F10: CLEANUP - tools/call clear_graph..."
send_tools_call 4 "${CLEANUP_PAYLOAD}"

exec 4>&-
wait "${SERVER_PID}" 2>/dev/null || true

# Validate via python3: the JSON-RPC stream must contain results for
# ids 2 / 3 (the search MAY be empty due to upstream's async queue -
# eventual-consistency OR-disjunction per WO-0079 scope_in #21; we
# accept either a non-empty fact list OR a successful add_memory
# response confirming the episode was queued).
if ! SMOKE_OUTPUT="${SMOKE_OUTPUT}" python3 - <<'PYEOF'
import json, os, sys
out_path = os.environ["SMOKE_OUTPUT"]
seen = {}
with open(out_path) as fh:
    for line in fh:
        line = line.strip()
        if not line:
            continue
        try:
            frame = json.loads(line)
        except json.JSONDecodeError:
            continue
        rid = frame.get("id")
        if rid in (2, 3, 4):
            seen[rid] = frame
needed = [2, 3]
missing = [i for i in needed if i not in seen]
if missing:
    sys.stderr.write(f"missing JSON-RPC responses for ids {missing}; see {out_path}\n")
    sys.exit(1)
def pluck_text(frame):
    result = frame.get("result") or {}
    if isinstance(result, dict) and "isError" in result and result.get("isError"):
        return "ERROR: " + json.dumps(result)
    structured = result.get("structuredContent") or {}
    if structured:
        return json.dumps(structured)
    content = result.get("content") or []
    if content and isinstance(content, list):
        first = content[0]
        if isinstance(first, dict) and "text" in first:
            return str(first["text"])
    return json.dumps(result)
add_text = pluck_text(seen[2])
search_text = pluck_text(seen[3])
# Eventual-consistency OR-disjunction per WO-0079 scope_in #21:
# the search may not reflect the just-stored episode within the
# smoke window because graphiti's queue is async. Accept the smoke
# if EITHER the search references the run id, OR the add_memory
# response confirmed enqueue (no isError flag).
if "wo-0079" in search_text:
    print(f"OK: search_memory_facts surfaced the stored episode (search={len(search_text)}b)")
elif add_text.startswith("ERROR:"):
    sys.stderr.write(f"add_memory failed: {add_text[:300]}\n")
    sys.exit(1)
else:
    print(f"OK: add_memory enqueued (add={len(add_text)}b); search yet to surface (eventual consistency)")
PYEOF
then
    echo "[FAIL] P3-W9-F10: tool-level smoke did not satisfy round-trip invariant - see ${SMOKE_OUTPUT} ${SMOKE_LOG}" >&2
    tail -40 "${SMOKE_OUTPUT}" >&2 || true
    tail -40 "${SMOKE_LOG}" >&2 || true
    exit 1
fi

echo "[INFO] P3-W9-F10: tool-level smoke green."
echo "[OK] P3-W9-F10"
exit 0
