#!/usr/bin/env bash
# Dogfood test — UCIL indexes the ucil repo itself and answers a query
# we know the correct answer to.
#
# The original spec called for two natural-language queries answered
# through the agent layer ("find the function that flips a feature's
# passes=true" + "explain how the verifier session marker works"),
# which depends on Phase 3.5 + 4 agent infrastructure. At Phase 3 the
# achievable dogfood is the structural slice: spawn `daemon mcp` over
# the ucil repo, ask find_definition for a known UCIL Rust symbol,
# and assert the response (a) is non-stub and (b) points to a real
# crates/ path. That is "UCIL ate its own source and pointed back at
# itself" — the meta-test the spec is after.
#
# When the agent layer lands, this script should grow the two NL
# probes back in. Track via DEC-* if/when that happens.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"
PHASE="${1:-$(jq -r .phase ucil-build/progress.json 2>/dev/null || echo 1)}"

case "$PHASE" in
  0|1|2) echo "[dogfood] phase $PHASE: not required (needs Phase 3 agents)"; exit 0 ;;
  *) ;;
esac

echo "[dogfood] phase=$PHASE"

DAEMON=./target/debug/ucil-daemon
if [[ ! -x "$DAEMON" ]]; then
  echo "[dogfood] building ucil-daemon..."
  if ! cargo build -p ucil-daemon --bin ucil-daemon --quiet 2>&1 | tail -10; then
    echo "[dogfood] FAIL: cargo build -p ucil-daemon failed" >&2
    exit 1
  fi
fi

REPO="$(git rev-parse --show-toplevel)"
TMP=$(mktemp -d -t dogfood-XXXXXX)
trap 'rm -rf "$TMP"' EXIT

# Two known structural anchors in UCIL Rust source — pick symbols that
# are defined in exactly one crate so we can assert the response file
# matches a fixed path. PidFile lives in ucil-daemon/src/lifecycle.rs;
# BranchManager lives in ucil-daemon/src/branch_manager.rs. If either
# moves, the assertion below documents the new home — that's the
# point.
declare -A EXPECT=(
  [PidFile]="crates/ucil-daemon/src/lifecycle.rs"
  [BranchManager]="crates/ucil-daemon/src/branch_manager.rs"
)

INIT='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"dogfood-probe","version":"1.0.0"}}}'

FAIL_COUNT=0
for sym in "${!EXPECT[@]}"; do
  expect="${EXPECT[$sym]}"
  out="$TMP/$sym.jsonl"
  err="$TMP/$sym.err"

  call=$(jq -cn --arg name "$sym" '{
    jsonrpc:"2.0",id:2,method:"tools/call",
    params:{
      name:"find_definition",
      arguments:{
        name:$name,reason:"ucil dogfood probe",current_task:"ucil dogfood probe",
        files_in_context:[],token_budget:8192
      }
    }
  }')

  printf '%s\n%s\n' "$INIT" "$call" \
    | timeout 60 "$DAEMON" mcp --stdio --repo "$REPO" >"$out" 2>"$err"
  rc=$?
  if [[ $rc -ne 0 && $rc -ne 124 ]]; then
    echo "[dogfood] $sym FAIL — daemon exited $rc" >&2
    tail -10 "$err" >&2
    FAIL_COUNT=$((FAIL_COUNT + 1))
    continue
  fi

  resp=$(jq -c 'select(.id == 2)' <"$out" 2>/dev/null | head -1)
  if [[ -z "$resp" ]]; then
    echo "[dogfood] $sym FAIL — no response" >&2
    FAIL_COUNT=$((FAIL_COUNT + 1))
    continue
  fi

  nyi=$(jq -r '.result._meta.not_yet_implemented // false' <<<"$resp")
  found=$(jq -r '.result._meta.found // false' <<<"$resp")
  file=$(jq -r '.result._meta.file_path // ""' <<<"$resp")
  err_msg=$(jq -r '.error.message // ""' <<<"$resp")

  if [[ -n "$err_msg" ]]; then
    echo "[dogfood] $sym FAIL — RPC error: $err_msg" >&2
    FAIL_COUNT=$((FAIL_COUNT + 1))
    continue
  fi
  if [[ "$nyi" == "true" ]]; then
    echo "[dogfood] $sym FAIL — find_definition stub (not_yet_implemented)" >&2
    FAIL_COUNT=$((FAIL_COUNT + 1))
    continue
  fi
  if [[ "$found" != "true" || -z "$file" ]]; then
    echo "[dogfood] $sym FAIL — symbol not resolved against UCIL repo" >&2
    FAIL_COUNT=$((FAIL_COUNT + 1))
    continue
  fi

  # Strip any leading "$REPO/" prefix the daemon may emit so the
  # comparison is path-agnostic.
  rel="${file#"$REPO"/}"
  if [[ "$rel" != "$expect" ]]; then
    echo "[dogfood] $sym FAIL — resolved to $rel, expected $expect" >&2
    FAIL_COUNT=$((FAIL_COUNT + 1))
    continue
  fi

  echo "[dogfood] $sym OK — resolved to $rel"
done

if (( FAIL_COUNT > 0 )); then
  echo "[dogfood] FAIL — $FAIL_COUNT UCIL self-symbol probe(s) failed" >&2
  exit 1
fi

echo "[dogfood] OK — UCIL pointed back at itself for all probes"
exit 0
