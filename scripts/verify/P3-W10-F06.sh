#!/usr/bin/env bash
# Acceptance test for P3-W10-F06 — Git G6 plugin manifest
# health-check + git_log smoke against a tmpdir copy of the
# rust-project fixture.
#
# Behaviour:
#   1. cd to the repo root via `git rev-parse --show-toplevel`.
#   2. Sanity-check `plugins/platform/git/plugin.toml` parses as TOML
#      and asserts the keys named in WO-0075 scope_in #2
#      (plugin.name == "git", plugin.category == "platform",
#      transport.type == "stdio", capabilities.provides starts with
#      "platform.").
#   3. Assert `uvx` is on PATH; otherwise exit 1 with a clear hint
#      pointing at `scripts/devtools/install-git-mcp.sh`.
#   4. Print uvx + the pinned mcp-server-git version for the verifier
#      log.
#   5. Run `cargo test -p ucil-daemon --test g6_plugin_manifests
#      g6_plugin_manifests::git_manifest_health_check` and require
#      the cargo-test summary line "1 passed; 0 failed" or the
#      cargo-nextest equivalent.
#   6. Tool-level smoke (gated by ${UCIL_SKIP_PLATFORM_PLUGIN_E2E:-0},
#      default RUN per WO-0074 §executor #3 + WO-0075 scope_in #8):
#        a. Copy `tests/fixtures/rust-project` into a mktemp -d
#           tmpdir (per WO-0074 §executor #5 — copy BEFORE invoking
#           so the read-only fixture stays pristine).
#        b. Re-init a one-commit git repo inside the tmpdir copy
#           (the outer commit elides any inner .git dir in the
#           checked-in fixture; a fresh init gives git_log something
#           to show).
#        c. Spawn `uvx mcp-server-git@<pin> --repository <tmpdir>`.
#        d. INIT — `initialize` + `notifications/initialized`.
#        e. tools/list — assert ≥3 tools.
#        f. tools/call git_log — assert response contains a hex SHA
#           prefix matching `[0-9a-f]{7,40}` and at least one of
#           `Author` / `Date` / `commit `.
#   7. On all-green prints `[OK] P3-W10-F06` and exits 0; on any
#      failure prints `[FAIL] P3-W10-F06: <reason>` and exits 1.
#
# This script is read-only against the fixture: it copies into a
# tmpdir before invoking the upstream binary (forbidden_paths in
# WO-0075).
set -euo pipefail
IFS=$'\n\t'

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

PINNED_PYPI_VERSION="2026.1.14"
PINNED_PKG_SPEC="mcp-server-git@${PINNED_PYPI_VERSION}"
MANIFEST_PATH="plugins/platform/git/plugin.toml"

# ── Step 0: manifest TOML sanity ───────────────────────────────────────
echo "[INFO] P3-W10-F06: parsing ${MANIFEST_PATH} ..."
if ! python3 -c "
import tomllib
m = tomllib.load(open('${MANIFEST_PATH}', 'rb'))
assert m['plugin']['name'] == 'git', f\"plugin.name != 'git': {m['plugin']['name']!r}\"
assert m['plugin']['category'] == 'platform', f\"plugin.category != 'platform': {m['plugin']['category']!r}\"
assert m['transport']['type'] == 'stdio', f\"transport.type != 'stdio': {m['transport']['type']!r}\"
assert any(c.startswith('platform.') for c in m['capabilities']['provides']), \
    f\"no capability under platform.* namespace: {m['capabilities']['provides']!r}\"
print('manifest-sanity: ok')
"; then
    echo "[FAIL] P3-W10-F06: manifest sanity failed for ${MANIFEST_PATH}" >&2
    exit 1
fi

# ── Prereq: uvx on PATH ────────────────────────────────────────────────
if ! command -v uvx >/dev/null 2>&1; then
    echo "[FAIL] P3-W10-F06: uvx not on PATH." >&2
    echo "  See scripts/devtools/install-git-mcp.sh for install hints." >&2
    exit 1
fi
echo "[INFO] P3-W10-F06: uvx version: $(uvx --version)"
echo "[INFO] P3-W10-F06: git pinned: ${PINNED_PKG_SPEC}"

# ── Step 1: integration test ──────────────────────────────────────────
CARGO_LOG="/tmp/wo-0075-f06-cargo.log"
echo "[INFO] P3-W10-F06: running cargo test g6_plugin_manifests::git_manifest_health_check..."
if ! cargo test -p ucil-daemon --test g6_plugin_manifests \
        g6_plugin_manifests::git_manifest_health_check 2>&1 | tee "${CARGO_LOG}" >/dev/null; then
    echo "[FAIL] P3-W10-F06: cargo test exited non-zero — see ${CARGO_LOG}" >&2
    tail -20 "${CARGO_LOG}" >&2 || true
    exit 1
