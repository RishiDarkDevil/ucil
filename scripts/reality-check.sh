#!/usr/bin/env bash
# Mutation-style check: stash the files changed for a feature, re-run the
# feature's acceptance tests — they must FAIL. Pop, re-run — they must PASS.
# Any "passes even when code is stashed" indicates a fake-green test.
#
# Usage: scripts/reality-check.sh <feature-id>
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

FEATURE_ID="${1:-}"
if [[ -z "$FEATURE_ID" ]]; then
  echo "Usage: $0 <feature-id>" >&2
  exit 2
fi

# Extract the feature entry
ENTRY=$(jq -c --arg id "$FEATURE_ID" '.features[] | select(.id == $id)' ucil-build/feature-list.json 2>/dev/null)
if [[ -z "$ENTRY" ]]; then
  echo "Feature $FEATURE_ID not found." >&2
  exit 2
fi

# Find the files changed by this feature's last substantive commit. The
# `last_verified_commit` field or the most-recent `Feature: $FEATURE_ID`
# trailer may point at an administrative commit (ready-for-review marker,
# verifier sign-off) that touches no source — walk backwards from there
# until we find a commit that actually changed source files.
extract_changed_source() {
  # `|| true` guards against pipefail tripping on a grep no-match (exit 1).
  git show --no-color --name-only --pretty=format: "$1" 2>/dev/null \
    | grep -E '\.(rs|ts|tsx|py)$' \
    | grep -v '^tests/' \
    | grep -v '^ucil-build/' \
    | sort -u \
    || true
}

START_COMMIT=$(echo "$ENTRY" | jq -r '.last_verified_commit // empty')
if [[ -z "$START_COMMIT" || "$START_COMMIT" == "null" ]]; then
  START_COMMIT=$(git log --grep="Feature: $FEATURE_ID" --format='%H' -1 || true)
fi
if [[ -z "$START_COMMIT" ]]; then
  echo "No commit found for feature $FEATURE_ID." >&2
  exit 3
fi

# Candidate commits, newest first: every commit trailed "Feature: $FEATURE_ID".
# Fall back to the chain START_COMMIT..HEAD~history.
CANDIDATES=$(git log --grep="Feature: $FEATURE_ID" --format='%H' || true)
if [[ -z "$CANDIDATES" ]]; then
  CANDIDATES="$START_COMMIT"
fi

LAST_COMMIT=""
CHANGED_FILES=""
for sha in $CANDIDATES; do
  files=$(extract_changed_source "$sha")
  if [[ -n "$files" ]]; then
    LAST_COMMIT="$sha"
    CHANGED_FILES="$files"
    break
  fi
done

if [[ -z "$LAST_COMMIT" ]]; then
  echo "No commit with source-file changes found for feature $FEATURE_ID — nothing to mutation-check."
  exit 0
fi

if [[ -z "$CHANGED_FILES" ]]; then
  echo "No source files changed by $LAST_COMMIT — nothing to mutation-check."
  exit 0
fi

echo "[reality-check] feature=$FEATURE_ID commit=$LAST_COMMIT"
echo "[reality-check] files:"
echo "$CHANGED_FILES" | sed 's/^/  /'

# Build the test command(s)
TESTS=$(echo "$ENTRY" | jq -c '.acceptance_tests[]')

run_acceptance() {
  while IFS= read -r t; do
    local kind
    kind=$(echo "$t" | jq -r .kind)
    case "$kind" in
      cargo_test)
        selector=$(echo "$t" | jq -r .selector)
        # shellcheck disable=SC2086
        cargo nextest run $selector --no-fail-fast 2>/dev/null || cargo test $selector --no-fail-fast
        ;;
      pytest)
        selector=$(echo "$t" | jq -r .selector)
        pytest "$selector"
        ;;
      vitest)
        selector=$(echo "$t" | jq -r .selector)
        npx vitest run "$selector"
        ;;
      script)
        path=$(echo "$t" | jq -r '.path // .script')
        bash "$path"
        ;;
      bench)
        # Mutation check skips benches (they measure perf, not correctness)
        true
        ;;
      *)
        echo "unknown acceptance test kind: $kind"
        return 1
        ;;
    esac
  done <<< "$TESTS"
}

# Stash the feature's files
echo ""
echo "[reality-check] Stashing $CHANGED_FILES"
git stash push -u -m "reality-check-$FEATURE_ID" -- $CHANGED_FILES

# Reset them to HEAD~1 state (before this commit) if possible, else delete
for f in $CHANGED_FILES; do
  if git cat-file -e "${LAST_COMMIT}^:$f" 2>/dev/null; then
    git show "${LAST_COMMIT}^:$f" > "$f" || true
  else
    rm -f "$f"
  fi
done

echo ""
echo "[reality-check] Running acceptance tests with code stashed — they MUST FAIL"
if run_acceptance >/tmp/reality-stashed.log 2>&1; then
  echo "[reality-check] FAILURE: acceptance tests PASSED with code stashed — this is a fake-green test."
  # Restore state
  git checkout HEAD -- $CHANGED_FILES 2>/dev/null || true
  git stash pop 2>/dev/null || true
  exit 1
else
  echo "[reality-check] OK: tests failed with code stashed (as expected)"
fi

# Restore
echo ""
echo "[reality-check] Restoring code and re-running — tests MUST PASS"
git checkout HEAD -- $CHANGED_FILES 2>/dev/null || true
git stash pop 2>/dev/null || true

if run_acceptance >/tmp/reality-restored.log 2>&1; then
  echo "[reality-check] OK: tests pass with code restored."
  exit 0
else
  echo "[reality-check] FAILURE: tests fail with code restored. Inconsistent state."
  exit 1
fi
