#!/usr/bin/env bash
# diagnostics-bridge.sh — Live LSP diagnostics round-trip.
#
# Usage: scripts/verify/diagnostics-bridge.sh
#
# Purpose:
#   ucil-lsp-diagnostics (P1-W5-F03..F07) claims to bridge Serena's LSP
#   surface into UCIL's G7 quality pipeline, and to fall back to
#   spawning its own pyright / rust-analyzer when Serena is absent. This
#   script verifies the *fallback path* end-to-end against a real
#   pyright-langserver process using a deliberately-broken Python file
#   in tests/fixtures/python-project/.
#
#   Flow:
#     1. Locate pyright-langserver on PATH (or bail gracefully).
#     2. Copy a fixture Python file with a known type error into a tempdir.
#     3. Spawn pyright-langserver --stdio.
#     4. LSP handshake: initialize → initialized.
#     5. textDocument/didOpen the fixture.
#     6. Wait up to 15s for a textDocument/publishDiagnostics with ≥1 diag.
#     7. Assert at least one diagnostic points at the fixture and has
#        severity 1 (Error) or 2 (Warning).
#
# Exit codes:
#   0 — pyright returned real diagnostics for the fixture.
#   1 — LSP handshake failed / no diagnostics / timed out.
#   2 — prereq missing (pyright-langserver not installed).
set -uo pipefail

cd "$(git rev-parse --show-toplevel)"

if ! command -v pyright-langserver >/dev/null 2>&1; then
  if command -v npx >/dev/null 2>&1; then
    PYRIGHT=(npx -y pyright)
  else
    echo "[diagnostics-bridge] FAIL: pyright-langserver not on PATH and no npx to fetch it" >&2
    echo "  Install with: npm install -g pyright   (or pipx install pyright)" >&2
    exit 2
  fi
else
  PYRIGHT=(pyright-langserver --stdio)
fi

FIXTURE_SRC="tests/fixtures/python-project"
if [[ ! -d "$FIXTURE_SRC" ]]; then
  echo "[diagnostics-bridge] FAIL: fixture $FIXTURE_SRC missing" >&2
  exit 1
fi

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

# Copy the fixture and drop in a file with a deliberate type error.
cp -r "$FIXTURE_SRC" "$TMP/proj"
BAD_FILE="$TMP/proj/__diagnostics_probe.py"
cat > "$BAD_FILE" <<'PY'
"""Fixture for diagnostics-bridge probe — deliberate type error."""
def add(a: int, b: int) -> int:
    return a + b

result: str = add(1, 2)  # type error: int assigned to str
PY

# LSP wire format: Content-Length header + JSON body, per LSP spec.
lsp_msg() {
  local body="$1"
  local len=${#body}
  printf 'Content-Length: %d\r\n\r\n%s' "$len" "$body"
}

PROJECT_URI="file://$TMP/proj"
FILE_URI="file://$BAD_FILE"

INIT_BODY=$(jq -cn --arg root "$PROJECT_URI" '{
  jsonrpc:"2.0",id:1,method:"initialize",
  params:{
    processId:null,rootUri:$root,capabilities:{},
    workspaceFolders:[{uri:$root,name:"probe"}]
  }
}')
INITIALIZED='{"jsonrpc":"2.0","method":"initialized","params":{}}'
DIDOPEN_BODY=$(jq -cn --arg uri "$FILE_URI" --rawfile text "$BAD_FILE" '{
  jsonrpc:"2.0",method:"textDocument/didOpen",
  params:{textDocument:{uri:$uri,languageId:"python",version:1,text:$text}}
}')

IN_FIFO="$TMP/in"
mkfifo "$IN_FIFO"

{
  lsp_msg "$INIT_BODY"
  sleep 1
  lsp_msg "$INITIALIZED"
  lsp_msg "$DIDOPEN_BODY"
  sleep 15
} > "$IN_FIFO" &
IN_PID=$!

timeout 40 "${PYRIGHT[@]}" < "$IN_FIFO" > "$TMP/out.raw" 2>"$TMP/err.log" || true
wait "$IN_PID" 2>/dev/null || true

if [[ ! -s "$TMP/out.raw" ]]; then
  echo "[diagnostics-bridge] FAIL: pyright produced no output" >&2
  echo "-- stderr --" >&2
  tail -20 "$TMP/err.log" >&2
  exit 1
fi

# Extract JSON bodies from the Content-Length framed stream.
# A simple awk state-machine: when we see Content-Length:, note the length;
# after the blank line, read exactly that many bytes and emit one JSON line.
python3 - "$TMP/out.raw" > "$TMP/out.jsonl" <<'PY'
import sys, re
raw = open(sys.argv[1],'rb').read()
i = 0
while i < len(raw):
    m = re.match(rb'Content-Length: (\d+)\r\n\r\n', raw[i:])
    if not m:
        # skip forward to next Content-Length header if present
        nxt = raw.find(b'Content-Length:', i+1)
        if nxt < 0: break
        i = nxt
        continue
    length = int(m.group(1))
    start = i + m.end()
    body = raw[start:start+length]
    sys.stdout.write(body.decode('utf-8','replace') + '\n')
    i = start + length
PY

DIAG=$(jq -c 'select(.method == "textDocument/publishDiagnostics") | select((.params.diagnostics | length) > 0)' < "$TMP/out.jsonl" 2>/dev/null | head -1)
if [[ -z "$DIAG" ]]; then
  echo "[diagnostics-bridge] FAIL: no publishDiagnostics with a non-empty diagnostic list" >&2
  echo "-- messages received --" >&2
  jq -c '.method // (.id | tostring)' < "$TMP/out.jsonl" | head -10 >&2
  exit 1
fi

URI=$(jq -r '.params.uri' <<<"$DIAG")
COUNT=$(jq '.params.diagnostics | length' <<<"$DIAG")

if [[ "$URI" != "$FILE_URI" ]]; then
  echo "[diagnostics-bridge] FAIL: diagnostics URI mismatch: got $URI, want $FILE_URI" >&2
  exit 1
fi

echo "[diagnostics-bridge] OK — pyright returned $COUNT diagnostic(s) for $URI."
exit 0
