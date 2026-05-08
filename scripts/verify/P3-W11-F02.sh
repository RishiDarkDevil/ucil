#!/usr/bin/env bash
# Acceptance test for P3-W11-F02 — ESLint G7 plugin manifest health-
# check + lint-files smoke against a tmpdir copy of the
# typescript-project fixture (with a fabricated minimal
# eslint.config.js + a deliberate-lint-violation file).
#
# Behaviour:
#   1. cd to the repo root via `git rev-parse --show-toplevel`.
#   2. Sanity-check `plugins/quality/eslint/plugin.toml` parses as TOML
#      and asserts the keys named in WO-0076 scope_in #1 (plugin.name
#      == "eslint", plugin.category == "quality",
#      transport.type == "stdio", capabilities.provides starts with
#      "quality.").
#   3. Assert `npx` is on PATH; otherwise exit 1 with a clear hint
#      pointing at `scripts/devtools/install-eslint-mcp.sh`.
#   4. Print npx + the pinned @eslint/mcp version for the verifier
#      log.
#   5. Run `cargo test -p ucil-daemon --test g7_plugin_manifests
#      g7_plugin_manifests::eslint_manifest_health_check` and require
#      the cargo-test summary line "1 passed; 0 failed" or the
#      cargo-nextest equivalent — alternation regex per
#      WO-0042 / WO-0044 / WO-0069 / WO-0072 / WO-0074 / WO-0075.
#   6. Tool-level smoke (gated by ${UCIL_SKIP_QUALITY_PLUGIN_E2E:-0},
#      default RUN per WO-0074 §executor #3 + WO-0076 scope_in #6):
#        a. Copy `tests/fixtures/typescript-project` into a mktemp -d
#           tmpdir (per WO-0074 §executor #5 + WO-0076 scope_in #5/#16
#           — copy BEFORE invoking so the read-only fixture stays
#           pristine).
#        b. Fabricate a minimal `eslint.config.js` (flat config v9+)
#           in the tmpdir that enables `no-unused-vars` and `no-undef`
#           on `**/*.js` files, and drop a `bad.js` deliberate-lint-
#           violation file with `var unused = 42; console.log(undef);`
#           — the typescript-project fixture's TS source ships with
#           `// @ts-strict` style — but ESLint's flat-config without
#           a TS parser cannot parse TS files. Linting a synthetic JS
#           file in the tmpdir gives a deterministic >=1-finding
#           signal independent of the upstream's TS-parsing config.
#        c. Spawn `npx -y @eslint/mcp@<pin>` from the tmpdir as cwd.
#        d. INIT - `initialize` + `notifications/initialized`.
#        e. tools/list - assert `lint-files` is advertised
#           (load-bearing - verifies the manifest's pin matches the
#           upstream's tool surface).
#        f. tools/call lint-files {filePaths: [<absolute-path>]} -
#           assert response is structured with file/line/rule/severity
#           per F02 spec.
#   7. On all-green prints `[OK] P3-W11-F02` and exits 0; on any
#      failure prints `[FAIL] P3-W11-F02: <reason>` and exits 1.
#
# Per WO-0074 §executor #4: the tool-level smoke uses python polling
# with a deadline rather than bash `sleep` so wall-time-sensitive
# stdio JSON-RPC reads complete reliably even on slow workstations.
set -euo pipefail
IFS=$'\n\t'

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

PINNED_NPM_VERSION="0.3.5"
PINNED_PKG_SPEC="@eslint/mcp@${PINNED_NPM_VERSION}"
MANIFEST_PATH="plugins/quality/eslint/plugin.toml"

# Step 0: manifest TOML sanity (cheap pre-flight).
echo "[INFO] P3-W11-F02: parsing ${MANIFEST_PATH} ..."
if ! python3 -c "
import tomllib
m = tomllib.load(open('${MANIFEST_PATH}', 'rb'))
assert m['plugin']['name'] == 'eslint', f\"plugin.name != 'eslint': {m['plugin']['name']!r}\"
assert m['plugin']['category'] == 'quality', f\"plugin.category != 'quality': {m['plugin']['category']!r}\"
assert m['transport']['type'] == 'stdio', f\"transport.type != 'stdio': {m['transport']['type']!r}\"
assert any(c.startswith('quality.') for c in m['capabilities']['provides']), \
    f\"no capability under quality.* namespace: {m['capabilities']['provides']!r}\"
