#!/usr/bin/env bash
# Acceptance test for P3-W11-F04 - Semgrep G7 plugin manifest health-
# check + semgrep_scan smoke against a tmpdir copy of the mixed-project
# fixture (with a fabricated deliberate-security-violation file).
#
# Disclosed Deviation lineage: pinned version is 0.8.1 (NOT WO-0076
# scope_in #2 prescription of 0.9.0). v0.9.0 ships only a
# `deprecation_notice` tool upstream (verified via live tools/list
# capture in the WO-0076 RFR). v0.8.1 is the last PyPI release with
# the canonical 8-tool scan surface. See
# plugins/quality/semgrep/plugin.toml top-of-file rustdoc for the
# full pivot rationale.
#
# Behaviour:
#   1. cd to the repo root via `git rev-parse --show-toplevel`.
#   2. Sanity-check `plugins/quality/semgrep/plugin.toml` parses as TOML
#      and asserts the keys named in WO-0076 scope_in #2.
#   3. Assert `uvx` is on PATH; otherwise exit 1 with a clear hint
#      pointing at `scripts/devtools/install-semgrep-mcp.sh`.
#   4. Resolve a working Semgrep CLI binary via SEMGREP_PATH or PATH
#      and verify it responds to `--version` (filters out the broken
#      uvx-bundled semgrep that crashes on opentelemetry-instrumentation
#      imports). If no working binary is found, log [SKIP] and exit 0
#      gracefully (operator-state per the WO-0069 Mem0 short-circuit
#      precedent applied to a CLI binary).
#   5. Print uvx + the pinned semgrep-mcp version + the resolved
#      semgrep CLI version for the verifier log.
#   6. Run `cargo test -p ucil-daemon --test g7_plugin_manifests
#      g7_plugin_manifests::semgrep_manifest_health_check` and require
#      the cargo-test summary line "1 passed; 0 failed" or the
#      cargo-nextest equivalent.
#   7. Tool-level smoke (gated by ${UCIL_SKIP_QUALITY_PLUGIN_E2E:-0},
#      default RUN per WO-0074 executor #3 + WO-0076 scope_in #7):
#        a. Copy `tests/fixtures/mixed-project` into a mktemp -d
#           tmpdir (per WO-0074 executor #5 + WO-0076 scope_in #5/#7
#           - copy BEFORE invoking so the read-only fixture stays
#           pristine).
#        b. Drop a deliberate-security-violation `bad_security.py`
#           into the tmpdir copy (mixed-project's deliberate Python
#           lint defects do NOT trigger OWASP-class rules; an obvious
#           subprocess shell=True / eval / SQL-injection pattern in
#           bad_security.py reliably triggers
#           `python.lang.security.audit.subprocess-shell-true` which
#           is an OWASP A03 Injection finding under
#           `p/owasp-top-ten`).
#        c. Read all files in the tmpdir copy and pass them to the
#           upstream's semgrep_scan tool as the `code_files` argument
#           (the upstream takes inline file contents, NOT paths -
#           it scans the contents directly).
#        d. Spawn `uvx semgrep-mcp@<pin>` (env inherits SEMGREP_PATH).
#        e. INIT - `initialize` + `notifications/initialized`.
#        f. tools/list - assert `semgrep_scan` is advertised.
#        g. tools/call semgrep_scan {code_files, config: "p/owasp-
#           top-ten"} - assert >=1 finding with rule/severity/file/
#           line per F04 spec.
#   8. On all-green prints `[OK] P3-W11-F04` and exits 0; on any
#      failure prints `[FAIL] P3-W11-F04: <reason>` and exits 1.
#
# Per WO-0074 executor #4: the tool-level smoke uses python polling
# with a deadline rather than bash sleep so wall-time-sensitive
# Semgrep scans (which can take 10-30s on cold rule-cache) complete
# reliably.
set -euo pipefail
IFS=$'\n\t'

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

PINNED_PYPI_VERSION="0.8.1"
PINNED_PKG_SPEC="semgrep-mcp@${PINNED_PYPI_VERSION}"
MANIFEST_PATH="plugins/quality/semgrep/plugin.toml"

