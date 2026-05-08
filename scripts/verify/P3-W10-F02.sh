#!/usr/bin/env bash
# Acceptance test for P3-W10-F02 — Context7 G5 plugin manifest
# health-check + library-docs lookup smoke against the
# typescript-project fixture.
#
# Behaviour:
#   1. cd to the repo root via `git rev-parse --show-toplevel`.
#   2. Sanity-check `plugins/context/context7/plugin.toml` parses as
#      TOML and asserts the keys named in WO-0074 scope_in #1
#      (plugin.name == "context7", plugin.category == "context",
#      transport.type == "stdio", capabilities.provides starts with
#      "context.").
#   3. Assert `npx` is on PATH; otherwise exit 1 with a clear hint
#      pointing at `scripts/devtools/install-context7-mcp.sh`.
#   4. Print npx + the pinned @upstash/context7-mcp version for the
#      verifier log.
#   5. Run `cargo test -p ucil-daemon --test g5_plugin_manifests
#      g5_plugin_manifests::context7_manifest_health_check` and
#      require the cargo-test summary line "1 passed; 0 failed" or
#      the cargo-nextest equivalent — alternation regex per
#      WO-0042 / WO-0043 / WO-0044 / WO-0069 / WO-0072.
#   6. Tool-level resolve-library-id smoke (gated by
#      ${UCIL_SKIP_CONTEXT_PLUGIN_E2E:-0}, default RUN per WO-0074
#      scope_in #6 since the tool needs no API key):
#        a. Spawn the Context7 MCP server over a single stdio
#           session (mirrors WO-0072 P3-W9-F08.sh dep-graph-smoke
#           pattern, adapted for the docs-lookup vocabulary).
#        b. INIT — `initialize` + `notifications/initialized`.
#        c. RESOLVE — `tools/call` resolve-library-id with
#           `query="how to write a vitest test"` and
#           `libraryName="vitest"`. The vitest library is chosen
#           because it is one of the two devDependencies declared in
#           tests/fixtures/typescript-project/package.json (the other
#           is `typescript` — vitest is preferred because Context7
#           returns multiple matching candidates for it, exercising
#           the disambiguation logic that resolve-library-id is
#           designed for; `typescript` would also work but is more
#           ambiguous as a query). The fixture is read-only:
#           neither the fixture file nor anything else under
#           tests/fixtures/ is mutated.
#        d. Assert the JSON-RPC response is a successful reply
#           (id matches, no `error` key) and contains a non-empty
#           `content` field consistent with the F02 spec line
#           ("query for a library used in the fixture
#           typescript-project returns current API docs").
#   7. On all-green prints `[OK] P3-W10-F02` and exits 0; on any
#      failure prints `[FAIL] P3-W10-F02: <reason>` and exits 1.
#
# This script is read-only against the fixture: it never modifies
# tests/fixtures/typescript-project (forbidden_paths in WO-0074).
set -euo pipefail
IFS=$'\n\t'

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

PINNED_NPM_VERSION="2.2.4"
PINNED_PKG_SPEC="@upstash/context7-mcp@${PINNED_NPM_VERSION}"
MANIFEST_PATH="plugins/context/context7/plugin.toml"

# ── Step 0: manifest TOML sanity (cheap pre-flight) ────────────────────
echo "[INFO] P3-W10-F02: parsing ${MANIFEST_PATH} ..."
if ! python3 -c "
import tomllib
m = tomllib.load(open('${MANIFEST_PATH}', 'rb'))
assert m['plugin']['name'] == 'context7', f\"plugin.name != 'context7': {m['plugin']['name']!r}\"
assert m['plugin']['category'] == 'context', f\"plugin.category != 'context': {m['plugin']['category']!r}\"
assert m['transport']['type'] == 'stdio', f\"transport.type != 'stdio': {m['transport']['type']!r}\"
assert any(c.startswith('context.') for c in m['capabilities']['provides']), \
    f\"no capability under context.* namespace: {m['capabilities']['provides']!r}\"
print('manifest-sanity: ok')
"; then
    echo "[FAIL] P3-W10-F02: manifest sanity failed for ${MANIFEST_PATH}" >&2
    exit 1
fi

# ── Prereq: npx on PATH ────────────────────────────────────────────────
if ! command -v npx >/dev/null 2>&1; then
    echo "[FAIL] P3-W10-F02: npx not on PATH." >&2
    echo "  See scripts/devtools/install-context7-mcp.sh for install hints." >&2
    exit 1
fi
echo "[INFO] P3-W10-F02: npx version: $(npx --version)"
echo "[INFO] P3-W10-F02: context7 pinned: ${PINNED_PKG_SPEC}"

# ── Step 1: integration test (real subprocess, real JSON-RPC) ─────────
CARGO_LOG="/tmp/wo-0074-f02-cargo.log"
echo "[INFO] P3-W10-F02: running cargo test g5_plugin_manifests::context7_manifest_health_check..."
if ! cargo test -p ucil-daemon --test g5_plugin_manifests \
        g5_plugin_manifests::context7_manifest_health_check 2>&1 | tee "${CARGO_LOG}" >/dev/null; then
    echo "[FAIL] P3-W10-F02: cargo test exited non-zero — see ${CARGO_LOG}" >&2
    tail -20 "${CARGO_LOG}" >&2 || true
    exit 1
