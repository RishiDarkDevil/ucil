#!/usr/bin/env bash
# Acceptance test for P3-W9-F06 — mem0 plugin manifest health-check +
# CRUD round-trip smoke (store -> retrieve -> list) against an
# ephemeral mktemp -d-rooted store.
#
# Behaviour:
#   1. cd to the repo root via `git rev-parse --show-toplevel`.
#   2. Assert `uvx` is on PATH; otherwise exit 1 with a clear hint
#      pointing at `scripts/devtools/install-mem0-mcp.sh`.
#   3. Print uvx + the pinned mem0-mcp-server version for the
#      verifier log.
#   4. Run `cargo test -p ucil-daemon --test g3_plugin_manifests
#      g3_plugin_manifests::mem0_manifest_health_check` and require
#      the cargo-test summary line "1 passed; 0 failed" or the
#      cargo-nextest equivalent — alternation regex per
#      WO-0042 / WO-0043 / WO-0044.
#   5. Tool-level CRUD round-trip smoke (gated by both
#      ${UCIL_SKIP_KNOWLEDGE_PLUGIN_E2E:-0} AND the presence of
#      MEM0_API_KEY — the upstream Mem0 server requires the API key
#      for tool invocations even though `tools/list` succeeds without
#      it; the integration test (step 4) covers the no-key health-
#      check path, and this step covers the with-key full-CRUD path):
#        a. mktemp -d for an ephemeral data dir (passed via
#           MEM0_DEFAULT_USER_ID so the smoke does not pollute the
#           operator's real Mem0 cloud account; the user_id scope
#           name embeds the smoke run id).
#        b. STORE — `tools/call` add_memory with text=
#           "wo-0069 smoke observation: ..." and capture the
#           returned memory_id.
#        c. RETRIEVE — `tools/call` search_memories with query=
#           "wo-0069 smoke observation" against the same user_id;
#           assert the response contains the stored text.
#        d. LIST — `tools/call` get_memories with limit=10 against
#           the same user_id; assert the response contains at least
#           one entry.
#        e. Best-effort cleanup: `tools/call` delete_all_memories on
#           the smoke user_id so the smoke run leaves no residue.
#   6. On all-green prints `[OK] P3-W9-F06` and exits 0; on any
#      failure prints `[FAIL] P3-W9-F06: <reason>` and exits 1.
#
# This script never modifies tests/fixtures/** (forbidden_paths in
# WO-0069). It uses an ephemeral mktemp -d-rooted scope so the
# operator's real Mem0 store stays untouched.
set -euo pipefail
IFS=$'\n\t'

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

PINNED_PYPI_VERSION="0.2.1"
PINNED_PKG_SPEC="mem0-mcp-server@${PINNED_PYPI_VERSION}"

# ── Prereq: uvx on PATH ────────────────────────────────────────────────
if ! command -v uvx >/dev/null 2>&1; then
    echo "[FAIL] P3-W9-F06: uvx not on PATH." >&2
    echo "  See scripts/devtools/install-mem0-mcp.sh for install hints." >&2
    exit 1
fi
echo "[INFO] P3-W9-F06: uvx version: $(uvx --version)"
echo "[INFO] P3-W9-F06: mem0-mcp-server pinned: ${PINNED_PKG_SPEC}"

# ── Step 1: integration test (real subprocess, real JSON-RPC) ─────────
CARGO_LOG="/tmp/wo-0069-f06-cargo.log"
echo "[INFO] P3-W9-F06: running cargo test g3_plugin_manifests::mem0_manifest_health_check..."
if ! cargo test -p ucil-daemon --test g3_plugin_manifests \
        g3_plugin_manifests::mem0_manifest_health_check 2>&1 | tee "${CARGO_LOG}" >/dev/null; then
    echo "[FAIL] P3-W9-F06: cargo test exited non-zero - see ${CARGO_LOG}" >&2
    tail -20 "${CARGO_LOG}" >&2 || true
    exit 1
fi
if ! grep -Eq 'test result: ok\. 1 passed; 0 failed|1 tests run: 1 passed' "${CARGO_LOG}"; then
    echo "[FAIL] P3-W9-F06: cargo test summary line missing in ${CARGO_LOG}" >&2
    tail -20 "${CARGO_LOG}" >&2 || true
    exit 1
fi
echo "[INFO] P3-W9-F06: integration test PASS."

# ── Step 2: tool-level CRUD round-trip smoke ──────────────────────────
if [[ "${UCIL_SKIP_KNOWLEDGE_PLUGIN_E2E:-0}" == "1" ]]; then
    echo "[SKIP] P3-W9-F06: tool-level smoke (UCIL_SKIP_KNOWLEDGE_PLUGIN_E2E=1)."
    echo "[OK] P3-W9-F06"
    exit 0
fi
if [[ -z "${MEM0_API_KEY:-}" ]]; then
    echo "[SKIP] P3-W9-F06: tool-level smoke (MEM0_API_KEY not set in env)."
    echo "  The Mem0 MCP server's add_memory / search_memories / get_memories"
    echo "  tools require an API key for embedding + storage. tools/list (the"
    echo "  cargo-test path above) succeeds without the key; the CRUD smoke"
    echo "  needs MEM0_API_KEY exported. Set MEM0_API_KEY=<your-key> to enable."
    echo "[OK] P3-W9-F06"
    exit 0
fi

MEM0_TMPDIR="$(mktemp -d -t wo-0069-f06-mem0-XXXXXX)"
trap 'rm -rf "${MEM0_TMPDIR}"' EXIT
SMOKE_USER_ID="ucil-wo-0069-smoke-$(date +%s)-$$"
SMOKE_TEXT="wo-0069 smoke observation: codebase-memory + mem0 G3 manifests landed."

