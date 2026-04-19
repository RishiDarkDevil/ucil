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

# 3. Integration pass (phases that integrate with real external processes).
#    Runs the integration-tester subagent once per gate-check call to bring
#    up docker fixtures (when needed) and execute the e2e verify scripts
#    against real collaborators. Skipped for phases that don't integrate
#    with external services (0, 4, 6, 8).
case "$PHASE" in
  1|2|3|5|7)
    if [[ "${UCIL_SKIP_INTEGRATION_TESTER:-0}" == "1" ]]; then
      echo "-- Skipping integration pass (UCIL_SKIP_INTEGRATION_TESTER=1)"
    elif [[ -x scripts/run-integration-tester.sh ]]; then
      echo "-- Running integration pass: scripts/run-integration-tester.sh $PHASE"
      if ! scripts/run-integration-tester.sh "$PHASE"; then
        echo "[FAIL] integration pass failed (see ucil-build/verification-reports/phase-${PHASE}-integration.md)"
        exit 1
      fi
    else
      echo "[WARN] scripts/run-integration-tester.sh missing; skipping integration pass"
    fi
    ;;
  *)
    echo "-- Phase $PHASE does not require integration pass (skipping)"
    ;;
esac

# 4. Phase-specific checks — with auto-invoke of harness-fixer on failure.
#
# When phase-N.sh fails because a `scripts/verify/*.sh` sub-check
# errors out, we spawn the harness-fixer subagent to diagnose and patch
# the failing scripts. After the fixer returns we re-run phase-N.sh
# exactly once. If sub-checks still fail (or the fixer halted with an
# escalation), we return failure.
#
# This closes the "harness-script bugs" gap where the normal planner →
# executor → verifier pipeline has no path to fix harness-side code
# because its scope is feature_ids, not shell scripts.
#
# Set UCIL_SKIP_HARNESS_FIXER=1 to bypass (rare; mostly for debugging
# the fixer itself).
PHASE_SCRIPT="scripts/gate/phase-${PHASE}.sh"
if [[ -x "$PHASE_SCRIPT" ]]; then
  echo "-- Running phase-specific checks: $PHASE_SCRIPT"
  GATE_LOG="/tmp/ucil-gate-check.log"
  : > "$GATE_LOG"
  if ! "$PHASE_SCRIPT" 2>&1 | tee -a "$GATE_LOG"; then
    echo "[FAIL] phase-specific checks failed"

    if [[ "${UCIL_SKIP_HARNESS_FIXER:-0}" == "1" ]]; then
      echo "[gate-check] UCIL_SKIP_HARNESS_FIXER=1 — not invoking harness-fixer"
      exit 1
    fi

    # Only spawn fixer if at least one verify-script sub-check failed
    # (grep the log for `[FAIL]` lines that reference a script path).
    if grep -qE '\[FAIL\]' "$GATE_LOG"; then
      if [[ -x scripts/run-harness-fixer.sh ]]; then
        echo "-- Invoking harness-fixer to diagnose + patch failing scripts"
        if scripts/run-harness-fixer.sh "$PHASE" "$GATE_LOG"; then
          echo "-- Re-running phase-specific checks after harness-fixer"
          : > "$GATE_LOG"
          if "$PHASE_SCRIPT" 2>&1 | tee -a "$GATE_LOG"; then
            echo "[OK] Gate for phase $PHASE is GREEN (post harness-fixer)"
            exit 0
          fi
          echo "[FAIL] phase-specific checks still failing after harness-fixer"
        else
          echo "[FAIL] harness-fixer halted or errored (see ucil-build/escalations/)"
        fi
      else
        echo "[WARN] scripts/run-harness-fixer.sh missing — can't self-repair"
      fi
    fi

    exit 1
  fi
else
  echo "[WARN] no phase-specific check script at $PHASE_SCRIPT (treated as pass)"
fi

echo "[OK] Gate for phase $PHASE is GREEN"
exit 0
