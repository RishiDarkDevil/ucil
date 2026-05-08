#!/usr/bin/env bash
# Acceptance test for P3-W11-F08 — mcp-pytest-runner G8 plugin manifest
# health-check + discover_tests + execute_tests selective-run smoke
# against a tmpdir copy of the python-project fixture (with a
# fabricated conftest.py that injects `src/` onto sys.path so pytest
# can collect the fixture's test modules without an installed-mode
# `pip install -e .` artifact).
#
# Behaviour:
#   1. cd to the repo root via `git rev-parse --show-toplevel`.
#   2. Sanity-check `plugins/testing/mcp-pytest-runner/plugin.toml`
#      parses as TOML and asserts the keys named in WO-0077 scope_in
#      #1 (plugin.name == "mcp-pytest-runner",
#      plugin.category == "testing", transport.type == "stdio",
#      capabilities.provides starts with "testing.").
#   3. Assert `uvx` + `python3` are on PATH; otherwise exit 1 with a
#      clear hint pointing at
#      `scripts/devtools/install-mcp-pytest-runner.sh`.
#   4. Print uvx + the pinned mcp-pytest-runner version for the
#      verifier log.
#   5. Run `cargo test -p ucil-daemon --test g8_plugin_manifests
#      g8_plugin_manifests::mcp_pytest_runner_manifest_health_check`
#      and require the cargo-test summary line "1 passed; 0 failed"
#      or the cargo-nextest equivalent — alternation regex per
#      WO-0042 / WO-0044 / WO-0069 / WO-0072 / WO-0074 / WO-0075 /
#      WO-0076.
#   6. Tool-level smoke (gated by ${UCIL_SKIP_TESTING_PLUGIN_E2E:-0},
#      default RUN per WO-0074 §executor #3 + WO-0076 scope_in #6):
#        a. Copy `tests/fixtures/python-project` into a mktemp -d
#           tmpdir (per WO-0074 §executor #5 + WO-0076 scope_in #5/#16
#           — copy BEFORE invoking so the read-only fixture stays
#           pristine). Skip __pycache__ / .pytest_cache / .ruff_cache
#           sub-trees during the copy so pytest's fresh-collection
#           pass starts from a clean cache.
#        b. Fabricate a conftest.py in the tmpdir copy that prepends
#           `src/` to sys.path so pytest can import the
#           `python_project` package without an editable install.
#        c. Spawn `uvx mcp-pytest-runner@<pin>` from the tmpdir as
#           cwd.
#        d. INIT - `initialize` + `notifications/initialized`.
#        e. tools/list - assert `discover_tests` AND `execute_tests`
#           are advertised (load-bearing - verifies the manifest's
#           pin matches the upstream's tool surface).
#        f. tools/call discover_tests - assert response carries a
#           non-empty list of canonical pytest node IDs in
#           `tests/test_x.py::test_y` form per F08 spec.
#        g. tools/call execute_tests - run a SUBSET of discovered
#           node IDs (1-3 specific ones from one test file) and
#           assert the structured pytest result carries a
#           `summary.passed >= 1` per F08 spec line "selective re-
#           run by node ID".
#   7. On all-green prints `[OK] P3-W11-F08` and exits 0; on any
#      failure prints `[FAIL] P3-W11-F08: <reason>` and exits 1.
#
# Per WO-0074 §executor #4: the tool-level smoke uses python polling
# with a deadline rather than bash `sleep` so wall-time-sensitive
# stdio JSON-RPC reads complete reliably even on slow workstations
# (pytest collection of a 159-test fixture takes several seconds on
# a cold cache).
set -euo pipefail
IFS=$'\n\t'

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

PINNED_PYPI_VERSION="0.2.1"
PINNED_PKG_SPEC="mcp-pytest-runner@${PINNED_PYPI_VERSION}"
MANIFEST_PATH="plugins/testing/mcp-pytest-runner/plugin.toml"

