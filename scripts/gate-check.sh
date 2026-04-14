#!/usr/bin/env bash
# Dispatcher for per-phase gate checks.
# Usage: scripts/gate-check.sh <N>
# Exit 0 iff all features in phase N are passing AND phase-N.sh exits 0 AND
# no feature was self-verified by its executor.
set -uo pipefail

cd "$(git rev-parse --show-toplevel 2>/dev/null || pwd)"

PHASE="${1:-}"
if [[ -z "$PHASE" ]]; then
  PHASE=$(jq -r '.phase // empty' ucil-build/progress.json 2>/dev/null)
fi
if [[ -z "$PHASE" ]]; then
  echo "ERROR: no phase specified and progress.json has no phase"
  exit 3
fi

echo "== Gate check: Phase $PHASE =="

if [[ ! -f ucil-build/feature-list.json ]]; then
  echo "feature-list.json missing — gate cannot pass until features are seeded."
  exit 1
fi

# 1. All phase-N features pass?
UNFIN=$(jq --argjson p "$PHASE" -r '
  [.features[] | select(.phase==($p|tonumber) and .passes==false) | .id] | join(",")
' ucil-build/feature-list.json 2>/dev/null)
if [[ -n "$UNFIN" ]]; then
  echo "[FAIL] Unfinished features in phase $PHASE: $UNFIN"
  exit 1
fi

# 2. No self-verified feature?
SELF_VER=$(jq --argjson p "$PHASE" -r '
  [.features[]
    | select(.phase==($p|tonumber))
    | select(.last_verified_by == null or (.last_verified_by | startswith("verifier-") | not))
    | .id] | join(",")
' ucil-build/feature-list.json 2>/dev/null)
if [[ -n "$SELF_VER" ]]; then
  echo "[FAIL] Features not verified by 'verifier-*' sessions: $SELF_VER"
  exit 1
fi

# 3. Phase-specific checks
PHASE_SCRIPT="scripts/gate/phase-${PHASE}.sh"
if [[ -x "$PHASE_SCRIPT" ]]; then
  echo "-- Running phase-specific checks: $PHASE_SCRIPT"
  if ! "$PHASE_SCRIPT"; then
    echo "[FAIL] phase-specific checks failed"
    exit 1
  fi
else
  echo "[WARN] no phase-specific check script at $PHASE_SCRIPT (treated as pass)"
fi

echo "[OK] Gate for phase $PHASE is GREEN"
exit 0
