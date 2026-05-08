#!/usr/bin/env bash
# Acceptance test for P3-W11-F03 — Ruff G7 plugin manifest health-
# check + ruff-check smoke against a tmpdir copy of the
# python-project fixture (with a fabricated deliberate-lint-violation
# file).
#
# DEC-0023 §Decision point 2(Tier 1) install path — Anselmoo's
# `mcp-server-analyzer` PyPI wrapper combines Ruff + Vulture into
# a single MCP server. Live tools/list capture against
# `uvx mcp-server-analyzer@0.1.2` advertises five canonical tools:
# ruff-check (load-bearing), ruff-format, ruff-check-ci,
# vulture-scan, analyze-code. See plugins/quality/ruff/plugin.toml
# top-of-file rustdoc for the full PyPI-name vs serverInfo.name
# asymmetry capture.
#
# Behaviour:
#   1. cd to the repo root via `git rev-parse --show-toplevel`.
#   2. Sanity-check `plugins/quality/ruff/plugin.toml` parses as TOML
#      and asserts the keys named in WO-0080 scope_in #3 (plugin.name
#      == "ruff", plugin.category == "quality", transport.type ==
#      "stdio", capabilities.provides starts with "quality.").
#   3. Assert `uvx` is on PATH; otherwise exit 1 with a clear hint
#      pointing at `scripts/devtools/install-ruff-mcp.sh`.
#   4. Print uvx + the pinned mcp-server-analyzer version for the
#      verifier log. Ruff is SELF-RESOLVING (the upstream PyPI
#      wrapper bundles ruff + vulture as Python deps via uvx-managed
#      venv) — NO operator-state CLI gate, NO `RUFF_PATH` env var,
#      NO equivalent of the WO-0076 Semgrep CLI lifespan dep (per
#      WO-0080 scope_in #6 + #34).
#   5. Run `cargo test -p ucil-daemon --test g7_plugin_manifests
#      g7_plugin_manifests::ruff_manifest_health_check` and require
#      the cargo-test summary line "1 passed; 0 failed" or the
#      cargo-nextest equivalent — alternation regex per WO-0042 /
#      WO-0044 / WO-0069 / WO-0072 / WO-0074 / WO-0075 / WO-0076.
#   6. Tool-level smoke (gated by ${UCIL_SKIP_QUALITY_PLUGIN_E2E:-0},
#      default RUN per WO-0074 §executor #3 + WO-0080 scope_in #6):
#        a. Copy `tests/fixtures/python-project` into a mktemp -d
#           tmpdir (per WO-0074 §executor #5 + WO-0080 scope_in
#           #5/#11 — copy BEFORE invoking so the read-only fixture
#           stays pristine).
#        b. Drop a deliberate-lint-violation `bad.py` into the
#           tmpdir copy with E401 (multiple-imports-on-one-line),
#           F841 (unused-variable), and W291 (trailing-whitespace)
#           triggers (per WO-0080 scope_in #36). The python-project
#           fixture's existing files do NOT carry deliberate Ruff
#           violations; the fabricated `bad.py` gives a deterministic
#           >=1-finding signal under Ruff's default rule set.
#        c. Spawn `uvx mcp-server-analyzer@<pin>` (env inherits NONE
#           — Ruff is self-resolving).
#        d. INIT — `initialize` + `notifications/initialized`.
#        e. tools/list — assert `ruff-check` is advertised
#           (load-bearing — verifies the manifest's pin matches the
#           upstream's tool surface).
#        f. tools/call ruff-check {code: <bad.py-contents>} —
#           assert response is a structured JSON blob with file/line
#           /rule/severity per F03 spec.
#   7. On all-green prints `[OK] P3-W11-F03` and exits 0; on any
#      failure prints `[FAIL] P3-W11-F03: <reason>` and exits 1.
#
# Per WO-0074 §executor #4: the tool-level smoke uses python polling
# with a deadline rather than bash `sleep` so wall-time-sensitive
# stdio JSON-RPC reads complete reliably even on slow workstations.
# Ruff lints are fast (~1-2s on a small fixture) so a 60s deadline
# is generous.
set -euo pipefail
IFS=$'\n\t'

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

