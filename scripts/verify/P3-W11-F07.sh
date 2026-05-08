#!/usr/bin/env bash
# Acceptance test for P3-W11-F07 — test-runner G8 plugin manifest
# health-check (cargo test) + INDEPENDENT JSON-RPC tools/list smoke
# asserting (a) tools[] len == 1, (b) tools[0].name == "run_tests",
# (c) inputSchema.properties.framework.enum has ≥6 values from the
# canonical set {bats, pytest, flutter, jest, go, rust, cargo,
# generic, vitest}.
#
# Behaviour:
#   1. cd to the repo root via `git rev-parse --show-toplevel`.
#   2. Sanity-check `plugins/testing/test-runner/plugin.toml`
#      parses as TOML and asserts the keys named in WO-0082 scope_in
#      #1 (plugin.name == "test-runner",
#      plugin.category == "testing", transport.type == "stdio",
#      transport.command == "npx", capabilities.provides starts with
#      "testing.").
#   3. Assert `npx` + `node` + `python3` are on PATH; otherwise exit 1
#      with a clear hint pointing at
#      `scripts/devtools/install-test-runner-mcp.sh`.
#   4. Print npx/node + the pinned test-runner version for the
#      verifier log.
#   5. Run `cargo test -p ucil-daemon --test g8_plugin_manifests
#      g8_plugin_manifests::test_runner_manifest_health_check`
#      and require the cargo-test summary line "1 passed; 0 failed"
#      or the cargo-nextest equivalent — alternation regex per
#      WO-0042 / WO-0044 / WO-0069 / WO-0072 / WO-0074 / WO-0075 /
#      WO-0076 / WO-0077.
#   6. Tool-level smoke (gated by ${UCIL_SKIP_TESTING_PLUGIN_E2E:-0},
#      default RUN per WO-0074 §executor #3 + WO-0076 scope_in #6 +
#      WO-0077 §executor #3 — extends the existing G8 env-var
#      verbatim from WO-0077, NO new env-var introduced):
#        a. Spawn `npx -y @iflow-mcp/mcp-test-runner@<pin>` from a
#           tmpdir as cwd (per WO-0074 §executor #5 + WO-0076
#           scope_in #5/#16 — the upstream `tools/list` is cwd-
#           independent so the tmpdir is purely a hygienic isolation
#           barrier).
#        b. INIT - `initialize` + `notifications/initialized`.
#        c. tools/list - assert (a) `tools[]` length == 1, (b)
#           `tools[0].name == "run_tests"` (single-tool dispatcher
#           per DEC-0025 §Decision point 3), (c) `tools[0].
#           inputSchema.properties.framework.enum` is a list with
#           len ≥6, (d) the enum is a subset of the canonical set
#           {bats, pytest, flutter, jest, go, rust, cargo, generic,
#           vitest} — allows future upstream additions, asserts no
#           foreign values.
#        d. OPTIONAL secondary tools/call run_tests — gated by
#           `cargo --version` short-circuit per WO-0076 operator-
#           state pattern. Informational-only on green/skip; not
#           load-bearing on the verify-script PASS verdict per
#           DEC-0025 §Decision point 2 + WO-0082 scope_in #6.
#   7. On all-green prints `[OK] P3-W11-F07` and exits 0; on any
#      failure prints `[FAIL] P3-W11-F07: <reason>` and exits 1.
#
# Per WO-0074 §executor #4: the tool-level smoke uses python polling
# with a deadline rather than bash `sleep` so wall-time-sensitive
# stdio JSON-RPC reads complete reliably even on slow workstations
# (cold-cache npx fetch of @iflow-mcp/mcp-test-runner +
# @modelcontextprotocol/sdk + transitive deps takes up to ~30s).
set -euo pipefail
IFS=$'\n\t'

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

PINNED_TEST_RUNNER_MCP_VERSION="0.2.1"
PINNED_TEST_RUNNER_MCP_PACKAGE="@iflow-mcp/mcp-test-runner"
PINNED_PKG_SPEC="${PINNED_TEST_RUNNER_MCP_PACKAGE}@${PINNED_TEST_RUNNER_MCP_VERSION}"
MANIFEST_PATH="plugins/testing/test-runner/plugin.toml"

# Step 0: manifest TOML sanity (cheap pre-flight).
echo "[INFO] P3-W11-F07: parsing ${MANIFEST_PATH} ..."
if ! python3 -c "
import tomllib
m = tomllib.load(open('${MANIFEST_PATH}', 'rb'))
assert m['plugin']['name'] == 'test-runner', f\"plugin.name != 'test-runner': {m['plugin']['name']!r}\"
assert m['plugin']['category'] == 'testing', f\"plugin.category != 'testing': {m['plugin']['category']!r}\"
assert m['transport']['type'] == 'stdio', f\"transport.type != 'stdio': {m['transport']['type']!r}\"
assert m['transport']['command'] == 'npx', f\"transport.command != 'npx': {m['transport']['command']!r}\"
assert any(c.startswith('testing.') for c in m['capabilities']['provides']), \
    f\"no capability under testing.* namespace: {m['capabilities']['provides']!r}\"
