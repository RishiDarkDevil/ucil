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
CANDIDATES=$(git log --grep="Feature: $FEATURE_ID" --format='%H' || true)
if [[ -z "$CANDIDATES" ]]; then
  CANDIDATES="$START_COMMIT"
fi

# UNION source files across every commit tagged for this feature. A feature
# can span multiple commits (e.g. impl commit + lib.rs wiring commit + test
# file commit) — stashing only one of those leaves the module intact via
# the others, producing a fake-green mutation check. We must stash the
# full set.
LAST_COMMIT=""            # newest commit that still touched source (for reporting)
declare -A _seen_files
UNION_FILES=""
for sha in $CANDIDATES; do
  files=$(extract_changed_source "$sha")
  if [[ -n "$files" ]]; then
    [[ -z "$LAST_COMMIT" ]] && LAST_COMMIT="$sha"
    while IFS= read -r f; do
      [[ -z "$f" ]] && continue
      if [[ -z "${_seen_files[$f]:-}" ]]; then
        _seen_files[$f]=1
        UNION_FILES+="$f"$'\n'
      fi
    done <<< "$files"
  fi
done
CHANGED_FILES=$(echo "$UNION_FILES" | grep -v '^$' | sort -u)

if [[ -z "$LAST_COMMIT" ]] || [[ -z "$CHANGED_FILES" ]]; then
  echo "No commits with source-file changes found for feature $FEATURE_ID — nothing to mutation-check."
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

# Per-file rollback baseline: for each file, find the commit in CANDIDATES
# (ordered newest→oldest) that actually touched THAT file. Roll it back to
# THAT commit's parent. A single $LAST_COMMIT^ baseline fails when files
# were introduced in different commits (e.g. types.rs added in commit A
# then lib.rs wired in commit B — rolling lib.rs to B^ leaves types.rs
# intact, and rolling types.rs to B^ leaves its own current content because
# types.rs existed before commit B).
declare -A _file_commit
for f in $CHANGED_FILES; do
  for sha in $CANDIDATES; do
    if git show --name-only --pretty=format: "$sha" 2>/dev/null | grep -qxF "$f"; then
      _file_commit["$f"]="$sha"
      break
    fi
  done
done

# Stash the feature's files
echo ""
echo "[reality-check] Stashing:"
echo "$CHANGED_FILES" | sed 's/^/  /'
git stash push -u -m "reality-check-$FEATURE_ID" -- $CHANGED_FILES

# Reset each file to its own introducing-commit's parent state (or delete if
# the file didn't exist before the introducing commit).
for f in $CHANGED_FILES; do
  sha="${_file_commit[$f]:-$LAST_COMMIT}"
  if git cat-file -e "${sha}^:$f" 2>/dev/null; then
    git show "${sha}^:$f" > "$f" || true
  else
    rm -f "$f"
  fi
done

echo ""
echo "[reality-check] Running acceptance tests with code stashed — they MUST FAIL"
# Capture whether any tests ran at all. cargo nextest/cargo test exit 0 when
# the selector matches 0 tests (undeclared module / removed test file). A
# zero-test pass is a fake-green and must be treated as FAILURE here.
set +e
run_acceptance >/tmp/reality-stashed.log 2>&1
stashed_rc=$?
set -e

# Detect the zero-tests case: any of the tools emits a "0 passed" marker.
zero_tests=0
if grep -qE '(0 tests? passed|Running 0 tests|no tests found|collected 0 items|0 passed|0 filtered in)' /tmp/reality-stashed.log; then
  zero_tests=1
fi

if [[ "$stashed_rc" -eq 0 ]] || [[ "$zero_tests" -eq 1 ]]; then
  if [[ "$zero_tests" -eq 1 ]]; then
    echo "[reality-check] FAILURE: acceptance tests reported ZERO tests run with code stashed — module was removed, not a genuine pass."
  else
    echo "[reality-check] FAILURE: acceptance tests PASSED with code stashed — this is a fake-green test."
  fi
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