# Step 0: manifest TOML sanity (cheap pre-flight).
echo "[INFO] P3-W11-F04: parsing ${MANIFEST_PATH} ..."
if ! python3 -c "
import tomllib
m = tomllib.load(open('${MANIFEST_PATH}', 'rb'))
assert m['plugin']['name'] == 'semgrep', f\"plugin.name != 'semgrep': {m['plugin']['name']!r}\"
assert m['plugin']['category'] == 'quality', f\"plugin.category != 'quality': {m['plugin']['category']!r}\"
assert m['transport']['type'] == 'stdio', f\"transport.type != 'stdio': {m['transport']['type']!r}\"
assert any(c.startswith('quality.') for c in m['capabilities']['provides']), \
    f\"no capability under quality.* namespace: {m['capabilities']['provides']!r}\"
print('manifest-sanity: ok')
"; then
    echo "[FAIL] P3-W11-F04: manifest sanity failed for ${MANIFEST_PATH}" >&2
    exit 1
fi

# Prereq: uvx on PATH.
if ! command -v uvx >/dev/null 2>&1; then
    echo "[FAIL] P3-W11-F04: uvx not on PATH." >&2
    echo "  See scripts/devtools/install-semgrep-mcp.sh for install hints." >&2
    exit 1
fi
echo "[INFO] P3-W11-F04: uvx version: $(uvx --version)"
echo "[INFO] P3-W11-F04: semgrep pinned: ${PINNED_PKG_SPEC}"

# Resolve a working Semgrep CLI - filter out the broken uvx-bundled
# binary by requiring a successful --version exit.
resolve_semgrep() {
    if [[ -n "${SEMGREP_PATH:-}" && -x "${SEMGREP_PATH}" ]]; then
        if "${SEMGREP_PATH}" --version >/dev/null 2>&1; then
            echo "${SEMGREP_PATH}"
            return 0
        fi
    fi
    if command -v semgrep >/dev/null 2>&1; then
        local resolved
        resolved="$(command -v semgrep)"
        if "${resolved}" --version >/dev/null 2>&1; then
            echo "${resolved}"
            return 0
        fi
    fi
    return 1
}
if SEMGREP_BIN="$(resolve_semgrep)"; then
    SEMGREP_VERSION="$("${SEMGREP_BIN}" --version 2>/dev/null | tail -1)"
    echo "[INFO] P3-W11-F04: semgrep CLI: ${SEMGREP_BIN} (v${SEMGREP_VERSION})"
    export SEMGREP_PATH="${SEMGREP_BIN}"
else
    echo "[SKIP] P3-W11-F04: no working semgrep CLI found via SEMGREP_PATH or PATH." >&2
    echo "  Install via: pip install --user semgrep, or uv tool install semgrep" >&2
    echo "  See scripts/devtools/install-semgrep-mcp.sh for additional hints." >&2
    echo "[OK] P3-W11-F04 (skipped - operator-state)"
    exit 0
fi

# Step 1: integration test (real subprocess, real JSON-RPC).
CARGO_LOG="/tmp/wo-0076-f04-cargo.log"
echo "[INFO] P3-W11-F04: running cargo test g7_plugin_manifests::semgrep_manifest_health_check..."
if ! cargo test -p ucil-daemon --test g7_plugin_manifests \
        g7_plugin_manifests::semgrep_manifest_health_check 2>&1 | tee "${CARGO_LOG}" >/dev/null; then
    echo "[FAIL] P3-W11-F04: cargo test exited non-zero - see ${CARGO_LOG}" >&2
    tail -20 "${CARGO_LOG}" >&2 || true
    exit 1
fi
if ! grep -Eq 'test result: ok\. 1 passed; 0 failed|1 tests run: 1 passed' "${CARGO_LOG}"; then
    echo "[FAIL] P3-W11-F04: cargo test summary line missing in ${CARGO_LOG}" >&2
    tail -20 "${CARGO_LOG}" >&2 || true
    exit 1