# Step 0: manifest TOML sanity (cheap pre-flight).
echo "[INFO] P3-W11-F08: parsing ${MANIFEST_PATH} ..."
if ! python3 -c "
import tomllib
m = tomllib.load(open('${MANIFEST_PATH}', 'rb'))
assert m['plugin']['name'] == 'mcp-pytest-runner', f\"plugin.name != 'mcp-pytest-runner': {m['plugin']['name']!r}\"
assert m['plugin']['category'] == 'testing', f\"plugin.category != 'testing': {m['plugin']['category']!r}\"
assert m['transport']['type'] == 'stdio', f\"transport.type != 'stdio': {m['transport']['type']!r}\"
assert any(c.startswith('testing.') for c in m['capabilities']['provides']), \
    f\"no capability under testing.* namespace: {m['capabilities']['provides']!r}\"
print('manifest-sanity: ok')
"; then
    echo "[FAIL] P3-W11-F08: manifest sanity failed for ${MANIFEST_PATH}" >&2
    exit 1
fi

# Prereq: uvx + python3 on PATH.
if ! command -v uvx >/dev/null 2>&1; then
    echo "[FAIL] P3-W11-F08: uvx not on PATH." >&2
    echo "  See scripts/devtools/install-mcp-pytest-runner.sh for install hints." >&2
    exit 1
fi
if ! command -v python3 >/dev/null 2>&1; then
    echo "[FAIL] P3-W11-F08: python3 not on PATH." >&2
    exit 1
fi
echo "[INFO] P3-W11-F08: uvx version: $(uvx --version | head -1)"
echo "[INFO] P3-W11-F08: mcp-pytest-runner pinned: ${PINNED_PKG_SPEC}"

# Step 1: integration test (real subprocess, real JSON-RPC).
CARGO_LOG="/tmp/wo-0077-f08-cargo.log"
echo "[INFO] P3-W11-F08: running cargo test g8_plugin_manifests::mcp_pytest_runner_manifest_health_check..."
if ! cargo test -p ucil-daemon --test g8_plugin_manifests \
        g8_plugin_manifests::mcp_pytest_runner_manifest_health_check 2>&1 | tee "${CARGO_LOG}" >/dev/null; then
    echo "[FAIL] P3-W11-F08: cargo test exited non-zero - see ${CARGO_LOG}" >&2
    tail -20 "${CARGO_LOG}" >&2 || true
    exit 1
fi
if ! grep -Eq 'test result: ok\. 1 passed; 0 failed|1 tests run: 1 passed' "${CARGO_LOG}"; then
    echo "[FAIL] P3-W11-F08: cargo test summary line missing in ${CARGO_LOG}" >&2
    tail -20 "${CARGO_LOG}" >&2 || true
    exit 1
fi
echo "[INFO] P3-W11-F08: integration test PASS."

# Step 2: tool-level smoke.
if [[ "${UCIL_SKIP_TESTING_PLUGIN_E2E:-0}" == "1" ]]; then
    echo "[SKIP] P3-W11-F08: tool-level smoke (UCIL_SKIP_TESTING_PLUGIN_E2E=1)."
    echo "[OK] P3-W11-F08"
    exit 0
fi

PYTEST_TMPDIR="$(mktemp -d -t wo-0077-f08-pytest-XXXXXX)"
SMOKE_LOG="/tmp/wo-0077-f08-smoke.log"
trap 'rm -rf "${PYTEST_TMPDIR}"' EXIT

# Copy the python-project fixture into the tmpdir BEFORE invoking
# the upstream binary (per WO-0074 executor #5 + WO-0076 scope_in
# #5/#16). The fixture's source remains read-only; we only operate
# against the tmpdir copy. Skip transient pycache / pytest-cache /
# ruff-cache sub-trees (those are gitignored in the fixture but may
# be present from previous local runs).
FIXTURE_COPY="${PYTEST_TMPDIR}/python-project"
mkdir -p "${FIXTURE_COPY}"
( cd "${REPO_ROOT}/tests/fixtures/python-project" && \
  find . \( -name '__pycache__' -o -name '.pytest_cache' -o -name '.ruff_cache' \) -prune -o \
    -type f -print | \
  while IFS= read -r f; do
    rel="${f#./}"
    dst_dir="${FIXTURE_COPY}/$(dirname "${rel}")"
    mkdir -p "${dst_dir}"
    cp -p "${f}" "${dst_dir}/"
  done )