fi
if ! grep -Eq 'test result: ok\. 1 passed; 0 failed|1 tests run: 1 passed' "${CARGO_LOG}"; then
    echo "[FAIL] P3-W10-F06: cargo test summary line missing in ${CARGO_LOG}" >&2
    tail -20 "${CARGO_LOG}" >&2 || true
    exit 1
fi
echo "[INFO] P3-W10-F06: integration test PASS."

# ── Step 2: tool-level smoke ──────────────────────────────────────────
if [[ "${UCIL_SKIP_PLATFORM_PLUGIN_E2E:-0}" == "1" ]]; then
    echo "[SKIP] P3-W10-F06: tool-level smoke (UCIL_SKIP_PLATFORM_PLUGIN_E2E=1)."
    echo "[OK] P3-W10-F06"
    exit 0
fi

GIT_TMPDIR="$(mktemp -d -t wo-0075-f06-git-XXXXXX)"
SMOKE_LOG="/tmp/wo-0075-f06-smoke.log"
trap 'rm -rf "${GIT_TMPDIR}"' EXIT

# Copy the fixture into the tmpdir BEFORE invoking the upstream
# binary (WO-0074 §executor #5 + WO-0075 scope_in #8).
FIXTURE_COPY="${GIT_TMPDIR}/rust-project"
cp -r "${REPO_ROOT}/tests/fixtures/rust-project" "${FIXTURE_COPY}"

# Re-init a one-commit git repo so git_log has commits to show.
# The outer commit elides any inner .git dir in the checked-in
# fixture, so we fabricate a fresh repo inside the tmpdir copy.
if ! git -C "${FIXTURE_COPY}" rev-parse --git-dir >/dev/null 2>&1; then
    git -C "${FIXTURE_COPY}" init -q
    git -C "${FIXTURE_COPY}" add -A
    git -C "${FIXTURE_COPY}" \
        -c user.name="ucil-wo-0075" \
        -c user.email="ucil@example.invalid" \
        commit -q -m "ucil-wo-0075 fixture-copy seed commit" \
        || true
fi

export PINNED_PKG_SPEC FIXTURE_COPY SMOKE_LOG

# Drive the MCP session from python3 with deadline polling per
# WO-0074 §executor #4. uvx + git operations are wall-time-light
# but python is also more reliable for the JSON-RPC parsing.
if ! python3 - <<'PY'
import json
import os
import re
import subprocess
import sys
import time


def main() -> int:
    pkg_spec = os.environ["PINNED_PKG_SPEC"]
    fixture_copy = os.environ["FIXTURE_COPY"]
    smoke_log = open(os.environ["SMOKE_LOG"], "ab")

    proc = subprocess.Popen(
        ["uvx", pkg_spec, "--repository", fixture_copy],
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
            sys.stderr.write("git did not reply to initialize within 60s\n")
            return 1

        # initialized notification.
        proc.stdin.write(json.dumps({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {},
        }) + "\n")
        proc.stdin.flush()

        # tools/list — assert >=3 tools and contains git_log.
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
            sys.stderr.write("git did not reply to tools/list within 30s\n")
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
        if "git_log" not in tool_names:
            sys.stderr.write(
                f"expected `git_log` in tools/list; got: {tool_names!r}\n"
            )
            return 1

        # tools/call git_log — assert hex SHA + Author/Date/commit.
        proc.stdin.write(json.dumps({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "git_log",
                "arguments": {
                    "repo_path": fixture_copy,
                    "max_count": 5,
                },
            },
        }) + "\n")
        proc.stdin.flush()

        deadline = time.monotonic() + 30.0
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
            sys.stderr.write("git did not reply to tools/call git_log within 30s\n")
            return 1
        if "error" in call_frame:
            sys.stderr.write(
                f"tools/call git_log returned a JSON-RPC error: {call_frame['error']!r}\n"
            )
            return 1
        result = call_frame.get("result") or {}
        content = result.get("content") or []
        text_blocks = [b.get("text", "") for b in content if isinstance(b, dict)]
        joined = "\n".join(text_blocks)
        if not joined.strip():
            sys.stderr.write(
                f"tools/call git_log content is empty: {json.dumps(result)[:300]}\n"
            )
            return 1
        # Heuristic: response should contain a hex SHA prefix and one
        # of Author/Date/commit.
        if not re.search(r"\b[0-9a-f]{7,40}\b", joined):
            sys.stderr.write(
                f"tools/call git_log response missing hex SHA prefix: {joined[:300]}\n"
            )
            return 1
        if not re.search(r"\b(Author|Date|commit )\b", joined):
            sys.stderr.write(
                f"tools/call git_log response missing Author/Date/commit marker: "
                f"{joined[:300]}\n"
            )
            return 1
        print(f"tools/call git_log ok: {len(joined)} chars of commit-shaped output")

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
    echo "[FAIL] P3-W10-F06: tool-level smoke failed - see ${SMOKE_LOG}" >&2
    tail -20 "${SMOKE_LOG}" >&2 || true
    exit 1
fi

echo "[INFO] P3-W10-F06: tool-level smoke PASS."
echo "[OK] P3-W10-F06"
exit 0
