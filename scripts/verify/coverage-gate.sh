#!/usr/bin/env bash
# coverage-gate.sh — Line/branch coverage floor for a single crate.
#
# Usage: scripts/verify/coverage-gate.sh <crate> [<min_line>] [<min_branch>]
#
# Defaults: min_line=85, min_branch=75.
#
# Rationale:
#   Coverage is a necessary-but-not-sufficient signal — paired with
#   mutation-gate.sh, it catches the complementary failure mode: "tests
#   exist but don't execute the new code path at all". Together the two
#   gates force executors past the "one happy-path test with an eq-assert"
#   anti-pattern that dominated the fake-green rejections from Phase 0.
#
# Behavior:
#   - cd into crates/<crate>/; if absent → "no such crate, skip" exit 0.
#   - If cargo-llvm-cov isn't installed, skip with a warning (exit 0) so
#     gate scripts can call this idempotently before the tooling ships.
#   - Emit JSON summary via `cargo llvm-cov --summary-only --json`.
#   - Parse totals.lines.percent and totals.branches.percent.
#   - Write a structured report to ucil-build/verification-reports/coverage-<crate>.md.
#   - Exit 1 if line% < min_line OR branch% < min_branch, else 0.

set -uo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

# shellcheck source=scripts/_retry.sh
source "$REPO_ROOT/scripts/_retry.sh"

CRATE="${1:-}"
MIN_LINE="${2:-85}"
MIN_BRANCH="${3:-75}"

if [[ -z "$CRATE" ]]; then
  echo "usage: $0 <crate> [<min_line>] [<min_branch>]" >&2
  exit 2
fi

REPORT_DIR="$REPO_ROOT/ucil-build/verification-reports"
mkdir -p "$REPORT_DIR"
REPORT="$REPORT_DIR/coverage-$CRATE.md"

write_report() {
  local verdict="$1"
  local body="$2"
  {
    echo "# Coverage Gate — $CRATE"
    echo ""
    echo "- **Verdict**: $verdict"
    echo "- **Min line coverage**: ${MIN_LINE}%"
    echo "- **Min branch coverage**: ${MIN_BRANCH}%"
    echo "- **Generated**: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo ""
    echo "$body"
  } > "$REPORT"
}

CRATE_DIR="$REPO_ROOT/crates/$CRATE"
if [[ ! -d "$CRATE_DIR" ]]; then
  write_report "SKIP" "Crate directory \`crates/$CRATE\` does not exist yet."
  echo "[coverage-gate] crate '$CRATE' absent — skipping."
  exit 0
fi

# cargo-llvm-cov is a cargo subcommand; detect via `cargo llvm-cov --version`.
if ! cargo llvm-cov --version >/dev/null 2>&1; then
  write_report "SKIP" "\`cargo-llvm-cov\` is not installed. Install via \`cargo install cargo-llvm-cov --locked\` and ensure \`rustup component add llvm-tools-preview\` is present."
  echo "[coverage-gate] cargo-llvm-cov not installed — skipping (exit 0)." >&2
  exit 0
fi

echo "[coverage-gate] Running cargo llvm-cov on '$CRATE' (min_line=${MIN_LINE}%, min_branch=${MIN_BRANCH}%)..."

# We want per-crate coverage, scoped to this package only. `--package`
# filter + `--summary-only --json` gives a compact numeric report with a
# `totals` object.
JSON_OUT="$(mktemp -t coverage-gate-XXXXXX.json)"
LOG_OUT="$(mktemp -t coverage-gate-XXXXXX.log)"
trap 'rm -f "$JSON_OUT" "$LOG_OUT"' EXIT

# Clean any stale .profraw files from prior runs. Corrupt-header
# profraw (not zero-byte) from interrupted processes and from running
# cargo-test directly without show-env wrappers breaks
# `llvm-profdata merge` with "invalid instrumentation profile data".
# `cargo llvm-cov clean --workspace` wipes them atomically and is
# idempotent.
cargo llvm-cov clean --workspace >/dev/null 2>&1 || true

# Set up env for two-step run: we need to prune corrupt profraw files
# between `cargo test` and `cargo llvm-cov report`. Integration tests
# that spawn subprocesses (e.g. e2e_mcp_stdio* in ucil-daemon) leave
# behind profraw files from killed children whose headers are truncated.
# `cargo llvm-cov test` wraps both steps atomically but then `llvm-profdata
# merge` refuses the set with "no profile can be merged". Staging lets
# us run a prune step before the merge.
#
# shellcheck disable=SC1090
source <(cargo llvm-cov show-env --export-prefix 2>/dev/null || true)

TEST_LOG="$(mktemp -t coverage-gate-test-XXXXXX.log)"
trap 'rm -f "$JSON_OUT" "$LOG_OUT" "$TEST_LOG"' EXIT

if ! cargo test --package "$CRATE" >"$TEST_LOG" 2>&1; then
  write_report "FAIL" "\`cargo test -p $CRATE\` failed under coverage instrumentation. Tail of log:

