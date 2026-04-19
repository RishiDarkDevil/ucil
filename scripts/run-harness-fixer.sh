#!/usr/bin/env bash
# Launch the harness-fixer subagent.
#
# Usage: scripts/run-harness-fixer.sh <phase> <failing-scripts-log>
#
# The failing-scripts-log is expected to be the tail of
# /tmp/ucil-gate-check.log (or an integration-tester report) — the
# launcher greps it for `[FAIL]` lines and failing verify scripts and
# attaches them to the agent prompt along with the per-script stderr.
#
# Invoked by:
#   - scripts/gate-check.sh (after phase-N.sh exits non-zero)
#   - scripts/run-integration-tester.sh (after any scripts/verify/*.sh
#     exits non-zero)
#
# Exit codes:
#   0  — harness-fixer applied one or more fixes; caller should re-run
#        the gate and check results.
#   1  — harness-fixer halted (wrote a bucket-E escalation). Caller
#        should not auto-retry; human input needed.
#   2  — harness-fixer itself failed to spawn (auth, fork, etc).
set -uo pipefail

cd "$(git rev-parse --show-toplevel)"

# shellcheck source=scripts/_retry.sh
source "$(dirname "$0")/_retry.sh" 2>/dev/null || true

PHASE="${1:-}"
GATE_LOG="${2:-/tmp/ucil-gate-check.log}"

if [[ -z "$PHASE" ]]; then
  PHASE=$(jq -r '.phase // empty' ucil-build/progress.json 2>/dev/null || echo 1)
fi

LOG="/tmp/ucil-harness-fixer-phase-${PHASE}.log"
echo "[run-harness-fixer] phase=$PHASE gate-log=$GATE_LOG"
echo "[run-harness-fixer] log: $LOG"

# Refresh auth from credentials file if env var stale/missing.
if [[ -z "${ANTHROPIC_API_KEY:-}" ]]; then
  if [[ -f ~/.claude/.credentials.json ]]; then
    TOKEN=$(jq -r '.claudeAiOauth.accessToken // empty' ~/.claude/.credentials.json 2>/dev/null)
    if [[ -n "$TOKEN" ]]; then
      export CLAUDE_CODE_OAUTH_TOKEN="$TOKEN"
      echo "[_load-auth] loaded CLAUDE_CODE_OAUTH_TOKEN from ~/.claude/.credentials.json"
    fi
  fi
fi

# Extract failing scripts from the gate log.
FAIL_LINES=$(grep -E '\[FAIL\]' "$GATE_LOG" 2>/dev/null | tail -20 || true)
FAIL_TAIL=$(tail -120 "$GATE_LOG" 2>/dev/null || true)

if [[ -z "$FAIL_LINES" ]]; then
  echo "[run-harness-fixer] no [FAIL] lines in $GATE_LOG — nothing to do" >&2
  exit 0
fi

AGENT_PATH=".claude/agents/harness-fixer.md"
if [[ ! -f "$AGENT_PATH" ]]; then
  echo "[run-harness-fixer] FAIL: $AGENT_PATH missing" >&2
  exit 2
fi

PROMPT=$(cat <<EOF
You are the UCIL harness-fixer. Current phase: $PHASE.

The gate check just failed with these sub-check failures:

\`\`\`
$FAIL_LINES
\`\`\`

Last 120 lines of /tmp/ucil-gate-check.log:

\`\`\`
$FAIL_TAIL
\`\`\`

Your job: diagnose and fix each failing script per your agent contract
(.claude/agents/harness-fixer.md). You have write access to
scripts/verify/*.sh, scripts/gate/phase-*.sh (bug fixes only, not
structural changes), scripts/_retry.sh, scripts/_watchdog.sh, and
.githooks/*. You do NOT have write access to UCIL source (crates/,
adapters/, ml/, plugin*/, tests/*).

Diff budget: 120 LOC total per run. Iteration cap: 3 attempts per
script. Commit each fix separately; push immediately. If any script
can't be fixed within those limits, write a bucket-E escalation
(type: harness-fixer-halt) with the investigation log and halt.

Print the summary table before exiting.
EOF
)

# Invoke claude -p with the agent's system prompt appended.
set +e
retry_fn() {
  claude -p "$PROMPT" \
    --model claude-opus-4-7 \
    --dangerously-skip-permissions \
    --append-system-prompt "$(cat "$AGENT_PATH")" \
    2>&1 | tee -a "$LOG"
}

if command -v retry_with_backoff >/dev/null 2>&1; then
  retry_with_backoff 2 30 -- bash -c "$(declare -f retry_fn); retry_fn"
else
  retry_fn
fi
RC=$?
set -e

# Did the fixer write a halt escalation? If so, return 1 so caller halts.
if ls ucil-build/escalations/*-harness-fixer-halt-*.md 2>/dev/null | xargs -r grep -l "^resolved:\s*true" 2>/dev/null | wc -l | grep -qv "$(ls ucil-build/escalations/*-harness-fixer-halt-*.md 2>/dev/null | wc -l)"; then
  echo "[run-harness-fixer] unresolved harness-fixer-halt escalation present — caller should not auto-retry"
  exit 1
fi

if [[ $RC -ne 0 ]]; then
  echo "[run-harness-fixer] claude -p exited with $RC"
  exit 2
fi

echo "[run-harness-fixer] done"
exit 0