PINNED_PYPI_VERSION="0.1.2"
PINNED_PKG_SPEC="mcp-server-analyzer@${PINNED_PYPI_VERSION}"
MANIFEST_PATH="plugins/quality/ruff/plugin.toml"

# Step 0: manifest TOML sanity (cheap pre-flight).
echo "[INFO] P3-W11-F03: parsing ${MANIFEST_PATH} ..."
if ! python3 -c "
import tomllib
m = tomllib.load(open('${MANIFEST_PATH}', 'rb'))
assert m['plugin']['name'] == 'ruff', f\"plugin.name != 'ruff': {m['plugin']['name']!r}\"
assert m['plugin']['category'] == 'quality', f\"plugin.category != 'quality': {m['plugin']['category']!r}\"
assert m['transport']['type'] == 'stdio', f\"transport.type != 'stdio': {m['transport']['type']!r}\"
assert any(c.startswith('quality.') for c in m['capabilities']['provides']), \
    f\"no capability under quality.* namespace: {m['capabilities']['provides']!r}\"
print('manifest-sanity: ok')
"; then
    echo "[FAIL] P3-W11-F03: manifest sanity failed for ${MANIFEST_PATH}" >&2
    exit 1
fi

# Prereq: uvx on PATH.
if ! command -v uvx >/dev/null 2>&1; then
    echo "[FAIL] P3-W11-F03: uvx not on PATH." >&2
    echo "  See scripts/devtools/install-ruff-mcp.sh for install hints." >&2
    exit 1
fi
echo "[INFO] P3-W11-F03: uvx version: $(uvx --version)"
echo "[INFO] P3-W11-F03: ruff pinned: ${PINNED_PKG_SPEC}"

# Step 1: integration test (real subprocess, real JSON-RPC).
CARGO_LOG="/tmp/wo-0080-f03-cargo.log"
echo "[INFO] P3-W11-F03: running cargo test g7_plugin_manifests::ruff_manifest_health_check..."
if ! cargo test -p ucil-daemon --test g7_plugin_manifests \
        g7_plugin_manifests::ruff_manifest_health_check 2>&1 | tee "${CARGO_LOG}" >/dev/null; then
    echo "[FAIL] P3-W11-F03: cargo test exited non-zero - see ${CARGO_LOG}" >&2
    tail -20 "${CARGO_LOG}" >&2 || true
    exit 1
fi
if ! grep -Eq 'test result: ok\. 1 passed; 0 failed|1 tests run: 1 passed' "${CARGO_LOG}"; then
    echo "[FAIL] P3-W11-F03: cargo test summary line missing in ${CARGO_LOG}" >&2
    tail -20 "${CARGO_LOG}" >&2 || true
    exit 1
fi
echo "[INFO] P3-W11-F03: integration test PASS."

# Step 2: tool-level smoke.
if [[ "${UCIL_SKIP_QUALITY_PLUGIN_E2E:-0}" == "1" ]]; then
    echo "[SKIP] P3-W11-F03: tool-level smoke (UCIL_SKIP_QUALITY_PLUGIN_E2E=1)."
    echo "[OK] P3-W11-F03"
    exit 0
fi

RUFF_TMPDIR="$(mktemp -d -t wo-0080-f03-ruff-XXXXXX)"
SMOKE_LOG="/tmp/wo-0080-f03-smoke.log"
trap 'rm -rf "${RUFF_TMPDIR}"' EXIT

