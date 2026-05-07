#!/usr/bin/env bash
# Real OSS repo smoke test — clone a small real Rust repo, point the
# ucil-daemon MCP server at it, send a handful of JSON-RPC frames over
# stdio (initialize / tools/list / find_definition / search_code), and
# assert the daemon answered each one without erroring.
#
# Synthetic fixtures don't prove robustness against real-world layouts,
# missing deps, README-only commits, or vendored sub-crates. This pass
# is the first in the gate that exercises the full clone → index → query
# pipeline against an unmodified upstream repo.
#
# Gate budget: should complete in <10 min on a warm cargo cache.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"
PHASE="${1:-$(jq -r .phase ucil-build/progress.json)}"

case "$PHASE" in
  0|1) echo "[real-repo-smoke] phase $PHASE: not required"; exit 0 ;;
  *) ;;
esac

echo "[real-repo-smoke] phase=$PHASE"

# Default to a small, stable Rust crate. Override via env for ad-hoc testing.
REPO_URL="${UCIL_REAL_REPO_URL:-https://github.com/rust-lang/log}"
REPO_TAG="$(basename "$REPO_URL" .git)-$(date +%s)"
WORK="/tmp/ucil-realrepo-${REPO_TAG}"
PROBE_OUT="$WORK/probe.out"
DAEMON_ERR="$WORK/daemon.err"
REQ="$WORK/req.jsonl"
trap 'rm -rf "$WORK"' EXIT

mkdir -p "$WORK"

echo "[real-repo-smoke] cloning $REPO_URL (depth=1)..."
if ! git clone --quiet --depth 1 "$REPO_URL" "$WORK/repo" 2>"$WORK/clone.err"; then
  echo "[real-repo-smoke] FAIL — clone failed"
  head -10 "$WORK/clone.err" 2>/dev/null || true
  exit 1
fi

LOC=$(find "$WORK/repo" \( -name '*.rs' -o -name '*.py' -o -name '*.ts' \) -print0 \
        2>/dev/null | xargs -0 wc -l 2>/dev/null | tail -1 | awk '{print $1+0}')
echo "[real-repo-smoke] cloned, ~${LOC:-?} source lines"

DAEMON_BIN=""
for cand in target/release/ucil-daemon target/debug/ucil-daemon; do
  if [[ -x "$cand" ]]; then DAEMON_BIN="$cand"; break; fi
done
if [[ -z "$DAEMON_BIN" ]]; then
  echo "[real-repo-smoke] building ucil-daemon (debug)..."
  if ! cargo build --quiet -p ucil-daemon 2>"$WORK/build.err"; then
    echo "[real-repo-smoke] FAIL — daemon build failed"
    tail -10 "$WORK/build.err" 2>/dev/null || true
    exit 1
  fi
  DAEMON_BIN="target/debug/ucil-daemon"
fi
echo "[real-repo-smoke] daemon=$DAEMON_BIN"

: >"$REQ"
{
  echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"real-repo-smoke","version":"0.1"}}}'
  echo '{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}'
  echo '{"jsonrpc":"2.0","id":2,"method":"tools/list"}'
  echo '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"find_definition","arguments":{"symbol":"Log"}}}'
  echo '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"search_code","arguments":{"query":"log level"}}}'
} >"$REQ"

echo "[real-repo-smoke] sending 5 JSON-RPC frames (timeout 300s)..."
if ! timeout 300 "$DAEMON_BIN" mcp --stdio --repo "$WORK/repo" <"$REQ" >"$PROBE_OUT" 2>"$DAEMON_ERR"; then
  rc=$?
  echo "[real-repo-smoke] FAIL — daemon stdio call failed (rc=$rc)"
  echo "--- daemon stderr (tail 20) ---"
  tail -20 "$DAEMON_ERR" 2>/dev/null || true
  echo "--- daemon stdout (tail 20) ---"
  tail -20 "$PROBE_OUT" 2>/dev/null || true
  exit 1
fi

fail=0
check_id() {
  local id="$1" label="$2"
  if ! grep -qE "\"id\":${id}\\b" "$PROBE_OUT"; then
    echo "[real-repo-smoke] FAIL — ${label} response (id=${id}) missing"
    fail=1
  fi
}
check_id 1 "initialize"
check_id 2 "tools/list"
check_id 3 "find_definition"
check_id 4 "search_code"

if ! grep -qE '"name":"find_definition"' "$PROBE_OUT"; then
  echo "[real-repo-smoke] FAIL — find_definition not advertised in tools/list"
  fail=1
fi

if ! grep -qE '"name":"search_code"' "$PROBE_OUT"; then
  echo "[real-repo-smoke] FAIL — search_code not advertised in tools/list"
  fail=1
fi

if grep -qE '"id":3,"error"' "$PROBE_OUT"; then
  echo "[real-repo-smoke] FAIL — find_definition returned JSON-RPC error"
  fail=1
fi
if grep -qE '"id":4,"error"' "$PROBE_OUT"; then
  echo "[real-repo-smoke] FAIL — search_code returned JSON-RPC error"
  fail=1
fi

if [[ "$fail" -eq 0 ]]; then
  echo "[real-repo-smoke] PASS — daemon answered initialize+tools/list+find_definition+search_code against $REPO_URL (~${LOC:-?} source lines)"
  exit 0
fi

echo "--- probe output (tail 40) ---"
tail -40 "$PROBE_OUT" 2>/dev/null || true
echo "--- daemon stderr (tail 20) ---"
tail -20 "$DAEMON_ERR" 2>/dev/null || true
exit 1