print('manifest-sanity: ok')
"; then
    echo "[FAIL] P3-W11-F02: manifest sanity failed for ${MANIFEST_PATH}" >&2
    exit 1
fi

# Prereq: npx on PATH.
if ! command -v npx >/dev/null 2>&1; then
    echo "[FAIL] P3-W11-F02: npx not on PATH." >&2
    echo "  See scripts/devtools/install-eslint-mcp.sh for install hints." >&2
    exit 1
fi
echo "[INFO] P3-W11-F02: npx version: $(npx --version)"
echo "[INFO] P3-W11-F02: eslint pinned: ${PINNED_PKG_SPEC}"

# Step 1: integration test (real subprocess, real JSON-RPC).
CARGO_LOG="/tmp/wo-0076-f02-cargo.log"
echo "[INFO] P3-W11-F02: running cargo test g7_plugin_manifests::eslint_manifest_health_check..."
if ! cargo test -p ucil-daemon --test g7_plugin_manifests \
        g7_plugin_manifests::eslint_manifest_health_check 2>&1 | tee "${CARGO_LOG}" >/dev/null; then
    echo "[FAIL] P3-W11-F02: cargo test exited non-zero - see ${CARGO_LOG}" >&2
    tail -20 "${CARGO_LOG}" >&2 || true
    exit 1
fi
if ! grep -Eq 'test result: ok\. 1 passed; 0 failed|1 tests run: 1 passed' "${CARGO_LOG}"; then
    echo "[FAIL] P3-W11-F02: cargo test summary line missing in ${CARGO_LOG}" >&2
    tail -20 "${CARGO_LOG}" >&2 || true
    exit 1
fi
echo "[INFO] P3-W11-F02: integration test PASS."

# Step 2: tool-level smoke.
if [[ "${UCIL_SKIP_QUALITY_PLUGIN_E2E:-0}" == "1" ]]; then
    echo "[SKIP] P3-W11-F02: tool-level smoke (UCIL_SKIP_QUALITY_PLUGIN_E2E=1)."
    echo "[OK] P3-W11-F02"
    exit 0
fi

ESLINT_TMPDIR="$(mktemp -d -t wo-0076-f02-eslint-XXXXXX)"
SMOKE_LOG="/tmp/wo-0076-f02-smoke.log"
trap 'rm -rf "${ESLINT_TMPDIR}"' EXIT

# Copy the typescript-project fixture into the tmpdir BEFORE invoking
# the upstream binary (per WO-0074 executor #5 + WO-0076 scope_in
# #5/#16). The fixture's TS source remains read-only; we only operate
# against the tmpdir copy.
FIXTURE_COPY="${ESLINT_TMPDIR}/typescript-project"
cp -r "${REPO_ROOT}/tests/fixtures/typescript-project" "${FIXTURE_COPY}"

# Fabricate a minimal eslint.config.js (flat-config v9+) and a
# deliberate-lint-violation `bad.js` in the tmpdir copy. Per WO-0076
# scope_in #16: the typescript-project fixture does NOT carry an
# eslint config, and ESLint flat-config without a TS parser cannot
# parse TS files - so lint a synthetic JS file in the tmpdir for a
# deterministic >=1-finding signal.
cat > "${FIXTURE_COPY}/eslint.config.js" <<'CFG'
export default [
  {
    files: ["**/*.js"],
    rules: {
      "no-unused-vars": "error",
      "no-undef": "error"
    },
    languageOptions: {
      sourceType: "module",
      globals: {}
    }
  }
];
CFG
cat > "${FIXTURE_COPY}/bad.js" <<'JS'
var unused = 42;
console.log(undefinedVariable);
JS

export PINNED_PKG_SPEC FIXTURE_COPY SMOKE_LOG