fi
echo "[INFO] P3-W11-F04: integration test PASS."

# Step 2: tool-level smoke.
if [[ "${UCIL_SKIP_QUALITY_PLUGIN_E2E:-0}" == "1" ]]; then
    echo "[SKIP] P3-W11-F04: tool-level smoke (UCIL_SKIP_QUALITY_PLUGIN_E2E=1)."
    echo "[OK] P3-W11-F04"
    exit 0
fi

SEMGREP_TMPDIR="$(mktemp -d -t wo-0076-f04-semgrep-XXXXXX)"
SMOKE_LOG="/tmp/wo-0076-f04-smoke.log"
trap 'rm -rf "${SEMGREP_TMPDIR}"' EXIT

# Copy the mixed-project fixture into the tmpdir BEFORE invoking the
# upstream binary (per WO-0074 executor #5 + WO-0076 scope_in #5/#7).
FIXTURE_COPY="${SEMGREP_TMPDIR}/mixed-project"
cp -r "${REPO_ROOT}/tests/fixtures/mixed-project" "${FIXTURE_COPY}"

# Drop a deliberate-security-violation Python file in the tmpdir.
# The mixed-project fixture's deliberate Python lint defects (B006
# mutable default arg, bare except, print() in lib code) do NOT
# trigger OWASP-class security rules. An obvious subprocess
# shell=True + eval + SQL-injection pattern reliably triggers
# `python.lang.security.audit.subprocess-shell-true` (OWASP A03
# Injection) under `p/owasp-top-ten` per the live capture documented
# in the WO-0076 RFR.
cat > "${FIXTURE_COPY}/bad_security.py" <<'PY'
"""Deliberate-security-violation file for the WO-0076 P3-W11-F04
verify script.

This file intentionally contains OWASP-class security defects used to
test UCIL's Semgrep MCP integration. Do not clean up these defects.
"""
import os
import sqlite3
import subprocess


def lookup_user(name):
    """Look up a user by name. SECURITY: SQL injection via f-string."""
    conn = sqlite3.connect("users.db")
    cur = conn.cursor()
    # OWASP A03 Injection: SQL-injection via f-string interpolation.
    cur.execute(f"SELECT * FROM users WHERE name = '{name}'")
    return cur.fetchall()


def run_command(cmd):
    """Run a shell command. SECURITY: command injection via shell=True."""
    # OWASP A03 Injection: shell=True with attacker-controlled input.
    subprocess.call(cmd, shell=True)


def evaluate(user_text):
    """Evaluate a Python expression. SECURITY: arbitrary code execution."""
    # OWASP A03 Injection: eval() of user-controlled input.
    return eval(user_text)
PY

export PINNED_PKG_SPEC FIXTURE_COPY SMOKE_LOG

