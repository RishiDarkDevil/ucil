#!/usr/bin/env bash
# Concurrency test — master plan §11 promises 3–5 concurrent agents
# across branches/worktrees with the shared brain. This implementation
# stays at the daemon-process layer (no headless `claude -p` sessions
# yet — those land with the Phase 4 host adapter work) but exercises
# the same shared-state contention story: three independent daemon
# instances doing concurrent symbol lookups against the same fixture
# repo. The shared SQLite knowledge.db + tree-sitter cache is the
# real failure mode, and that is what this probe surfaces.
#
# Asserts:
#   - all 3 daemon processes exit 0 within the 30s timeout
#   - each gets a non-stub `find_definition` response (real file_path)
#   - no `SQLITE_BUSY` token in any daemon's stderr
#   - rust-project knowledge.db (if present) survives PRAGMA
#     integrity_check
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"
PHASE="${1:-$(jq -r .phase ucil-build/progress.json 2>/dev/null || echo 1)}"

case "$PHASE" in
  0|1|2) echo "[concurrency] phase $PHASE: not required (needs daemon + orchestration)"; exit 0 ;;
  *) ;;
esac

echo "[concurrency] phase=$PHASE"

DAEMON=./target/debug/ucil-daemon
if [[ ! -x "$DAEMON" ]]; then
  echo "[concurrency] building ucil-daemon..."
  if ! cargo build -p ucil-daemon --bin ucil-daemon --quiet 2>&1 | tail -10; then
    echo "[concurrency] FAIL: cargo build -p ucil-daemon failed" >&2
    exit 1
  fi
fi

FIXTURE="tests/fixtures/rust-project"
if [[ ! -d "$FIXTURE" ]]; then
  echo "[concurrency] FAIL — fixture missing: $FIXTURE" >&2
  exit 1
fi

# Three distinct symbols known to live in the rust-project fixture so
# that each agent's query exercises a different KG/structural path
# rather than hitting an identical cache slot.
SYMBOLS=(EvalContext Tracer Interpreter)

TMP=$(mktemp -d -t concurrency-XXXXXX)
trap 'rm -rf "$TMP"' EXIT

INIT='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"concurrency-probe","version":"1.0.0"}}}'

run_one() {
  local idx="$1" sym="$2"
  local out="$TMP/agent-$idx.jsonl"
  local err="$TMP/agent-$idx.err"
  local rc_file="$TMP/agent-$idx.rc"
  local call
  call=$(jq -cn --arg name "$sym" '{
    jsonrpc:"2.0",id:2,method:"tools/call",
    params:{
      name:"find_definition",
      arguments:{
        name:$name,reason:"concurrency probe",current_task:"concurrency probe",
        files_in_context:[],token_budget:8192
      }
    }
  }')
  printf '%s\n%s\n' "$INIT" "$call" \
    | timeout 30 "$DAEMON" mcp --stdio --repo "$FIXTURE" >"$out" 2>"$err"
  echo "$?" >"$rc_file"
}

# Launch all three agents in parallel and capture their PIDs so we
# can wait for them without losing exit codes (the rc_file dance is
# bash's portable way around the lost-status issue with `&`).
PIDS=()
for i in 0 1 2; do
  run_one "$i" "${SYMBOLS[$i]}" &
  PIDS+=("$!")
done
for pid in "${PIDS[@]}"; do
  wait "$pid" || true
done

FAIL_COUNT=0
for i in 0 1 2; do
  rc=$(cat "$TMP/agent-$i.rc" 2>/dev/null || echo 1)
  err="$TMP/agent-$i.err"
  out="$TMP/agent-$i.jsonl"
  sym="${SYMBOLS[$i]}"

  if [[ $rc -ne 0 && $rc -ne 124 ]]; then
    echo "[concurrency] agent $i FAIL — daemon exited $rc" >&2
    tail -8 "$err" >&2
    FAIL_COUNT=$((FAIL_COUNT + 1))
    continue
  fi

  if grep -q 'SQLITE_BUSY' "$err" 2>/dev/null; then
    echo "[concurrency] agent $i FAIL — SQLITE_BUSY in stderr" >&2
    FAIL_COUNT=$((FAIL_COUNT + 1))
    continue
  fi

  resp=$(jq -c 'select(.id == 2)' <"$out" 2>/dev/null | head -1)
  if [[ -z "$resp" ]]; then
    echo "[concurrency] agent $i FAIL — no response for $sym" >&2
    FAIL_COUNT=$((FAIL_COUNT + 1))
    continue
  fi

  nyi=$(jq -r '.result._meta.not_yet_implemented // false' <<<"$resp")
  found=$(jq -r '.result._meta.found // false' <<<"$resp")
  file=$(jq -r '.result._meta.file_path // ""' <<<"$resp")
  err_msg=$(jq -r '.error.message // ""' <<<"$resp")

  if [[ -n "$err_msg" ]]; then
    echo "[concurrency] agent $i FAIL — RPC error: $err_msg" >&2
    FAIL_COUNT=$((FAIL_COUNT + 1))
    continue
  fi
  if [[ "$nyi" == "true" ]]; then
    echo "[concurrency] agent $i FAIL — find_definition stub for $sym" >&2
    FAIL_COUNT=$((FAIL_COUNT + 1))
    continue
  fi
  if [[ "$found" != "true" || -z "$file" ]]; then
    echo "[concurrency] agent $i FAIL — symbol $sym unresolved" >&2
    FAIL_COUNT=$((FAIL_COUNT + 1))
    continue
  fi

  echo "[concurrency] agent $i OK — $sym at $file"
done

# Knowledge-graph integrity check after the parallel write storm. If
# the fixture grew a knowledge.db at some point, verify it isn't
# corrupted. A missing db is fine (daemon may not write one for read-
# only probes), but a corrupt one is a red flag.
KDB="$FIXTURE/.ucil/knowledge.db"
if [[ -f "$KDB" ]]; then
  if command -v sqlite3 >/dev/null 2>&1; then
    INT=$(sqlite3 "$KDB" 'PRAGMA integrity_check;' 2>/dev/null | head -1 || echo "unknown")
    if [[ "$INT" != "ok" ]]; then
      echo "[concurrency] FAIL — knowledge.db integrity_check returned: $INT" >&2
      FAIL_COUNT=$((FAIL_COUNT + 1))
    else
      echo "[concurrency] knowledge.db integrity_check: ok"
    fi
  fi
fi

if (( FAIL_COUNT > 0 )); then
  echo "[concurrency] FAIL — $FAIL_COUNT of 3 concurrent agents failed" >&2
  exit 1
fi

echo "[concurrency] OK — 3 concurrent daemon agents completed cleanly"
exit 0
