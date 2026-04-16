#!/usr/bin/env bash
# Launch the critic subagent on a work-order's feat branch.
# Read-only adversarial review. Writes ucil-build/critic-reports/<WO-ID>.md.
#
# Usage: scripts/run-critic.sh <work-order-id>
#   e.g. scripts/run-critic.sh WO-0001
set -euo pipefail

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

SLUG=$(jq -r .slug "$WO_FILE" 2>/dev/null || echo "")
BRANCH="feat/${WO_ID}-${SLUG}"

# Try to fetch the remote branch so critic can inspect it even if user hasn't
git fetch origin "$BRANCH" 2>/dev/null || true

if ! git rev-parse --verify "$BRANCH" >/dev/null 2>&1 && ! git rev-parse --verify "origin/$BRANCH" >/dev/null 2>&1; then
  echo "ERROR: branch $BRANCH not found locally or on origin" >&2
  exit 3
fi

# shellcheck source=scripts/_load-auth.sh
source "$(dirname "$0")/_load-auth.sh"

LOG="/tmp/ucil-critic-${WO_ID}.log"
REF="$(git rev-parse --verify "$BRANCH" 2>/dev/null || git rev-parse "origin/$BRANCH")"

PROMPT=$(cat <<EOF
You are the UCIL critic. Adversarial, read-only review of a work-order's diff.

Work-order: ${WO_FILE}
Branch: ${BRANCH}
Ref: ${REF}
Merge base: \$(git merge-base main ${BRANCH} 2>/dev/null || echo main)

Run every check listed in .claude/agents/critic.md against the diff main..${BRANCH}:
1. Stub detection (ast-grep for todo!(), unimplemented!(), raise NotImplementedError, bare pass, trivial default returns).
2. Mocked critical dependencies (Serena, LSP servers, SQLite, LanceDB, Docker) in tests/.
3. Skipped/ignored tests (#[ignore], .skip, xfail, xit, pytest.skip).
4. Weak assertions (assert!(true), expect(true), etc.).
5. Hallucinated imports / paths not present on the branch.
6. Feature coverage: every feature_id in the WO has at least one new/modified test referencing its behaviour.
7. Commit hygiene: Conventional Commits, Phase/Feature/Work-order trailers, size sanity.
8. Doc + public API: new pub items have docs.

Write ucil-build/critic-reports/${WO_ID}.md in the format given in critic.md:
- Findings organised as Blockers / Warnings / OK
- Cite file:line for every finding
- Final verdict: CLEAN or BLOCKED

Commit and push the report. You must end cleanly. Never edit source code.
EOF
)

echo "[run-critic] work-order: ${WO_FILE}"
echo "[run-critic] branch:     ${BRANCH}"
echo "[run-critic] log:        ${LOG}"
echo "[run-critic] starting in 3s..."
sleep 3

UCIL_WO_ID="${WO_ID}" CLAUDE_CODE_ENABLE_TELEMETRY=1 \
CLAUDE_SUBAGENT_NAME=critic \
exec claude -p "$PROMPT" \
  --dangerously-skip-permissions \
  --append-system-prompt "$(cat .claude/agents/critic.md)" \
  2>&1 | tee "$LOG"