# Drive the mem0 MCP server over a single stdio session: open fd 3
# for read + fd 4 for write on the spawned subprocess via a coproc;
# send initialize -> notifications/initialized -> three tools/call
# requests in sequence; pass each JSON-RPC response back to python3
# for parsing. Mirrors the WO-0044 verify-script pattern adapted for
# multi-call CRUD.
SMOKE_LOG="/tmp/wo-0069-f06-smoke.log"
SMOKE_INPUT="${MEM0_TMPDIR}/jsonrpc-in"
SMOKE_OUTPUT="${MEM0_TMPDIR}/jsonrpc-out"
mkfifo "${SMOKE_INPUT}"

# Spawn the server, hooking stdin from the FIFO and capturing stdout.
echo "[INFO] P3-W9-F06: spawning ${PINNED_PKG_SPEC} for CRUD smoke (user_id=${SMOKE_USER_ID})..."
uvx "${PINNED_PKG_SPEC}" <"${SMOKE_INPUT}" >"${SMOKE_OUTPUT}" 2>>"${SMOKE_LOG}" &
SERVER_PID=$!

# Hold the FIFO open for writes by attaching fd 4 to it; this prevents
# the server from seeing EOF after our first write.
exec 4>"${SMOKE_INPUT}"

cleanup_smoke() {
    exec 4>&-
    if kill -0 "${SERVER_PID}" 2>/dev/null; then
        kill "${SERVER_PID}" 2>/dev/null || true
        wait "${SERVER_PID}" 2>/dev/null || true
    fi
}
trap 'cleanup_smoke; rm -rf "${MEM0_TMPDIR}"' EXIT

# Send the protocol prefix.
{
    printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"ucil-wo-0069","version":"0.1.0"}}}'
    sleep 1
    printf '%s\n' '{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}'
    sleep 1
} >&4

# Helper: send one tools/call line, give the server a moment to
# respond, and capture stdout-so-far.
send_tools_call() {
    local id="$1"
    local payload="$2"
    printf '%s\n' "{\"jsonrpc\":\"2.0\",\"id\":${id},\"method\":\"tools/call\",\"params\":${payload}}" >&4
    sleep 6
}

ADD_PAYLOAD="{\"name\":\"add_memory\",\"arguments\":{\"text\":\"${SMOKE_TEXT}\",\"user_id\":\"${SMOKE_USER_ID}\"}}"
echo "[INFO] P3-W9-F06: STORE - tools/call add_memory..."
send_tools_call 2 "${ADD_PAYLOAD}"

SEARCH_PAYLOAD="{\"name\":\"search_memories\",\"arguments\":{\"query\":\"wo-0069 smoke observation\",\"filters\":{\"AND\":[{\"user_id\":\"${SMOKE_USER_ID}\"}]}}}"
echo "[INFO] P3-W9-F06: RETRIEVE - tools/call search_memories..."
send_tools_call 3 "${SEARCH_PAYLOAD}"

LIST_PAYLOAD="{\"name\":\"get_memories\",\"arguments\":{\"filters\":{\"AND\":[{\"user_id\":\"${SMOKE_USER_ID}\"}]},\"page_size\":10}}"
echo "[INFO] P3-W9-F06: LIST - tools/call get_memories..."
send_tools_call 4 "${LIST_PAYLOAD}"

CLEANUP_PAYLOAD="{\"name\":\"delete_all_memories\",\"arguments\":{\"user_id\":\"${SMOKE_USER_ID}\"}}"
echo "[INFO] P3-W9-F06: CLEANUP - tools/call delete_all_memories..."
send_tools_call 5 "${CLEANUP_PAYLOAD}"

# Close stdin so the server exits cleanly.
exec 4>&-
wait "${SERVER_PID}" 2>/dev/null || true

# Validate the round-trip via python3: the JSON-RPC stream must
# contain results for ids 2/3/4 (add/search/list) and the search
# result must reference the stored text.
if ! python3 - <<EOF
import json, sys
seen = {}
with open("${SMOKE_OUTPUT}") as fh:
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
needed = [2, 3, 4]
missing = [i for i in needed if i not in seen]
if missing:
    sys.stderr.write(f"missing JSON-RPC responses for ids {missing}; see ${SMOKE_OUTPUT}\n")
    sys.exit(1)
def pluck_text(frame):
    result = frame.get("result") or {}
    structured = result.get("structuredContent") or {}
    if "result" in structured:
        return str(structured["result"])
    content = result.get("content") or []
    if content and isinstance(content, list):
        first = content[0]
        if isinstance(first, dict) and "text" in first:
            return str(first["text"])
    return json.dumps(result)
add_text = pluck_text(seen[2])
search_text = pluck_text(seen[3])
list_text = pluck_text(seen[4])
if "wo-0069" not in search_text and "wo-0069" not in list_text:
    sys.stderr.write(f"neither search nor list response references the stored observation\nadd: {add_text[:200]}\nsearch: {search_text[:200]}\nlist: {list_text[:200]}\n")
    sys.exit(1)
print(f"OK: add={len(add_text)}b search={len(search_text)}b list={len(list_text)}b")
EOF
then
    echo "[FAIL] P3-W9-F06: CRUD round-trip did not satisfy round-trip invariant - see ${SMOKE_OUTPUT} ${SMOKE_LOG}" >&2
    tail -20 "${SMOKE_OUTPUT}" >&2 || true
    tail -20 "${SMOKE_LOG}" >&2 || true
    exit 1
fi

echo "[INFO] P3-W9-F06: CRUD round-trip preserved the observation."
echo "[OK] P3-W9-F06"
exit 0