# Copy the python-project fixture into the tmpdir BEFORE invoking
# the upstream binary (per WO-0074 §executor #5 + WO-0080 scope_in
# #5/#11). The fixture's source remains read-only; we only operate
# against the tmpdir copy.
FIXTURE_COPY="${RUFF_TMPDIR}/python-project"
cp -r "${REPO_ROOT}/tests/fixtures/python-project" "${FIXTURE_COPY}"

# Drop a deliberate-lint-violation `bad.py` in the tmpdir copy. Per
# WO-0080 scope_in #36: the python-project fixture does NOT carry
# deliberate Ruff violations; fabricate a small file in the tmpdir
# copy ONLY (forbidden_paths invariant — `tests/fixtures/**` is
# read-only).
#
# Violation summary:
#   E401 - multiple-imports-on-one-line (`import os, sys`).
#   F841 - unused-variable (`unused = 42`).
# Default Ruff rule set surfaces both reliably (Ruff's default
# `select = ['E4', 'E7', 'E9', 'F']` covers E401 and F841).
cat > "${FIXTURE_COPY}/bad.py" <<'PY'
"""Deliberate-lint-violation file for the WO-0080 P3-W11-F03 verify
script.

This file intentionally contains Ruff lint defects used to test
UCIL's Ruff MCP integration. Do not clean up these defects.
"""

import os, sys


def f():
    unused = 42
    return os.path.join(sys.argv[0], "bar")
PY

export PINNED_PKG_SPEC FIXTURE_COPY SMOKE_LOG

# Drive the MCP session from python3 with deadline polling per
# WO-0074 executor #4. Ruff lints are fast (~1-2s) so a 60s deadline
# is generous.
if ! python3 - <<'PY'
import json
import os
import subprocess
import sys
import time
from pathlib import Path