fi
if ! grep -Eq 'test result: ok\. 1 passed; 0 failed|1 tests run: 1 passed' "${CARGO_LOG}"; then
    echo "[FAIL] P3-W10-F02: cargo test summary line missing in ${CARGO_LOG}" >&2
    tail -20 "${CARGO_LOG}" >&2 || true
    exit 1
fi
echo "[INFO] P3-W10-F02: integration test PASS."

# ── Step 2: tool-level docs-lookup smoke ──────────────────────────────
if [[ "${UCIL_SKIP_CONTEXT_PLUGIN_E2E:-0}" == "1" ]]; then
    echo "[SKIP] P3-W10-F02: tool-level smoke (UCIL_SKIP_CONTEXT_PLUGIN_E2E=1)."
    echo "[OK] P3-W10-F02"
    exit 0
fi

C7_TMPDIR="$(mktemp -d -t wo-0074-f02-c7-XXXXXX)"
SMOKE_LOG="/tmp/wo-0074-f02-smoke.log"
SMOKE_INPUT="${C7_TMPDIR}/jsonrpc-in"
SMOKE_OUTPUT="${C7_TMPDIR}/jsonrpc-out"
mkfifo "${SMOKE_INPUT}"

echo "[INFO] P3-W10-F02: spawning ${PINNED_PKG_SPEC} for docs-lookup smoke..."
npx -y "${PINNED_PKG_SPEC}" \
    <"${SMOKE_INPUT}" >"${SMOKE_OUTPUT}" 2>>"${SMOKE_LOG}" &
SERVER_PID=$!

exec 4>"${SMOKE_INPUT}"

cleanup_smoke() {
    exec 4>&-
    if kill -0 "${SERVER_PID}" 2>/dev/null; then
        kill "${SERVER_PID}" 2>/dev/null || true
        wait "${SERVER_PID}" 2>/dev/null || true
    fi
    rm -rf "${C7_TMPDIR}"
}
trap cleanup_smoke EXIT

# Send the protocol prefix (initialize + initialized notification).
{
    printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"ucil-wo-0074","version":"0.1.0"}}}'
    sleep 1
    printf '%s\n' '{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}'
    sleep 1
} >&4

send_tools_call() {
    local id="$1"
    local payload="$2"
    local pause="${3:-5}"
    printf '%s\n' "{\"jsonrpc\":\"2.0\",\"id\":${id},\"method\":\"tools/call\",\"params\":${payload}}" >&4
    sleep "${pause}"
}

# resolve-library-id requires both `query` and `libraryName`.
RESOLVE_PAYLOAD='{"name":"resolve-library-id","arguments":{"query":"how to write a vitest test","libraryName":"vitest"}}'
echo "[INFO] P3-W10-F02: RESOLVE - tools/call resolve-library-id (libraryName=vitest)..."
send_tools_call 2 "${RESOLVE_PAYLOAD}" 10

# Close stdin so the server exits cleanly.
exec 4>&-
wait "${SERVER_PID}" 2>/dev/null || true

# Validate via python3: the JSON-RPC stream must contain a successful
# result for id=2 (no error key, non-empty content).
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
        if rid in (1, 2):
            seen[rid] = frame
needed = [2]
missing = [i for i in needed if i not in seen]
if missing:
    sys.stderr.write(f"missing JSON-RPC responses for ids {missing}; see ${SMOKE_OUTPUT}\n")
    sys.exit(1)
frame = seen[2]
if "error" in frame:
    sys.stderr.write(f"resolve-library-id returned a JSON-RPC error: {frame['error']!r}\n")
    sys.exit(1)
result = frame.get("result") or {}
content = result.get("content") or []
if not content:
    sys.stderr.write(f"resolve-library-id result.content is empty: {json.dumps(result)[:300]}\n")
    sys.exit(1)
# content is a list of {type,text} blocks. Concatenate the text blocks
# to get the full disambiguation payload — must be non-empty AND
# mention vitest somewhere (case-insensitive) so we know the response
# is consistent with the libraryName we asked about.
text_blocks = [b.get("text","") for b in content if isinstance(b, dict)]
joined = "\n".join(text_blocks)
if not joined.strip():
    sys.stderr.write(f"resolve-library-id text content is whitespace-only: {json.dumps(result)[:300]}\n")
    sys.exit(1)
if "vitest" not in joined.lower():
    sys.stderr.write(f"resolve-library-id text content does not mention vitest: {joined[:300]}\n")
    sys.exit(1)
print(f"OK: resolve-library-id returned {len(joined)} chars of disambiguation content for vitest")
EOF
then
    echo "[FAIL] P3-W10-F02: docs-lookup smoke did not satisfy the round-trip invariant - see ${SMOKE_OUTPUT} ${SMOKE_LOG}" >&2
    tail -20 "${SMOKE_OUTPUT}" >&2 || true
    tail -20 "${SMOKE_LOG}" >&2 || true
    exit 1
fi

echo "[INFO] P3-W10-F02: docs-lookup smoke preserved the disambiguation signal."
echo "[OK] P3-W10-F02"
exit 0