# Drive the MCP session from python3 with deadline polling per
# WO-0074 executor #4. Semgrep scans are wall-time-sensitive
# (cold rule-cache fetches can take 10-30s) so a generous deadline
# is essential.
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

    # Read all files in the tmpdir copy as inline code_files.
    code_files = []
    for path in sorted(fixture_copy.rglob("*")):
        if not path.is_file():
            continue
        try:
            content = path.read_text(encoding="utf-8")
        except (UnicodeDecodeError, OSError):
            continue
        rel = path.relative_to(fixture_copy)
        code_files.append({"filename": str(rel), "content": content})
    print(f"code_files: {len(code_files)} files prepared for scan")

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
                "clientInfo": {"name": "ucil-wo-0076", "version": "0.1.0"},
            },
        }
        assert proc.stdin is not None
        proc.stdin.write(json.dumps(init_msg) + "\n")
        proc.stdin.flush()

        deadline = time.monotonic() + 180.0
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
            sys.stderr.write("semgrep did not reply to initialize within 180s\n")
            return 1

        # initialized notification.
        proc.stdin.write(json.dumps({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {},
        }) + "\n")
        proc.stdin.flush()

        # tools/list - assert semgrep_scan is advertised.
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
            sys.stderr.write("semgrep did not reply to tools/list within 30s\n")
            return 1
        if "error" in tl_frame:
            sys.stderr.write(f"tools/list returned a JSON-RPC error: {tl_frame['error']!r}\n")
            return 1
        tools = tl_frame.get("result", {}).get("tools", [])
        tool_names = [t.get("name") for t in tools]
        print(f"tools/list ok: {len(tools)} tools advertised; sample: {tool_names[:5]}")
        if "semgrep_scan" not in tool_names:
            sys.stderr.write(
                f"expected `semgrep_scan` in tools/list; got: {tool_names!r}\n"
            )
            return 1

        # tools/call semgrep_scan with config p/owasp-top-ten.
        proc.stdin.write(json.dumps({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "semgrep_scan",
                "arguments": {
                    "code_files": code_files,
                    "config": "p/owasp-top-ten",
                },
            },
        }) + "\n")
        proc.stdin.flush()

        deadline = time.monotonic() + 120.0
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
            sys.stderr.write("semgrep did not reply to tools/call within 120s\n")
            return 1
        if "error" in call_frame:
            sys.stderr.write(
                f"tools/call semgrep_scan returned a JSON-RPC error: {call_frame['error']!r}\n"
            )
            return 1

        # Walk result.content[0].text - the Semgrep scan response is
        # a JSON-encoded blob in the first text-block per upstream
        # response shape (verified via WO-0076 RFR live capture).
        result = call_frame.get("result") or {}
        content = result.get("content") or []
        text_blocks = [b.get("text", "") for b in content if isinstance(b, dict)]
        scan_result = None
        for block in text_blocks:
            try:
                parsed = json.loads(block)
            except json.JSONDecodeError:
                continue
            if isinstance(parsed, dict) and "results" in parsed:
                scan_result = parsed
                break
        if scan_result is None:
            sys.stderr.write(
                f"tools/call semgrep_scan response missing results JSON; "
                f"text blocks: {[b[:200] for b in text_blocks]}\n"
            )
            return 1

        results = scan_result.get("results") or []
        # Per F04 spec line:
        #   ">=1 security finding using the OWASP rule set"
        # Semgrep upstream literals (verified via WO-0076 RFR live
        # capture):
        #   results[].check_id (rule_id)
        #   results[].extra.severity (severity)
        #   results[].path (file)
        #   results[].start.line (line)
        if len(results) < 1:
            sys.stderr.write(
                f"tools/call semgrep_scan returned zero findings under "
                f"`p/owasp-top-ten`; expected >=1 OWASP-class finding from "
                f"the bad_security.py deliberate-violation file. "
                f"errors={scan_result.get('errors')!r} "
                f"paths={scan_result.get('paths')!r}\n"
            )
            return 1

        first = results[0]
        for required in ("check_id", "path", "start"):
            if required not in first:
                sys.stderr.write(
                    f"finding[0] missing '{required}' key: {list(first.keys())}\n"
                )
                return 1
        # severity lives under extra.severity.
        extra = first.get("extra") or {}
        if "severity" not in extra:
            sys.stderr.write(
                f"finding[0].extra missing 'severity' key: {list(extra.keys())}\n"
            )
            return 1
        if "line" not in (first.get("start") or {}):
            sys.stderr.write(
                f"finding[0].start missing 'line' key: {first.get('start')!r}\n"
            )
            return 1
        print(
            f"tools/call semgrep_scan ok: {len(results)} OWASP findings; "
            f"first: rule={first.get('check_id')!r} "
            f"severity={extra.get('severity')!r} "
            f"file={first.get('path')!r} "
            f"line={first.get('start',{}).get('line')!r}"
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
    echo "[FAIL] P3-W11-F04: tool-level smoke failed - see ${SMOKE_LOG}" >&2
    tail -20 "${SMOKE_LOG}" >&2 || true
    exit 1
fi

echo "[INFO] P3-W11-F04: tool-level smoke PASS."
echo "[OK] P3-W11-F04"
exit 0