\`\`\`
$(tail -40 "$TEST_LOG")
\`\`\`"
  echo "[coverage-gate] FAIL — cargo test under coverage errored for $CRATE" >&2
  exit 1
fi

# Prune zero-byte AND corrupt-header .profraw files. `llvm-profdata
# show` exits non-zero on corrupt files without modifying them, so we
# can use it as a sanity check. Without this, crates whose tests spawn
# subprocesses (eg. ucil-daemon's e2e_mcp_stdio tests) leave garbage
# profraw that breaks the merge step.
PROFRAW_DIR="${CARGO_LLVM_COV_TARGET_DIR:-$REPO_ROOT/target/llvm-cov-target}"
LLVM_PROFDATA="$(rustc --print target-libdir)/../bin/llvm-profdata"
if [[ -d "$PROFRAW_DIR" && -x "$LLVM_PROFDATA" ]]; then
  # Zero-byte first (cheap), then corrupt-header (validate each).
  find "$PROFRAW_DIR" -name '*.profraw' -size 0 -delete 2>/dev/null || true
  find "$PROFRAW_DIR" -name '*.profraw' -print0 2>/dev/null \
    | while IFS= read -r -d '' f; do
        if ! "$LLVM_PROFDATA" show "$f" >/dev/null 2>&1; then
          rm -f "$f"
        fi
      done
fi

if ! cargo llvm-cov report --package "$CRATE" --summary-only --json \
       >"$JSON_OUT" 2>"$LOG_OUT"; then
  write_report "FAIL" "\`cargo llvm-cov report\` failed after profraw prune. Tail of log:

\`\`\`
$(tail -40 "$LOG_OUT")
\`\`\`"
  echo "[coverage-gate] FAIL — cargo llvm-cov report errored for $CRATE" >&2
  exit 1
fi

if ! jq -e . "$JSON_OUT" >/dev/null 2>&1; then
  write_report "FAIL" "\`cargo llvm-cov\` produced non-JSON output. Contents:

\`\`\`
$(head -80 "$JSON_OUT")
\`\`\`"
  echo "[coverage-gate] FAIL — non-JSON output from cargo llvm-cov" >&2
  exit 1
fi

# cargo llvm-cov JSON schema (LLVM json-export v2) uses:
#   .data[0].totals.lines.percent
#   .data[0].totals.branches.percent   (may be 0 with count==0 on stable
#                                       — branch cov is nightly-gated)
LINE_PCT=$(jq -r '.data[0].totals.lines.percent // 0' "$JSON_OUT")
BRANCH_PCT=$(jq -r '.data[0].totals.branches.percent // null' "$JSON_OUT")
BRANCH_COUNT=$(jq -r '.data[0].totals.branches.count // 0' "$JSON_OUT")

# Normalise to integer percent (truncating) for gate comparison. Keep
# the raw float for the report so the delta is visible.
LINE_INT=$(printf '%.0f' "$LINE_PCT" 2>/dev/null || echo 0)

# Branch coverage is unstable in cargo-llvm-cov on stable Rust — the JSON
# contains branches={count:0, covered:0, percent:0} when it wasn't actually
# measured. Treat count==0 as "unavailable" rather than "0% coverage".
if [[ "$BRANCH_PCT" == "null" || -z "$BRANCH_PCT" ]] || (( BRANCH_COUNT == 0 )); then
  BRANCH_INT="-1"   # sentinel: unavailable
else
  BRANCH_INT=$(printf '%.0f' "$BRANCH_PCT" 2>/dev/null || echo 0)
fi

FAIL=0
FAIL_REASONS=""

if (( LINE_INT < MIN_LINE )); then
  FAIL=1
  FAIL_REASONS="$FAIL_REASONS\n- Line coverage ${LINE_INT}% < floor ${MIN_LINE}% (delta: $(( MIN_LINE - LINE_INT ))pp)."
fi

if (( BRANCH_INT >= 0 )) && (( BRANCH_INT < MIN_BRANCH )); then
  FAIL=1
  FAIL_REASONS="$FAIL_REASONS\n- Branch coverage ${BRANCH_INT}% < floor ${MIN_BRANCH}% (delta: $(( MIN_BRANCH - BRANCH_INT ))pp)."
fi

# Compose report body
if [[ "$BRANCH_INT" == "-1" ]]; then
  BRANCH_ROW="| Branch       | _unavailable (toolchain)_ |"
else
  BRANCH_ROW="| Branch       | ${BRANCH_PCT}% (floor ${MIN_BRANCH}%) |"
fi

BODY=$(cat <<EOF
## Summary

| Metric       | Value |
|--------------|-------|
| Line         | ${LINE_PCT}% (floor ${MIN_LINE}%) |
$BRANCH_ROW

## Raw JSON

\`\`\`
$(jq '.data[0].totals' "$JSON_OUT" 2>/dev/null || head -20 "$JSON_OUT")
\`\`\`
EOF
)

if (( BRANCH_INT < 0 )); then
  BRANCH_LOG="branch=n/a"
else
  BRANCH_LOG="branch=${BRANCH_INT}%"
fi

if (( FAIL == 1 )); then
  write_report "FAIL" "$BODY

## Failures

$(printf '%b' "$FAIL_REASONS")

## Why this is failing

Coverage below the floor means code paths exist that no test ever
exercises. Combine this with mutation-gate: if a new file has 95% line
coverage but 40% mutation score, the tests run the lines without
asserting on their effects. Address both dimensions."
  echo "[coverage-gate] FAIL — $CRATE line=${LINE_INT}% $BRANCH_LOG" >&2
  exit 1
fi

write_report "PASS" "$BODY"
echo "[coverage-gate] PASS — $CRATE line=${LINE_INT}% $BRANCH_LOG"
exit 0
