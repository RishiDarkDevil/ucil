#!/usr/bin/env bash
# Multi-language coverage check — UCIL must work across Rust, Python,
# TypeScript, Go (and by Phase 5, also Java or C/C++).
#
# For each required language in the current phase, runs a probe query
# through UCIL's MCP that exercises `find_definition` against a known
# symbol in `tests/fixtures/<lang>-project/`. A non-stub response
# (`result._meta.found == true` with a real `file_path`) counts as a
# pass; a stub (`_meta.not_yet_implemented`) or an error response
# counts as a fail.
#
# Usage: scripts/verify/multi-lang-coverage.sh <phase>
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"

PHASE="${1:-$(jq -r .phase ucil-build/progress.json 2>/dev/null || echo 1)}"

# Required languages per phase
case "$PHASE" in
  1|2) REQUIRED="rust python typescript" ;;
  3|3.5|4) REQUIRED="rust python typescript go" ;;
  5|6|7|8) REQUIRED="rust python typescript go" ;;
  *) REQUIRED="" ;;
esac

if [[ -z "$REQUIRED" ]]; then
  echo "[multi-lang] no languages required for phase $PHASE"
  exit 0
fi

echo "[multi-lang] phase=$PHASE required=$REQUIRED"

# Known symbol per fixture project — pick one that has a clear definition
# in the project tree so `find_definition` returns a non-stub result.
declare -A FIXTURE_DIR=(
  [rust]="tests/fixtures/rust-project"
  [python]="tests/fixtures/python-project"
  [typescript]="tests/fixtures/typescript-project"
  [go]="tests/fixtures/go-project"
)
declare -A PROBE_SYMBOL=(
  [rust]="EvalContext"
  [python]="Evaluator"
  [typescript]="FilterEngine"
  [go]="Server"
)

DAEMON=./target/debug/ucil-daemon
if [[ ! -x "$DAEMON" ]]; then
  echo "[multi-lang] building ucil-daemon..."
  if ! cargo build -p ucil-daemon --bin ucil-daemon --quiet 2>&1 | tail -10; then
    echo "[multi-lang] FAIL: cargo build -p ucil-daemon failed" >&2
    exit 1
  fi
fi

TMP=$(mktemp -d -t multi-lang-probes-XXXXXX)
trap 'rm -rf "$TMP"' EXIT

FAIL_COUNT=0
for LANG in $REQUIRED; do
  DIR="${FIXTURE_DIR[$LANG]:-}"
  SYM="${PROBE_SYMBOL[$LANG]:-}"
  if [[ -z "$DIR" || -z "$SYM" ]]; then
    echo "[multi-lang] $LANG FAIL — no fixture/symbol mapping configured" >&2
    FAIL_COUNT=$((FAIL_COUNT + 1))
    continue
  fi
  if [[ ! -d "$DIR" ]]; then
    echo "[multi-lang] $LANG FAIL — fixture dir missing: $DIR" >&2
    FAIL_COUNT=$((FAIL_COUNT + 1))
    continue
  fi

  INIT='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"multi-lang-probe","version":"1.0.0"}}}'
  CALL=$(jq -cn --arg name "$SYM" '{
    jsonrpc:"2.0",id:2,method:"tools/call",
    params:{
      name:"find_definition",
      arguments:{
        name:$name,reason:"multi-lang probe",current_task:"multi-lang probe",
        files_in_context:[],token_budget:8192
      }
    }
  }')

  OUT="$TMP/$LANG-out.jsonl"
  ERR="$TMP/$LANG-err.log"
  printf '%s\n%s\n' "$INIT" "$CALL" \
    | timeout 30 "$DAEMON" mcp --stdio --repo "$DIR" > "$OUT" 2>"$ERR"
  rc=$?
  if [[ $rc -ne 0 && $rc -ne 124 ]]; then
    echo "[multi-lang] $LANG FAIL — daemon exited $rc" >&2
    tail -10 "$ERR" >&2
    FAIL_COUNT=$((FAIL_COUNT + 1))
    continue
  fi

  RESP=$(jq -c 'select(.id == 2)' < "$OUT" 2>/dev/null | head -1)
  if [[ -z "$RESP" ]]; then
    echo "[multi-lang] $LANG FAIL — no response to find_definition" >&2
    FAIL_COUNT=$((FAIL_COUNT + 1))
    continue
  fi

  NYI=$(jq -r '.result._meta.not_yet_implemented // false' <<<"$RESP")
  FOUND=$(jq -r '.result._meta.found // false' <<<"$RESP")
  FILE=$(jq -r '.result._meta.file_path // ""' <<<"$RESP")
  ERR_MSG=$(jq -r '.error.message // ""' <<<"$RESP")

  if [[ -n "$ERR_MSG" ]]; then
    echo "[multi-lang] $LANG FAIL — RPC error: $ERR_MSG" >&2
    FAIL_COUNT=$((FAIL_COUNT + 1))
    continue
  fi
  if [[ "$NYI" == "true" ]]; then
    echo "[multi-lang] $LANG FAIL — find_definition stub (not_yet_implemented)" >&2
    FAIL_COUNT=$((FAIL_COUNT + 1))
    continue
  fi
  if [[ "$FOUND" != "true" || -z "$FILE" ]]; then
    echo "[multi-lang] $LANG FAIL — symbol '$SYM' not found in fixture $DIR" >&2
    FAIL_COUNT=$((FAIL_COUNT + 1))
    continue
  fi

  echo "[multi-lang] $LANG OK — $SYM resolved to $FILE"
done

if (( FAIL_COUNT > 0 )); then
  echo "[multi-lang] FAIL — $FAIL_COUNT language(s) failed the probe" >&2
  exit 1
fi

echo "[multi-lang] OK — all $(wc -w <<<"$REQUIRED") required language(s) passed"
exit 0