print('manifest-sanity: ok')
"; then
    echo "[FAIL] P3-W11-F07: manifest sanity failed for ${MANIFEST_PATH}" >&2
    exit 1
fi

# Prereq: npx + node + python3 on PATH.
if ! command -v npx >/dev/null 2>&1; then
    echo "[FAIL] P3-W11-F07: npx not on PATH." >&2
    echo "  See scripts/devtools/install-test-runner-mcp.sh for install hints." >&2
    exit 1
fi
if ! command -v node >/dev/null 2>&1; then
    echo "[FAIL] P3-W11-F07: node not on PATH." >&2
    exit 1
fi
if ! command -v python3 >/dev/null 2>&1; then
    echo "[FAIL] P3-W11-F07: python3 not on PATH." >&2
    exit 1
fi
echo "[INFO] P3-W11-F07: node version: $(node --version | head -1)"
echo "[INFO] P3-W11-F07: npx version: $(npx --version | head -1)"
echo "[INFO] P3-W11-F07: test-runner pinned: ${PINNED_PKG_SPEC}"

# Step 1: integration test (real subprocess, real JSON-RPC).
CARGO_LOG="/tmp/wo-0082-f07-cargo.log"
echo "[INFO] P3-W11-F07: running cargo test g8_plugin_manifests::test_runner_manifest_health_check..."
if ! cargo test -p ucil-daemon --test g8_plugin_manifests \
        g8_plugin_manifests::test_runner_manifest_health_check 2>&1 | tee "${CARGO_LOG}" >/dev/null; then
    echo "[FAIL] P3-W11-F07: cargo test exited non-zero - see ${CARGO_LOG}" >&2
    tail -20 "${CARGO_LOG}" >&2 || true
    exit 1
fi
if ! grep -Eq 'test result: ok\. 1 passed; 0 failed|1 tests run: 1 passed' "${CARGO_LOG}"; then
    echo "[FAIL] P3-W11-F07: cargo test summary line missing in ${CARGO_LOG}" >&2
    tail -20 "${CARGO_LOG}" >&2 || true
    exit 1
fi
echo "[INFO] P3-W11-F07: integration test PASS."

# Step 2: tool-level smoke.
if [[ "${UCIL_SKIP_TESTING_PLUGIN_E2E:-0}" == "1" ]]; then
    echo "[SKIP] P3-W11-F07: tool-level smoke (UCIL_SKIP_TESTING_PLUGIN_E2E=1)."
    echo "[OK] P3-W11-F07"
    exit 0
fi

SMOKE_TMPDIR="$(mktemp -d -t wo-0082-f07-smoke-XXXXXX)"
SMOKE_LOG="/tmp/wo-0082-f07-smoke.log"
trap 'rm -rf "${SMOKE_TMPDIR}"' EXIT

export PINNED_PKG_SPEC SMOKE_TMPDIR SMOKE_LOG

# Drive the MCP session from python3 with deadline polling per
# WO-0074 §executor #4. cold-cache npx fetch of
# @iflow-mcp/mcp-test-runner + @modelcontextprotocol/sdk +
# transitive deps takes several seconds.
if ! python3 - <<'PY'
import json
import os
import subprocess
import sys
import time


CANONICAL_FRAMEWORKS = {
    "bats",
    "pytest",
    "flutter",
    "jest",
    "go",
    "rust",
    "cargo",
    "generic",
    "vitest",
}


