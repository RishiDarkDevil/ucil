---
type: technical-debt
severity: high
created_at: 2026-04-15T16:30:00Z
created_by: verifier-266e9762-c7ef-48d6-a620-38dac16056fa
work_order: WO-0002
requires_planner_action: true
resolved: false
---

# scripts/reality-check.sh: per-file rollback needed for multi-commit features

## Context

The c18977b fix correctly union-izes source files across all commits tagged
for a feature. However, the rollback logic still uses a single `LAST_COMMIT`
(the newest source-changing commit) as the baseline for all files.

## Bug

For P0-W1-F02, the feature spans two source-changing commits:

| SHA | File | 
|-----|------|
| ea983dd | crates/ucil-core/src/types.rs (added) |
| e5ecae2 | crates/ucil-core/src/lib.rs (modified) |

`LAST_COMMIT = e5ecae2` (newest). The rollback does:

```bash
git show "${LAST_COMMIT}^:$f" > "$f"
```

For types.rs: `e5ecae2^` already contains the full types.rs (added in the
earlier commit ea983dd). So types.rs is "rolled back" to its own current
content -- no mutation occurs.

For lib.rs: `e5ecae2^` correctly lacks the `pub mod types;` declarations.
But with the types module undeclared, `cargo test -p ucil-core types::` finds
0 matching tests, exits 0, and the script treats this as "tests passed with
code stashed" -- a false positive.

## Fix

For each file in the union, find the specific commit that introduced/changed
it for this feature, and roll back to THAT commit's parent:

```bash
for f in $CHANGED_FILES; do
  # Find the commit in CANDIDATES that changed this specific file
  file_commit=""
  for sha in $CANDIDATES; do
    if git show --name-only --pretty=format: "$sha" | grep -qF "$f"; then
      file_commit="$sha"
      break
    fi
  done
  if [[ -n "$file_commit" ]]; then
    if git cat-file -e "${file_commit}^:$f" 2>/dev/null; then
      git show "${file_commit}^:$f" > "$f"
    else
      rm -f "$f"
    fi
  fi
done
```

Additionally, the script should detect when `cargo test` exits 0 with 0 tests
run, and treat that as a failure (no matching tests = module was removed, not
a genuine pass).

## Impact

Any multi-commit feature where an earlier commit adds a new file and a later
commit wires it into lib.rs will hit this bug. As the project grows, more
features will span multiple commits and trigger false positives.

## Workaround used

Verifier manually verified the P0-W1-F02 mutation property by removing
types.rs entirely and confirming tests fail (exit 101). The feature was
flipped to pass based on this manual verification.
