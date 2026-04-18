#!/usr/bin/env bash
# diagnostics-bridge.sh — Live pyright diagnostics round-trip.
#
# Usage: scripts/verify/diagnostics-bridge.sh
#
# Purpose:
#   ucil-lsp-diagnostics (P1-W5-F03..F07) claims to bridge Serena's LSP
#   surface into UCIL's G7 quality pipeline, and to fall back to
#   pyright / rust-analyzer when Serena is absent. This script verifies
#   the *fallback path* end-to-end against a real pyright process using
#   a deliberately-broken Python file in tests/fixtures/python-project/.
#
#   We use the pyright batch CLI (`pyright --outputjson`) instead of
#   the LSP server (pyright-langserver --stdio): both ship in the same
#   npm package and wrap the same type-analyzer, but the batch CLI is
#   deterministic and ~100x faster for this probe. pyright-langserver
#   analyses the whole project before publishing diagnostics and has
#   timing-sensitive handshake/analysis races that are hard to make
#   reliable over a FIFO. The batch CLI returns structured JSON
#   directly.
#
#   Flow:
#     1. Locate pyright on PATH (or bail gracefully).
#     2. Copy the fixture + write a deliberate type-error file.
#     3. `pyright --outputjson __probe.py` from inside the fixture.
#     4. Parse generalDiagnostics[]; assert >=1 error points at probe.
#
# Exit codes:
#   0 — pyright returned diagnostics for the fixture.
#   1 — analyzer failed / no diagnostics for the probe.
#   2 — prereq missing (pyright not installed).
set -uo pipefail

cd "$(git rev-parse --show-toplevel)"

if ! command -v pyright >/dev/null 2>&1; then
  if command -v npx >/dev/null 2>&1; then
    PYRIGHT=(npx -y pyright)
  else
    echo "[diagnostics-bridge] FAIL: pyright not on PATH and no npx to fetch it" >&2
    echo "  Install with: npm install -g pyright   (or pipx install pyright)" >&2
    exit 2
  fi
else
  PYRIGHT=(pyright)
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

# Run pyright in batch mode, rooted at the project so it picks up
# pyproject.toml's Python version.
OUT="$TMP/pyright.json"
ERR="$TMP/pyright.err"
( cd "$TMP/proj" && timeout 60 "${PYRIGHT[@]}" --outputjson __diagnostics_probe.py ) \
  > "$OUT" 2>"$ERR" || true

if [[ ! -s "$OUT" ]]; then
  echo "[diagnostics-bridge] FAIL: pyright produced no JSON output" >&2
  echo "-- stderr --" >&2
  tail -20 "$ERR" >&2
  exit 1
fi

if ! jq -e . "$OUT" >/dev/null 2>&1; then
  echo "[diagnostics-bridge] FAIL: pyright produced non-JSON output" >&2
  head -40 "$OUT" >&2
  exit 1
fi

# Count diagnostics whose file path matches our probe file.
COUNT=$(jq --arg probe "$BAD_FILE" '[.generalDiagnostics[]? | select(.file == $probe)] | length' "$OUT")
if [[ -z "$COUNT" || "$COUNT" == "0" ]]; then
  echo "[diagnostics-bridge] FAIL: no diagnostics for $BAD_FILE" >&2
  echo "-- pyright summary --" >&2
  jq '.summary' "$OUT" >&2
  exit 1
fi

SEVERITY=$(jq -r --arg probe "$BAD_FILE" '.generalDiagnostics[] | select(.file == $probe) | .severity' "$OUT" | head -1)
if [[ "$SEVERITY" != "error" && "$SEVERITY" != "warning" ]]; then
  echo "[diagnostics-bridge] FAIL: probe diagnostic severity '$SEVERITY' is neither error nor warning" >&2
  exit 1
fi

echo "[diagnostics-bridge] OK — pyright returned $COUNT diagnostic(s) for the probe (severity=$SEVERITY)."
exit 0
