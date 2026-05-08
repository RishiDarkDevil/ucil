#!/usr/bin/env bash
# Acceptance test for P3-W10-F05 — GitHub G6 plugin manifest
# health-check + (gated) tools/call smoke against the real
# api.github.com surface.
#
# Behaviour:
#   1. cd to the repo root via `git rev-parse --show-toplevel`.
#   2. Sanity-check `plugins/platform/github/plugin.toml` parses as
#      TOML and asserts the keys named in WO-0075 scope_in #1
#      (plugin.name == "github", plugin.category == "platform",
#      transport.type == "stdio", capabilities.provides starts with
#      "platform.").
#   3. Assert `npx` is on PATH; otherwise exit 1 with a clear hint
#      pointing at `scripts/devtools/install-github-mcp.sh`.
#   4. Print npx + the pinned @modelcontextprotocol/server-github
#      version for the verifier log.
#   5. Run `cargo test -p ucil-daemon --test g6_plugin_manifests
#      g6_plugin_manifests::github_manifest_health_check` and
#      require the cargo-test summary line "1 passed; 0 failed" or
#      the cargo-nextest equivalent — alternation regex per
#      WO-0042 / WO-0043 / WO-0044 / WO-0069 / WO-0072 / WO-0074.
#   6. Tool-level smoke (gated by ${UCIL_SKIP_PLATFORM_PLUGIN_E2E:-0},
#      default RUN per WO-0074 §executor #3 + WO-0075 scope_in #7):
#        a. Spawn the GitHub MCP server via the canonical `npx`
#           command from the manifest.
#        b. INIT — `initialize` + `notifications/initialized`.
#        c. tools/list — assert ≥3 tools (load-bearing — works
#           WITHOUT a PAT).
#        d. tools/call (API-key-gated): if
#           `${GITHUB_PERSONAL_ACCESS_TOKEN}` is set, send a
#           `tools/call search_repositories` with a small public
#           query and assert no JSON-RPC error. Else log
#           `[SKIP] tools/call gated on GITHUB_PERSONAL_ACCESS_TOKEN`
#           to stderr and proceed.
#   7. On all-green prints `[OK] P3-W10-F05` and exits 0; on any
#      failure prints `[FAIL] P3-W10-F05: <reason>` and exits 1.
#
# Per WO-0074 §executor #4 lesson: the tool-level smoke uses python
# polling with a deadline rather than bash `sleep` so wall-time-
# sensitive HTTPS round-trips to api.github.com complete reliably
# (HTTPS RTT can spike beyond 1s on flaky networks).
set -euo pipefail
IFS=$'\n\t'

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

PINNED_NPM_VERSION="2025.4.8"
PINNED_PKG_SPEC="@modelcontextprotocol/server-github@${PINNED_NPM_VERSION}"
MANIFEST_PATH="plugins/platform/github/plugin.toml"

# ── Step 0: manifest TOML sanity (cheap pre-flight) ────────────────────
echo "[INFO] P3-W10-F05: parsing ${MANIFEST_PATH} ..."
if ! python3 -c "
import tomllib
m = tomllib.load(open('${MANIFEST_PATH}', 'rb'))
assert m['plugin']['name'] == 'github', f\"plugin.name != 'github': {m['plugin']['name']!r}\"
assert m['plugin']['category'] == 'platform', f\"plugin.category != 'platform': {m['plugin']['category']!r}\"
assert m['transport']['type'] == 'stdio', f\"transport.type != 'stdio': {m['transport']['type']!r}\"
assert any(c.startswith('platform.') for c in m['capabilities']['provides']), \
    f\"no capability under platform.* namespace: {m['capabilities']['provides']!r}\"
print('manifest-sanity: ok')
"; then
    echo "[FAIL] P3-W10-F05: manifest sanity failed for ${MANIFEST_PATH}" >&2
    exit 1
fi

# ── Prereq: npx on PATH ────────────────────────────────────────────────
if ! command -v npx >/dev/null 2>&1; then
    echo "[FAIL] P3-W10-F05: npx not on PATH." >&2
    echo "  See scripts/devtools/install-github-mcp.sh for install hints." >&2
    exit 1
fi
echo "[INFO] P3-W10-F05: npx version: $(npx --version)"
echo "[INFO] P3-W10-F05: github pinned: ${PINNED_PKG_SPEC}"

# ── Step 1: integration test (real subprocess, real JSON-RPC) ─────────
CARGO_LOG="/tmp/wo-0075-f05-cargo.log"
echo "[INFO] P3-W10-F05: running cargo test g6_plugin_manifests::github_manifest_health_check..."
if ! cargo test -p ucil-daemon --test g6_plugin_manifests \
        g6_plugin_manifests::github_manifest_health_check 2>&1 | tee "${CARGO_LOG}" >/dev/null; then
    echo "[FAIL] P3-W10-F05: cargo test exited non-zero — see ${CARGO_LOG}" >&2
    tail -20 "${CARGO_LOG}" >&2 || true
    exit 1