# Drive the MCP session from python3 with deadline polling per
# WO-0074 executor #4. ESLint's npx fetch + lint round-trip is
# wall-time-light but python is also more reliable for the JSON-RPC
# parsing.
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
        ["npx", "-y", pkg_spec],
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
                "clientInfo": {"name": "ucil-wo-0076", "version": "0.1.0"},
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
            sys.stderr.write("eslint did not reply to initialize within 90s\n")
            return 1

        # initialized notification.
        proc.stdin.write(json.dumps({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {},
        }) + "\n")
        proc.stdin.flush()

        # tools/list - assert lint-files is advertised.
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
            sys.stderr.write("eslint did not reply to tools/list within 30s\n")
            return 1
        if "error" in tl_frame:
            sys.stderr.write(f"tools/list returned a JSON-RPC error: {tl_frame['error']!r}\n")
            return 1
        tools = tl_frame.get("result", {}).get("tools", [])
        tool_names = [t.get("name") for t in tools]
        print(f"tools/list ok: {len(tools)} tools advertised; names: {tool_names}")
        if "lint-files" not in tool_names:
            sys.stderr.write(
                f"expected `lint-files` in tools/list; got: {tool_names!r}\n"
            )
            return 1

        # tools/call lint-files - pass the absolute path to bad.js
        # and assert the structured findings shape per F02 spec.
        bad_js_abs = os.path.join(fixture_copy, "bad.js")
        proc.stdin.write(json.dumps({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "lint-files",
                "arguments": {
                    "filePaths": [bad_js_abs],
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
            sys.stderr.write("eslint did not reply to tools/call within 60s\n")
            return 1
        if "error" in call_frame:
            sys.stderr.write(
                f"tools/call lint-files returned a JSON-RPC error: {call_frame['error']!r}\n"
            )
            return 1

        # Walk result.content and find the JSON-blob text block -
        # ESLint MCP returns a 3-block response: preamble + JSON +
        # trailer. The middle block carries the structured findings
        # array verbatim.
        result = call_frame.get("result") or {}
        content = result.get("content") or []
        text_blocks = [b.get("text", "") for b in content if isinstance(b, dict)]
        findings = None
        for block in text_blocks:
            try:
                parsed = json.loads(block)
            except json.JSONDecodeError:
                continue
            if isinstance(parsed, dict) and "messages" in parsed:
                findings = parsed
                break
        if findings is None:
            sys.stderr.write(
                f"tools/call lint-files response missing structured findings JSON; "
                f"text blocks: {[b[:100] for b in text_blocks]}\n"
            )
            return 1
        # Per F02 spec line:
        #   "JS/TS lint errors from the fixture typescript-project
        #    returned with file, line, rule, and severity"
        # ESLint upstream literals:
        #   filePath (file)
        #   messages[].line (line)
        #   messages[].ruleId (rule)
        #   messages[].severity (severity)
        if "filePath" not in findings:
            sys.stderr.write(
                f"findings missing 'filePath' key: {list(findings.keys())}\n"
            )
            return 1
        messages = findings.get("messages") or []
        if not messages:
            sys.stderr.write(
                f"findings.messages is empty: {findings.get('errorCount')} "
                f"errorCount; expected >=1 finding from bad.js\n"
            )
            return 1
        first = messages[0]
        for required in ("ruleId", "severity", "line"):
            if required not in first:
                sys.stderr.write(
                    f"finding[0] missing '{required}' key: {list(first.keys())}\n"
                )
                return 1
        print(
            f"tools/call lint-files ok: {len(messages)} structured findings; "
            f"first: rule={first.get('ruleId')!r} severity={first.get('severity')!r} "
            f"line={first.get('line')!r} file={findings.get('filePath')!r}"
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
    echo "[FAIL] P3-W11-F02: tool-level smoke failed - see ${SMOKE_LOG}" >&2
    tail -20 "${SMOKE_LOG}" >&2 || true
    exit 1
fi

echo "[INFO] P3-W11-F02: tool-level smoke PASS."
echo "[OK] P3-W11-F02"
exit 0
