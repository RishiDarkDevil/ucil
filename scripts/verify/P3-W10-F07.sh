#!/usr/bin/env bash
# Acceptance test for P3-W10-F07 — Filesystem G6 plugin manifest
# health-check + read_file + list_directory smoke against a freshly-
# fabricated tmpdir.
#
# Behaviour:
#   1. cd to the repo root via `git rev-parse --show-toplevel`.
#   2. Sanity-check `plugins/platform/filesystem/plugin.toml` parses
#      as TOML and asserts the keys named in WO-0075 scope_in #3
#      (plugin.name == "filesystem", plugin.category == "platform",
#      transport.type == "stdio", capabilities.provides starts with
#      "platform.").
#   3. Assert `npx` is on PATH; otherwise exit 1 with a hint
#      pointing at `scripts/devtools/install-filesystem-mcp.sh`.
#   4. Print npx + the pinned @modelcontextprotocol/server-filesystem
#      version for the verifier log.
#   5. Run `cargo test -p ucil-daemon --test g6_plugin_manifests
#      g6_plugin_manifests::filesystem_manifest_health_check` and
#      require the cargo-test summary line "1 passed; 0 failed" or
#      the cargo-nextest equivalent.
#   6. Tool-level smoke (gated by ${UCIL_SKIP_PLATFORM_PLUGIN_E2E:-0},
#      default RUN per WO-0074 §executor #3 + WO-0075 scope_in #9):
#        a. Create a `mktemp -d` tmpdir.
#        b. Populate the tmpdir with 3 small known-content text files
#           (NOT a fixture copy — fabricate from scratch for
#           hermeticity per WO-0075 scope_in #9).
#        c. Spawn `npx -y @modelcontextprotocol/server-filesystem@<pin>
#                  <tmpdir>` (the tmpdir is the positional allowed-
#           path arg — only paths under it are accessible).
#        d. INIT — `initialize` + `notifications/initialized`.
#        e. tools/list — assert ≥3 tools.
#        f. tools/call read_file — assert response content matches
#           the known content of one of the populated files.
#        g. tools/call list_directory — assert the response lists
#           the 3 populated files.
#   7. CRITICAL: the smoke MUST be read-only. Do NOT exercise
#      write_file / move_file / edit_file / create_directory upstream
#      tools — F07 spec line is "secure file read and directory
#      listing" (per WO-0075 scope_in #9 + scope_out #13).
#   8. On all-green prints `[OK] P3-W10-F07` and exits 0; on any
#      failure prints `[FAIL] P3-W10-F07: <reason>` and exits 1.
set -euo pipefail
IFS=$'\n\t'

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

PINNED_NPM_VERSION="2026.1.14"
PINNED_PKG_SPEC="@modelcontextprotocol/server-filesystem@${PINNED_NPM_VERSION}"
MANIFEST_PATH="plugins/platform/filesystem/plugin.toml"

# ── Step 0: manifest TOML sanity ───────────────────────────────────────
echo "[INFO] P3-W10-F07: parsing ${MANIFEST_PATH} ..."
if ! python3 -c "
import tomllib
m = tomllib.load(open('${MANIFEST_PATH}', 'rb'))
assert m['plugin']['name'] == 'filesystem', f\"plugin.name != 'filesystem': {m['plugin']['name']!r}\"
assert m['plugin']['category'] == 'platform', f\"plugin.category != 'platform': {m['plugin']['category']!r}\"
assert m['transport']['type'] == 'stdio', f\"transport.type != 'stdio': {m['transport']['type']!r}\"
assert any(c.startswith('platform.') for c in m['capabilities']['provides']), \
    f\"no capability under platform.* namespace: {m['capabilities']['provides']!r}\"
print('manifest-sanity: ok')
"; then
    echo "[FAIL] P3-W10-F07: manifest sanity failed for ${MANIFEST_PATH}" >&2
    exit 1
fi

# ── Prereq: npx on PATH ────────────────────────────────────────────────
if ! command -v npx >/dev/null 2>&1; then
    echo "[FAIL] P3-W10-F07: npx not on PATH." >&2
    echo "  See scripts/devtools/install-filesystem-mcp.sh for install hints." >&2
    exit 1
fi
echo "[INFO] P3-W10-F07: npx version: $(npx --version)"
echo "[INFO] P3-W10-F07: filesystem pinned: ${PINNED_PKG_SPEC}"

# ── Step 1: integration test ──────────────────────────────────────────
CARGO_LOG="/tmp/wo-0075-f07-cargo.log"
echo "[INFO] P3-W10-F07: running cargo test g6_plugin_manifests::filesystem_manifest_health_check..."
if ! cargo test -p ucil-daemon --test g6_plugin_manifests \
        g6_plugin_manifests::filesystem_manifest_health_check 2>&1 | tee "${CARGO_LOG}" >/dev/null; then
    echo "[FAIL] P3-W10-F07: cargo test exited non-zero — see ${CARGO_LOG}" >&2
    tail -20 "${CARGO_LOG}" >&2 || true
    exit 1
fi
if ! grep -Eq 'test result: ok\. 1 passed; 0 failed|1 tests run: 1 passed' "${CARGO_LOG}"; then
    echo "[FAIL] P3-W10-F07: cargo test summary line missing in ${CARGO_LOG}" >&2
    tail -20 "${CARGO_LOG}" >&2 || true
    exit 1
fi
echo "[INFO] P3-W10-F07: integration test PASS."