# Fabricate conftest.py in the tmpdir copy that prepends `src/` to
# sys.path. The fixture's pyproject.toml declares the python_project
# package under src/python_project/ but does NOT carry an installed-
# mode `pip install -e .` artifact, so pytest cannot import the test
# modules without this nudge. The conftest is in the tmpdir copy
# only (NOT in the read-only fixture).
cat > "${FIXTURE_COPY}/conftest.py" <<'CONFTEST'
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent / "src"))
CONFTEST

export PINNED_PKG_SPEC FIXTURE_COPY SMOKE_LOG

# Drive the MCP session from python3 with deadline polling per
# WO-0074 executor #4. mcp-pytest-runner's pytest collection on a
# 159-test fixture takes several seconds on a cold cache.
if ! python3 - <<'PY'
import json
import os
import subprocess
import sys
import time


def main() -> int:
    pkg_spec = os.environ["PINNED_PKG_SPEC"]
    fixture_copy = os.environ["FIXTURE_COPY"]
    smoke_log = open(os.environ["SMOKE_LOG"], "ab")

    proc = subprocess.Popen(
        ["uvx", pkg_spec],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=smoke_log,
        text=True,
        bufsize=1,
        cwd=fixture_copy,
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
                "clientInfo": {"name": "ucil-wo-0077", "version": "0.1.0"},
            },
        }
        assert proc.stdin is not None
        proc.stdin.write(json.dumps(init_msg) + "\n")
        proc.stdin.flush()

        deadline = time.monotonic() + 120.0
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
            sys.stderr.write("mcp-pytest-runner did not reply to initialize within 120s\n")
            return 1

        # initialized notification.
        proc.stdin.write(json.dumps({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {},
        }) + "\n")
        proc.stdin.flush()

        # tools/list - assert discover_tests + execute_tests are advertised.
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
            sys.stderr.write("mcp-pytest-runner did not reply to tools/list within 30s\n")
            return 1
        if "error" in tl_frame:
            sys.stderr.write(f"tools/list returned a JSON-RPC error: {tl_frame['error']!r}\n")
            return 1
        tools = tl_frame.get("result", {}).get("tools", [])
        tool_names = [t.get("name") for t in tools]
        print(f"tools/list ok: {len(tools)} tools advertised; names: {tool_names}")
        for required in ("discover_tests", "execute_tests"):
            if required not in tool_names:
                sys.stderr.write(
                    f"expected `{required}` in tools/list; got: {tool_names!r}\n"
                )
                return 1

        # tools/call discover_tests - pass the absolute path to the
        # tmpdir and assert response is structured with a non-empty
        # list of canonical pytest node IDs per F08 spec.
        proc.stdin.write(json.dumps({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "discover_tests",
                "arguments": {
                    "path": fixture_copy,
                },
            },
        }) + "\n")
        proc.stdin.flush()

        deadline = time.monotonic() + 90.0
        discover_frame = None
        while time.monotonic() < deadline and discover_frame is None:
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
                discover_frame = frame
                break
        if discover_frame is None:
            sys.stderr.write("mcp-pytest-runner did not reply to discover_tests within 90s\n")
            return 1
        if "error" in discover_frame:
            sys.stderr.write(
                f"tools/call discover_tests returned a JSON-RPC error: {discover_frame['error']!r}\n"
            )
            return 1
        # Walk result.content and find the JSON-blob text block.
        result = discover_frame.get("result") or {}
        content = result.get("content") or []
        text_blocks = [b.get("text", "") for b in content if isinstance(b, dict)]
        discover_payload = None
        for block in text_blocks:
            try:
                parsed = json.loads(block)
            except json.JSONDecodeError:
                continue
            if isinstance(parsed, dict) and "tests" in parsed:
                discover_payload = parsed
                break
        if discover_payload is None:
            sys.stderr.write(
                f"discover_tests response missing structured tests JSON; "
                f"text blocks: {[b[:200] for b in text_blocks]}\n"
            )
            return 1
        tests = discover_payload.get("tests") or []
        if not tests:
            errs = discover_payload.get("collection_errors") or []
            sys.stderr.write(
                f"discover_tests returned 0 tests; collection_errors: {errs!r}\n"
            )
            return 1
        # Verify each test entry has the canonical pytest node-ID
        # format (path::function or path::Class::method) per F08
        # spec line "pytest hierarchical test discovery".
        node_ids = []
        for t in tests:
            if not isinstance(t, dict):
                sys.stderr.write(f"unexpected test entry shape: {t!r}\n")
                return 1
            nid = t.get("node_id")
            if not isinstance(nid, str) or "::" not in nid:
                sys.stderr.write(
                    f"test entry missing canonical node_id: {t!r}\n"
                )
                return 1
            node_ids.append(nid)
        print(
            f"discover_tests ok: {len(node_ids)} canonical node IDs; "
            f"first 3: {node_ids[:3]!r}"
        )

        # tools/call execute_tests - run a SUBSET of node IDs (the
        # first 3 from one test file). Per F08 spec line "selective
        # re-run by node ID verified on the fixture python-project".
        # Pick the first 3 node IDs that share a common test file
        # prefix so the selective filter is non-trivial.
        first_file_prefix = node_ids[0].split("::", 1)[0]
        same_file = [n for n in node_ids if n.startswith(first_file_prefix + "::")][:3]
        subset = same_file or [node_ids[0]]
        print(f"selected subset for execute_tests: {subset!r}")
        proc.stdin.write(json.dumps({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "execute_tests",
                "arguments": {
                    "node_ids": subset,
                    "verbosity": 1,
                },
            },
        }) + "\n")
        proc.stdin.flush()

        deadline = time.monotonic() + 120.0
        exec_frame = None
        while time.monotonic() < deadline and exec_frame is None:
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
                exec_frame = frame
                break
        if exec_frame is None:
            sys.stderr.write("mcp-pytest-runner did not reply to execute_tests within 120s\n")
            return 1
        if "error" in exec_frame:
            sys.stderr.write(
                f"tools/call execute_tests returned a JSON-RPC error: {exec_frame['error']!r}\n"
            )
            return 1
        # Walk result.content and find the JSON-blob text block.
        result = exec_frame.get("result") or {}
        content = result.get("content") or []
        text_blocks = [b.get("text", "") for b in content if isinstance(b, dict)]
        exec_payload = None
        for block in text_blocks:
            try:
                parsed = json.loads(block)
            except json.JSONDecodeError:
                continue
            if isinstance(parsed, dict) and "summary" in parsed:
                exec_payload = parsed
                break
        if exec_payload is None:
            sys.stderr.write(
                f"execute_tests response missing structured summary JSON; "
                f"text blocks: {[b[:200] for b in text_blocks]}\n"
            )
            return 1
        summary = exec_payload.get("summary") or {}
        for required in ("total", "passed", "failed", "skipped"):
            if required not in summary:
                sys.stderr.write(
                    f"summary missing '{required}' key: {list(summary.keys())}\n"
                )
                return 1
        passed = summary.get("passed", 0)
        total = summary.get("total", 0)
        if not (isinstance(passed, int) and passed >= 1):
            sys.stderr.write(
                f"summary.passed < 1: {summary!r}; subset was {subset!r}\n"
            )
            return 1
        print(
            f"execute_tests ok: total={total} passed={passed} "
            f"failed={summary.get('failed')} skipped={summary.get('skipped')}"
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
    echo "[FAIL] P3-W11-F08: tool-level smoke failed - see ${SMOKE_LOG}" >&2
    tail -30 "${SMOKE_LOG}" >&2 || true
    exit 1
fi

echo "[INFO] P3-W11-F08: tool-level smoke PASS."
echo "[OK] P3-W11-F08"
exit 0