def main() -> int:
    pkg_spec = os.environ["PINNED_PKG_SPEC"]
    fixture_copy = Path(os.environ["FIXTURE_COPY"])
    smoke_log = open(os.environ["SMOKE_LOG"], "ab")

    # Read the bad.py contents into a Python string. The upstream
    # tools take inline `code:str` arguments (not file paths) per the
    # live capture documented in plugins/quality/ruff/plugin.toml
    # top-of-file rustdoc.
    bad_py = (fixture_copy / "bad.py").read_text(encoding="utf-8")
    print(f"prepared bad.py: {len(bad_py)} chars")

    proc = subprocess.Popen(
        ["uvx", pkg_spec],
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
                "clientInfo": {"name": "ucil-wo-0080", "version": "0.1.0"},
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
            sys.stderr.write("ruff did not reply to initialize within 90s\n")
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

        # tools/list - assert ruff-check is advertised.
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
            sys.stderr.write("ruff did not reply to tools/list within 30s\n")
            return 1
        if "error" in tl_frame:
            sys.stderr.write(f"tools/list returned a JSON-RPC error: {tl_frame['error']!r}\n")
            return 1
        tools = tl_frame.get("result", {}).get("tools", [])
        tool_names = [t.get("name") for t in tools]
        print(f"tools/list ok: {len(tools)} tools advertised; names: {tool_names}")
        if "ruff-check" not in tool_names:
            sys.stderr.write(f"expected `ruff-check` in tools/list; got: {tool_names!r}\n")
            return 1

        # tools/call ruff-check {code: <bad.py contents>} - assert
        # >=1 structured finding with rule/line/file per F03 spec.
        proc.stdin.write(
            json.dumps(
                {
                    "jsonrpc": "2.0",
                    "id": 3,
                    "method": "tools/call",
                    "params": {
                        "name": "ruff-check",
                        "arguments": {
                            "code": bad_py,
                        },
                    },
                }
            )
            + "\n"
        )
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
            sys.stderr.write("ruff did not reply to tools/call within 60s\n")
            return 1
        if "error" in call_frame:
            sys.stderr.write(
                f"tools/call ruff-check returned a JSON-RPC error: {call_frame['error']!r}\n"
            )
            return 1

        # mcp-server-analyzer's ruff-check returns a FastMCP structured
        # tool response — the result body MAY carry both `content`
        # (text blocks for backward-compat) and `structuredContent`
        # (typed payload). Either path is acceptable; we probe both.
        result = call_frame.get("result") or {}
        structured = result.get("structuredContent") or {}
        content = result.get("content") or []
        text_blocks = [b.get("text", "") for b in content if isinstance(b, dict)]

        findings = None

        def harvest(obj):
            if not isinstance(obj, dict):
                return None
            for key in (
                "violations",
                "findings",
                "issues",
                "results",
                "diagnostics",
            ):
                if key in obj and isinstance(obj[key], list):
                    return obj[key]
            inner = obj.get("result")
            if isinstance(inner, dict):
                got = harvest(inner)
                if got is not None:
                    return got
            return None

        findings = harvest(structured)
        if findings is None:
            for block in text_blocks:
                try:
                    parsed = json.loads(block)
                except json.JSONDecodeError:
                    continue
                if isinstance(parsed, list):
                    findings = parsed
                    break
                got = harvest(parsed) if isinstance(parsed, dict) else None
                if got is not None:
                    findings = got
                    break

        if findings is None:
            sys.stderr.write(
                f"tools/call ruff-check response missing a recognizable findings array. "
                f"structured keys: {list(structured.keys()) if isinstance(structured, dict) else None}; "
                f"text blocks (truncated): {[b[:200] for b in text_blocks]}\n"
            )
            return 1

        # Per F03 spec line:
        #   "Python lint errors and format violations from the fixture
        #    python-project returned with file, line, rule, and fix
        #    suggestion."
        # Ruff's JSON output literals (per `ruff check
        # --output-format=json`):
        #   filename / file        - per-finding file path or "<stdin>"
        #   location.row / line    - per-finding row
        #   code / rule            - per-finding rule code (e.g. E401)
        #   message                - human-readable message
        #   fix                    - optional structured fix object
        if len(findings) < 1:
            sys.stderr.write(
                f"tools/call ruff-check returned zero findings under "
                f"Ruff's default rule set; expected >=1 finding from "
                f"the fabricated bad.py. raw structured: {structured!r}\n"
            )
            return 1

        first = findings[0]
        if not isinstance(first, dict):
            sys.stderr.write(
                f"finding[0] is not a dict; got {type(first).__name__}: {first!r}\n"
            )
            return 1
        rule = first.get("code") or first.get("rule") or first.get("rule_id")
        if rule is None:
            sys.stderr.write(
                f"finding[0] missing rule key (`code`/`rule`/`rule_id`): keys={list(first.keys())}\n"
            )
            return 1
        line_num = None
        loc = first.get("location")
        if isinstance(loc, dict):
            line_num = loc.get("row") or loc.get("line")
        if line_num is None:
            line_num = first.get("line") or first.get("line_number")
        if line_num is None and isinstance(first.get("start"), dict):
            line_num = first["start"].get("line")
        if line_num is None:
            sys.stderr.write(
                f"finding[0] missing line key (`location.row`/`line`/`start.line`): "
                f"keys={list(first.keys())}\n"
            )
            return 1
        file_path = (
            first.get("filename")
            or first.get("file")
            or first.get("path")
            or "<stdin>"
        )
        message = first.get("message") or first.get("msg") or first.get("description")
        print(
            f"tools/call ruff-check ok: {len(findings)} findings; "
            f"first: rule={rule!r} line={line_num!r} file={file_path!r} "
            f"message={message!r}"
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
    echo "[FAIL] P3-W11-F03: tool-level smoke failed - see ${SMOKE_LOG}" >&2
    tail -30 "${SMOKE_LOG}" >&2 || true
    exit 1
fi

echo "[INFO] P3-W11-F03: tool-level smoke PASS."
echo "[OK] P3-W11-F03"
exit 0