def main() -> int:
    pkg_spec = os.environ["PINNED_PKG_SPEC"]
    tmpdir = os.environ["SMOKE_TMPDIR"]
    smoke_log = open(os.environ["SMOKE_LOG"], "ab")

    proc = subprocess.Popen(
        ["npx", "-y", pkg_spec],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=smoke_log,
        text=True,
        bufsize=1,
        cwd=tmpdir,
    )
    try:
        # Initialize.
        init_msg = {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "ucil-wo-0082", "version": "0.1.0"},
            },
        }
        assert proc.stdin is not None
        proc.stdin.write(json.dumps(init_msg) + "\n")
        proc.stdin.flush()

        deadline = time.monotonic() + 90.0
        init_replied = False
        while time.monotonic() < deadline and not init_replied:
            assert proc.stdout is not None
            line = proc.stdout.readline()
            if not line:
                break
            line = line.strip()
            if not line:
                continue
            try:
                frame = json.loads(line)
            except json.JSONDecodeError:
                continue
            if frame.get("id") == 1:
                init_replied = True
        if not init_replied:
            sys.stderr.write("test-runner did not reply to initialize within 90s\n")
            return 1

        # initialized notification.
        proc.stdin.write(
            json.dumps(
                {
                    "jsonrpc": "2.0",
                    "method": "notifications/initialized",
                    "params": {},
                }
            )
            + "\n"
        )
        proc.stdin.flush()

        # tools/list.
        proc.stdin.write(
            json.dumps(
                {
                    "jsonrpc": "2.0",
                    "id": 2,
                    "method": "tools/list",
                    "params": {},
                }
            )
            + "\n"
        )
        proc.stdin.flush()

        deadline = time.monotonic() + 30.0
        tl_frame = None
        while time.monotonic() < deadline and tl_frame is None:
            assert proc.stdout is not None
            line = proc.stdout.readline()
            if not line:
                break
            line = line.strip()
            if not line:
                continue
            try:
                frame = json.loads(line)
            except json.JSONDecodeError:
                continue
            if frame.get("id") == 2:
                tl_frame = frame
                break
        if tl_frame is None:
            sys.stderr.write("test-runner did not reply to tools/list within 30s\n")
            return 1
        if "error" in tl_frame:
            sys.stderr.write(
                f"tools/list returned a JSON-RPC error: {tl_frame['error']!r}\n"
            )
            return 1

        tools = tl_frame.get("result", {}).get("tools") or []
        # (a) tools[] len == 1 (single-tool dispatcher per DEC-0025).
        if len(tools) != 1:
            sys.stderr.write(
                f"expected 1 tool from test-runner (single-tool dispatcher per "
                f"DEC-0025); got {len(tools)}: {[t.get('name') for t in tools]!r}\n"
            )
            return 1
        tool = tools[0]
        # (b) tools[0].name == "run_tests".
        if tool.get("name") != "run_tests":
            sys.stderr.write(
                f"expected tools[0].name == 'run_tests' (canonical upstream tool name "
                f"per DEC-0025); got: {tool.get('name')!r}\n"
            )
            return 1

        # (c) inputSchema.properties.framework.enum has len ≥6.
        schema = tool.get("inputSchema") or {}
        props = schema.get("properties") or {}
        framework = props.get("framework") or {}
        enum = framework.get("enum") or []
        if not isinstance(enum, list):
            sys.stderr.write(f"framework.enum is not a list: {enum!r}\n")
            return 1
        if len(enum) < 6:
            sys.stderr.write(
                f"framework.enum has only {len(enum)} values, need >=6 per DEC-0025: "
                f"{enum!r}\n"
            )
            return 1
        # (d) enum is a subset of the canonical set (no foreign
        # values; future upstream additions inside the canonical set
        # are tolerated).
        foreign = [v for v in enum if v not in CANONICAL_FRAMEWORKS]
        if foreign:
            sys.stderr.write(
                f"framework.enum contains foreign values (not in canonical set "
                f"{sorted(CANONICAL_FRAMEWORKS)!r}): {foreign!r}; full enum: {enum!r}\n"
            )
            return 1

        print(
            f"tools/list ok: 1 tool '{tool.get('name')}'; framework enum "
            f"({len(enum)} values): {enum!r}"
        )
        return 0
    finally:
        try:
            if proc.stdin is not None:
                proc.stdin.close()
        except Exception:
            pass
        try:
            proc.terminate()
            proc.wait(timeout=5)
        except Exception:
            try:
                proc.kill()
            except Exception:
                pass
        try:
            smoke_log.close()
        except Exception:
            pass


sys.exit(main())
PY
then
    echo "[FAIL] P3-W11-F07: tool-level smoke failed - see ${SMOKE_LOG}" >&2
    tail -30 "${SMOKE_LOG}" >&2 || true
    exit 1
fi

echo "[INFO] P3-W11-F07: tool-level smoke PASS."

# Step 3: OPTIONAL secondary tools/call run_tests — gated by
# `cargo --version` short-circuit per WO-0076 operator-state
# pattern. Informational-only on green/skip; not load-bearing on
# the verify-script verdict per DEC-0025 §Decision point 2 +
# WO-0082 scope_in #6 (the SECONDARY operator-state CLI deps
# `cargo`/`pytest`/`vitest`/`go`/`bats`/`flutter`/`jest` are
# tool-call-time only — initialize-time tools/list does NOT
# require any of them).
if ! command -v cargo >/dev/null 2>&1; then
    echo "[INFO] P3-W11-F07: cargo not on PATH; skipping optional run_tests tool-call smoke."
    echo "[OK] P3-W11-F07"
    exit 0
fi

echo "[INFO] P3-W11-F07: optional run_tests tool-call smoke skipped — only the cargo-test integration + tools/list dispatcher inspection are load-bearing per DEC-0025 §Decision point 3 + WO-0082 scope_in #6."

echo "[OK] P3-W11-F07"
exit 0
