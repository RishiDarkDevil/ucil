#!/usr/bin/env bash
# SessionStart hook: prints a dashboard of build state.
set -euo pipefail

REPO_ROOT="${CLAUDE_PROJECT_DIR:-$PWD}"
cd "$REPO_ROOT" || exit 0

echo ""
echo "=============================================="
echo "  UCIL AUTONOMOUS BUILD — SESSION DASHBOARD"
echo "=============================================="

if [[ -f ucil-build/progress.json ]] && command -v jq >/dev/null 2>&1; then
  PHASE=$(jq -r '.phase // "unseeded"' ucil-build/progress.json 2>/dev/null || echo "unseeded")
  WEEK=$(jq -r '.week // "—"' ucil-build/progress.json 2>/dev/null || echo "—")
  echo "  Phase: $PHASE    Week: $WEEK"
else
  echo "  State: unseeded (no progress.json yet)"
fi

if [[ -f ucil-build/feature-list.json ]] && command -v jq >/dev/null 2>&1; then
  TOTAL=$(jq -r '.features | length' ucil-build/feature-list.json 2>/dev/null || echo "?")
  PASSING=$(jq -r '[.features[] | select(.passes==true)] | length' ucil-build/feature-list.json 2>/dev/null || echo "?")
  echo "  Features passing: $PASSING / $TOTAL"
else
  echo "  Features: unseeded"
fi

# Open escalations
ESC_COUNT=$(ls -1 ucil-build/escalations/ 2>/dev/null | wc -l || echo 0)
if [[ "$ESC_COUNT" -gt 0 ]]; then
  echo ""
  echo "  ⚠️  $ESC_COUNT open escalation(s):"
  ls -1 ucil-build/escalations/ 2>/dev/null | head -5 | sed 's/^/      /'
fi

# Recent commits on current branch
if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  BRANCH=$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo "?")
  echo ""
  echo "  Branch: $BRANCH"
  DIRTY=$(git status --porcelain 2>/dev/null | wc -l)
  AHEAD=$(git rev-list "@{u}..HEAD" 2>/dev/null | wc -l || echo 0)
  if [[ "$DIRTY" -gt 0 ]]; then echo "  ⚠️  $DIRTY uncommitted change(s) in working tree"; fi
  if [[ "$AHEAD" -gt 0 ]]; then echo "  ⚠️  $AHEAD commit(s) ahead of upstream (not pushed)"; fi
  echo ""
  echo "  Last 3 commits:"
  git log --oneline -3 2>/dev/null | sed 's/^/    /'
fi

# Load phase-scoped instructions if present
if [[ -n "${PHASE:-}" && "$PHASE" != "unseeded" ]]; then
  PHASE_CLAUDE="ucil-build/phase-log/$(printf '%02d' "$PHASE")-phase-$PHASE/CLAUDE.md"
  if [[ -f "$PHASE_CLAUDE" ]]; then
    echo ""
    echo "  Phase instructions: $PHASE_CLAUDE"
  fi
fi

# Last 2 decisions
if [[ -d ucil-build/decisions ]]; then
  DEC_FILES=$(ls -1t ucil-build/decisions/ 2>/dev/null | head -2 || true)
  if [[ -n "$DEC_FILES" ]]; then
    echo ""
    echo "  Recent decisions:"
    echo "$DEC_FILES" | sed 's/^/    /'
  fi
fi

echo "=============================================="
echo ""
exit 0