fi
if ! grep -Eq 'test result: ok\. 1 passed; 0 failed|1 tests run: 1 passed' "${CARGO_LOG}"; then
    echo "[FAIL] P3-W10-F05: cargo test summary line missing in ${CARGO_LOG}" >&2
    tail -20 "${CARGO_LOG}" >&2 || true
    exit 1
fi
echo "[INFO] P3-W10-F05: integration test PASS."

# ── Step 2: tool-level smoke ──────────────────────────────────────────
if [[ "${UCIL_SKIP_PLATFORM_PLUGIN_E2E:-0}" == "1" ]]; then
    echo "[SKIP] P3-W10-F05: tool-level smoke (UCIL_SKIP_PLATFORM_PLUGIN_E2E=1)."
    echo "[OK] P3-W10-F05"
    exit 0
fi

SMOKE_LOG="/tmp/wo-0075-f05-smoke.log"
export PINNED_PKG_SPEC SMOKE_LOG

# Drive the MCP session from python3 — wall-time-sensitive HTTPS
# round-trips to api.github.com need deadline-based polling per
# WO-0074 §executor #4.
if ! python3 - <<'PY'
import json
import os
import subprocess
import sys
import time


def main() -> int:
    pkg_spec = os.environ["PINNED_PKG_SPEC"]
    smoke_log = open(os.environ["SMOKE_LOG"], "ab")
    pat_present = bool(os.environ.get("GITHUB_PERSONAL_ACCESS_TOKEN"))

    proc = subprocess.Popen(
        ["npx", "-y", pkg_spec],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=smoke_log,
        text=True,
        bufsize=1,
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
                "clientInfo": {"name": "ucil-wo-0075", "version": "0.1.0"},
            },
        }
        assert proc.stdin is not None
        proc.stdin.write(json.dumps(init_msg) + "\n")
        proc.stdin.flush()

        deadline = time.monotonic() + 60.0
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
            sys.stderr.write("github did not reply to initialize within 60s\n")
            return 1

        # initialized notification.
        proc.stdin.write(json.dumps({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {},
        }) + "\n")
        proc.stdin.flush()

        # tools/list — load-bearing assertion (works without PAT).
        proc.stdin.write(json.dumps({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {},
        }) + "\n")
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
            sys.stderr.write("github did not reply to tools/list within 30s\n")
            return 1
        if "error" in tl_frame:
            sys.stderr.write(f"tools/list returned a JSON-RPC error: {tl_frame['error']!r}\n")
            return 1
        tools = tl_frame.get("result", {}).get("tools", [])
        if len(tools) < 3:
            sys.stderr.write(f"tools/list returned <3 tools: {len(tools)}\n")
            return 1
        tool_names = [t.get("name") for t in tools]
        print(f"tools/list ok: {len(tools)} tools advertised; sample: {tool_names[:5]}")
        if "list_pull_requests" not in tool_names:
            sys.stderr.write(
                f"expected `list_pull_requests` in tools/list; got: {tool_names!r}\n"
            )
            return 1

        # tools/call (API-key-gated) — search_repositories with a small
        # public query that exercises the GraphQL surface.
        if not pat_present:
            sys.stderr.write(
                "[SKIP] tools/call gated on GITHUB_PERSONAL_ACCESS_TOKEN — env unset\n"
            )
            return 0

        proc.stdin.write(json.dumps({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "search_repositories",
                "arguments": {
                    "query": "ucil language:rust",
                    "perPage": 1,
                },
            },
        }) + "\n")
        proc.stdin.flush()

        deadline = time.monotonic() + 60.0
        call_frame = None
        while time.monotonic() < deadline and call_frame is None:
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
            if frame.get("id") == 3:
                call_frame = frame
                break
        if call_frame is None:
            sys.stderr.write("github did not reply to tools/call within 60s\n")
            return 1
        if "error" in call_frame:
            sys.stderr.write(
                f"tools/call search_repositories returned a JSON-RPC error: "
                f"{call_frame['error']!r}\n"
            )
            return 1
        result = call_frame.get("result") or {}
        content = result.get("content") or []
        if not content:
            sys.stderr.write(
                f"tools/call search_repositories result.content is empty: "
                f"{json.dumps(result)[:300]}\n"
            )
            return 1
        print("tools/call search_repositories ok: PAT-backed surface live.")
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
    echo "[FAIL] P3-W10-F05: tool-level smoke failed - see ${SMOKE_LOG}" >&2
    tail -20 "${SMOKE_LOG}" >&2 || true
    exit 1
fi

echo "[INFO] P3-W10-F05: tool-level smoke PASS."
echo "[OK] P3-W10-F05"
exit 0
