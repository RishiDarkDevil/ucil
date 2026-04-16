#!/usr/bin/env bash
# mutation-gate.sh — Anti-laziness mutation-score gate for a single crate.
#
# Usage: scripts/verify/mutation-gate.sh <crate> [<min_score>]
#
# Rationale:
#   Line/branch coverage is a necessary but not sufficient quality signal —
#   it's trivially gameable with tests that "exercise" code without
#   asserting anything meaningful. Mutation testing answers the harder
#   question: "if I perturb the code, do the tests actually catch it?"
#   We mandate a minimum mutation score (default 70%) for every shipped
#   Rust crate so verifiers can no longer rubber-stamp features whose
#   tests only cover the happy-path string comparison.
#
# Behavior:
#   - cd into crates/<crate>/; if absent → "no such crate, skip" exit 0.
#   - Run cargo-mutants in-place with a 120s per-mutant timeout.
#   - Parse mutants.out/outcomes.json for caught / missed / unviable counts.
#   - score = caught / (caught + missed) * 100; unviable mutants are
#     excluded from the denominator (they're builds that don't compile —
#     not a test-suite failure).
#   - Writes a structured report to ucil-build/verification-reports/mutation-<crate>.md.
#   - Exits 1 if score < min_score, 0 otherwise.
#   - If cargo-mutants isn't installed, writes a skip-with-warning report
#     and exits 0 so we don't block the gate on missing tooling; the
#     installer hook will nag separately.

set -uo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

# shellcheck source=scripts/_retry.sh
source "$REPO_ROOT/scripts/_retry.sh"

CRATE="${1:-}"
MIN_SCORE="${2:-70}"

if [[ -z "$CRATE" ]]; then
  echo "usage: $0 <crate> [<min_score>]" >&2
  exit 2
fi

REPORT_DIR="$REPO_ROOT/ucil-build/verification-reports"
mkdir -p "$REPORT_DIR"
REPORT="$REPORT_DIR/mutation-$CRATE.md"

write_report() {
  local verdict="$1"
  local body="$2"
  {
    echo "# Mutation Gate — $CRATE"
    echo ""
    echo "- **Verdict**: $verdict"
    echo "- **Min score**: ${MIN_SCORE}%"
    echo "- **Generated**: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo ""
    echo "$body"
  } > "$REPORT"
}

CRATE_DIR="$REPO_ROOT/crates/$CRATE"
if [[ ! -d "$CRATE_DIR" ]]; then
  write_report "SKIP" "Crate directory \`crates/$CRATE\` does not exist yet."
  echo "[mutation-gate] crate '$CRATE' absent — skipping."
  exit 0
fi

if ! command -v cargo-mutants >/dev/null 2>&1; then
  write_report "SKIP" "\`cargo-mutants\` is not installed. Run \`scripts/install-prereqs.sh\` to install it."
  echo "[mutation-gate] cargo-mutants not installed — skipping (exit 0)." >&2
  exit 0
fi

echo "[mutation-gate] Running cargo-mutants on '$CRATE' (min_score=${MIN_SCORE}%)..."
cd "$CRATE_DIR"

# Clean any prior output to force a fresh read.
rm -rf "$CRATE_DIR/mutants.out" "$REPO_ROOT/mutants.out"

# Capture full stdout+stderr for the report. Exit code is NOT load-bearing —
# cargo-mutants returns nonzero on ANY missed mutant, but the policy
# decision is score-based, not "zero misses".
MUT_LOG="$(mktemp -t mutation-gate-XXXXXX.log)"
trap 'rm -f "$MUT_LOG"' EXIT

cargo mutants --no-shuffle --timeout 120 --in-place >"$MUT_LOG" 2>&1 || true

# cargo-mutants writes mutants.out under the repo root or under the crate dir
# depending on workspace layout. Find whichever one was just written.
OUTCOMES_JSON=""
for candidate in "$CRATE_DIR/mutants.out/outcomes.json" "$REPO_ROOT/mutants.out/outcomes.json"; do
  if [[ -f "$candidate" ]]; then
    OUTCOMES_JSON="$candidate"
    break
  fi
done

if [[ -z "$OUTCOMES_JSON" ]]; then
  write_report "FAIL" "cargo-mutants produced no outcomes.json. Tail of log:\n\n\`\`\`\n$(tail -40 "$MUT_LOG")\n\`\`\`"
  echo "[mutation-gate] FAIL — no outcomes.json for $CRATE" >&2
  exit 1
fi

CAUGHT=$(jq '[.outcomes[] | select(.summary=="CaughtMutant")] | length' "$OUTCOMES_JSON")
MISSED=$(jq '[.outcomes[] | select(.summary=="MissedMutant")] | length' "$OUTCOMES_JSON")
UNVIABLE=$(jq '[.outcomes[] | select(.summary=="Unviable")] | length' "$OUTCOMES_JSON")
TIMEOUT=$(jq '[.outcomes[] | select(.summary=="Timeout")] | length' "$OUTCOMES_JSON")
FAILURE=$(jq '[.outcomes[] | select(.summary=="Failure")] | length' "$OUTCOMES_JSON")
TOTAL_VIABLE=$(( CAUGHT + MISSED ))

if (( TOTAL_VIABLE == 0 )); then
  write_report "FAIL" "No viable mutants generated for \`$CRATE\` (caught=$CAUGHT, missed=$MISSED, unviable=$UNVIABLE). A crate with zero viable mutants is either trivially empty or has a compile-only surface — in either case the mutation gate cannot vouch for test quality. Add meaningful logic or escalate via ADR."
  echo "[mutation-gate] FAIL — no viable mutants for $CRATE" >&2
  exit 1
fi

# Integer percent, truncating. 70 ≤ score → pass when MIN_SCORE=70.
SCORE=$(( CAUGHT * 100 / TOTAL_VIABLE ))

MISSED_LIST=""
if (( MISSED > 0 )); then
  MISSED_LIST=$(jq -r '.outcomes[] | select(.summary=="MissedMutant") | "- `" + (.scenario.Mutant.name // "unknown mutant") + "`"' "$OUTCOMES_JSON" 2>/dev/null || echo "- (unable to enumerate)")
fi

BODY=$(cat <<EOF
## Summary

| Metric       | Count |
|--------------|-------|
| Caught       | $CAUGHT |
| Missed       | $MISSED |
| Unviable     | $UNVIABLE |
| Timeout      | $TIMEOUT |
| Failure      | $FAILURE |
| **Score**    | **${SCORE}%** (caught / (caught+missed)) |

## Missed mutants

$(if (( MISSED > 0 )); then echo "$MISSED_LIST"; else echo "_None._"; fi)

## Raw outcomes

See \`mutants.out/outcomes.json\` in the crate directory for the full dump.
EOF
)

if (( SCORE < MIN_SCORE )); then
  write_report "FAIL" "$BODY

## Why this is failing

Mutation score ${SCORE}% is below the floor of ${MIN_SCORE}%. This means one
or more mutations (listed above) survived the test suite — i.e., the tests
passed even when the implementation was broken. Add assertions that would
fail under each listed mutant, then re-run."
  echo "[mutation-gate] FAIL — $CRATE score=${SCORE}% < min=${MIN_SCORE}%" >&2
  exit 1
fi

write_report "PASS" "$BODY"
echo "[mutation-gate] PASS — $CRATE score=${SCORE}% ≥ min=${MIN_SCORE}%"
exit 0
