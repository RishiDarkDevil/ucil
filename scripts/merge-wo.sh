#!/usr/bin/env bash
# Merge a work-order's feat branch into main.
#
# Workflow:
#   1. Fetch feat branch from origin (create/update local ref).
#   2. Checkout main, pull.
#   3. If feat has diverged (main moved forward since branch point), attempt
#      to rebase feat onto main in-place. On conflict, bail without pushing.
#   4. git merge --no-ff feat into main.
#   5. Push main.
#
# On any unrecoverable failure, writes an escalation and exits 1 so the
# outer loop halts for human review.
#
# Usage: scripts/merge-wo.sh <WO-ID>
set -uo pipefail

cd "$(git rev-parse --show-toplevel)"

WO_ARG="${1:-}"
if [[ -z "$WO_ARG" ]]; then
  echo "Usage: $0 <work-order-id>" >&2
  exit 2
fi
WO_ID="${WO_ARG#WO-}"
WO_ID="WO-${WO_ID}"

WO_FILE=$(ls ucil-build/work-orders/${WO_ID#WO-}-*.json 2>/dev/null | head -1)
if [[ -z "$WO_FILE" ]]; then
  echo "ERROR: no work-order JSON for $WO_ID" >&2
  exit 3
fi
SLUG=$(jq -r .slug "$WO_FILE")
BRANCH="feat/${WO_ID}-${SLUG}"

escalate() {
  local reason="$1"
  local path="ucil-build/escalations/$(date -u +%Y%m%d-%H%M)-merge-failure-${WO_ID}.md"
  cat > "$path" <<EOF
---
timestamp: $(date -u +%Y-%m-%dT%H:%M:%SZ)
type: merge-failure
work_order: ${WO_ID}
branch: ${BRANCH}
severity: high
blocks_loop: true
---

# Merge failure for ${WO_ID}

${reason}

## Repro

\`\`\`
git checkout main && git pull && git merge --no-ff ${BRANCH}
\`\`\`

## Recommended action

Human resolves the conflict manually, commits + pushes main, then resolves
this escalation.
EOF
  git add "$path" 2>/dev/null || true
  git commit -m "chore(escalation): merge-failure for ${WO_ID}" 2>/dev/null || true
  git push 2>/dev/null || true
}

echo "[merge-wo] ${WO_ID}: branch=${BRANCH}"

# 1. Fetch
git fetch origin "$BRANCH" 2>&1 | tail -2 || {
  escalate "git fetch origin ${BRANCH} failed. Branch may not be on origin."
  exit 1
}

# Make sure we have a local ref — prefer local if it exists, else build from origin
if ! git rev-parse --verify "$BRANCH" >/dev/null 2>&1; then
  git branch --track "$BRANCH" "origin/$BRANCH" 2>/dev/null || true
fi

# 2. Main baseline
git checkout main 2>&1 | tail -1
git pull 2>&1 | tail -2

# Fast-path: already contains the feat HEAD?
MAIN_SHA=$(git rev-parse main)
FEAT_SHA=$(git rev-parse "$BRANCH")
if git merge-base --is-ancestor "$FEAT_SHA" "$MAIN_SHA"; then
  echo "[merge-wo] main already contains $BRANCH ($FEAT_SHA) — nothing to merge."
  exit 0
fi

# 3. Divergence check — if feat's merge-base is NOT main's tip, main moved
# forward independently. Merge main INTO feat first (no force-push needed)
# so the subsequent merge into main is clean.
BASE=$(git merge-base main "$BRANCH")
if [[ "$BASE" != "$MAIN_SHA" ]]; then
  echo "[merge-wo] branches diverged (feat-base=$BASE, main-tip=$MAIN_SHA). Merging main into $BRANCH..."
  git checkout "$BRANCH" 2>&1 | tail -1
  if ! git merge --no-ff main -m "chore(integrate): pull main into $BRANCH before merge"; then
    echo "[merge-wo] merge conflict integrating main into $BRANCH. Aborting + escalating."
    git merge --abort 2>/dev/null || true
    git checkout main 2>/dev/null || true
    escalate "Merging main into $BRANCH hit conflict(s). Manual resolution needed."
    exit 1
  fi
  # Push the integrated feat branch (no force, just a fast-forward-able push).
  git push origin "$BRANCH" 2>&1 | tail -2 || {
    escalate "git push origin $BRANCH failed after integrating main. Upstream moved concurrently?"
    exit 1
  }
  git checkout main 2>&1 | tail -1
fi

# 4. Merge --no-ff
MERGE_MSG=$(cat <<EOF
merge: ${WO_ID} ${SLUG} (feat → main)

Brings WO-${WO_ID#WO-} from feat/${WO_ID}-${SLUG} into main.
All acceptance criteria pass (verifier), critic CLEAN or ADR-accepted.

Phase: $(jq -r .phase "$WO_FILE")
Features: $(jq -r '.feature_ids // .features // [] | join(", ")' "$WO_FILE")
Work-order: ${WO_ID}

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)

if ! git merge --no-ff "$BRANCH" -m "$MERGE_MSG"; then
  echo "[merge-wo] merge conflict on main ← $BRANCH. Aborting + escalating."
  git merge --abort 2>/dev/null || true
  escalate "git merge --no-ff $BRANCH into main hit conflict(s) even after rebase. Unexpected; manual resolution."
  exit 1
fi

# 5. Push
if ! git push origin main 2>&1 | tail -2; then
  escalate "git push origin main failed after successful local merge. Upstream moved concurrently?"
  exit 1
fi

echo "[merge-wo] ${WO_ID} merged into main successfully."
exit 0