# ── Step 2: tool-level smoke ──────────────────────────────────────────
if [[ "${UCIL_SKIP_PLATFORM_PLUGIN_E2E:-0}" == "1" ]]; then
    echo "[SKIP] P3-W10-F07: tool-level smoke (UCIL_SKIP_PLATFORM_PLUGIN_E2E=1)."
    echo "[OK] P3-W10-F07"
    exit 0
fi

FS_TMPDIR="$(mktemp -d -t wo-0075-f07-fs-XXXXXX)"
SMOKE_LOG="/tmp/wo-0075-f07-smoke.log"
trap 'rm -rf "${FS_TMPDIR}"' EXIT

# Fabricate 3 known-content text files in the tmpdir (NOT a fixture
# copy — fabricate from scratch per WO-0075 scope_in #9 for
# hermeticity).
HELLO_CONTENT="hello, ucil wo-0075!"
README_CONTENT="# G6 Filesystem F07 Test"
DATA_CONTENT='{"key":"value","ucil":"wo-0075"}'
printf '%s\n' "${HELLO_CONTENT}" >"${FS_TMPDIR}/hello.txt"
printf '%s\n' "${README_CONTENT}" >"${FS_TMPDIR}/readme.md"
printf '%s\n' "${DATA_CONTENT}" >"${FS_TMPDIR}/data.json"

export PINNED_PKG_SPEC FS_TMPDIR SMOKE_LOG HELLO_CONTENT README_CONTENT DATA_CONTENT

if ! python3 - <<'PY'
import json
import os
import subprocess
import sys
import time


def main() -> int:
    pkg_spec = os.environ["PINNED_PKG_SPEC"]
    fs_tmpdir = os.environ["FS_TMPDIR"]
    hello_content = os.environ["HELLO_CONTENT"]
    smoke_log = open(os.environ["SMOKE_LOG"], "ab")

    proc = subprocess.Popen(
        ["npx", "-y", pkg_spec, fs_tmpdir],
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
            sys.stderr.write("filesystem did not reply to initialize within 60s\n")
            return 1

        proc.stdin.write(json.dumps({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {},
        }) + "\n")
        proc.stdin.flush()

        # tools/list — assert >=3 tools and contains read_file +
        # list_directory.
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
            sys.stderr.write("filesystem did not reply to tools/list within 30s\n")
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
        if "read_file" not in tool_names:
            sys.stderr.write(
                f"expected `read_file` in tools/list; got: {tool_names!r}\n"
            )
            return 1
        if "list_directory" not in tool_names:
            sys.stderr.write(
                f"expected `list_directory` in tools/list; got: {tool_names!r}\n"
            )
            return 1

        # tools/call read_file — assert content matches.
        # READ-ONLY INVARIANT: do NOT exercise write_file / edit_file /
        # move_file / create_directory (WO-0075 scope_out #13).
        hello_path = os.path.join(fs_tmpdir, "hello.txt")
        proc.stdin.write(json.dumps({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "read_file",
                "arguments": {"path": hello_path},
            },
        }) + "\n")
        proc.stdin.flush()

        deadline = time.monotonic() + 30.0
        rf_frame = None
        while time.monotonic() < deadline and rf_frame is None:
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
                rf_frame = frame
                break
        if rf_frame is None:
            sys.stderr.write("filesystem did not reply to tools/call read_file within 30s\n")
            return 1
        if "error" in rf_frame:
            sys.stderr.write(
                f"tools/call read_file returned a JSON-RPC error: {rf_frame['error']!r}\n"
            )
            return 1
        result = rf_frame.get("result") or {}
        content = result.get("content") or []
        text_blocks = [b.get("text", "") for b in content if isinstance(b, dict)]
        joined = "\n".join(text_blocks)
        if hello_content not in joined:
            sys.stderr.write(
                f"tools/call read_file content does not contain expected text "
                f"{hello_content!r}; got: {joined[:300]!r}\n"
            )
            return 1
        print(f"tools/call read_file ok: matched expected content for hello.txt")

        # tools/call list_directory — assert the 3 populated files.
        proc.stdin.write(json.dumps({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "list_directory",
                "arguments": {"path": fs_tmpdir},
            },
        }) + "\n")
        proc.stdin.flush()

        deadline = time.monotonic() + 30.0
        ld_frame = None
        while time.monotonic() < deadline and ld_frame is None:
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
            if frame.get("id") == 4:
                ld_frame = frame
                break
        if ld_frame is None:
            sys.stderr.write("filesystem did not reply to tools/call list_directory within 30s\n")
            return 1
        if "error" in ld_frame:
            sys.stderr.write(
                f"tools/call list_directory returned a JSON-RPC error: {ld_frame['error']!r}\n"
            )
            return 1
        ld_result = ld_frame.get("result") or {}
        ld_content = ld_result.get("content") or []
        ld_text = "\n".join(b.get("text", "") for b in ld_content if isinstance(b, dict))
        for fname in ("hello.txt", "readme.md", "data.json"):
            if fname not in ld_text:
                sys.stderr.write(
                    f"tools/call list_directory missing {fname!r}; got: {ld_text[:500]!r}\n"
                )
                return 1
        print(f"tools/call list_directory ok: all 3 populated files listed")

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
    echo "[FAIL] P3-W10-F07: tool-level smoke failed - see ${SMOKE_LOG}" >&2
    tail -20 "${SMOKE_LOG}" >&2 || true
    exit 1
fi

echo "[INFO] P3-W10-F07: tool-level smoke PASS."
echo "[OK] P3-W10-F07"
exit 0
