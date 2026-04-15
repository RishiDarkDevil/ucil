#!/usr/bin/env bash
# Generic effectiveness gate. Called by scripts/gate/phase-N.sh.
#
# Usage: scripts/verify/effectiveness-gate.sh <phase>
#
# Exits 0 iff:
#   - At least one scenario tagged for this phase exists
#   - Every non-skipped scenario returns a PASS or WIN verdict
#
# Skipped scenarios (tool-not-ready) do NOT fail the gate — they carry over.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"
PHASE="${1:-}"

# Discover scenarios tagged for this phase
MATCHED=$(grep -l "^- *${PHASE}\\b\\|^phases: *\\[.*\\b${PHASE}\\b" tests/scenarios/*.yaml 2>/dev/null | wc -l)
if [[ "$MATCHED" -eq 0 ]]; then
  # No scenarios for this phase — by convention, acceptable for Phase 0
  # and Phase 8. Fail the gate for Phase 1–7 because UCIL must be
  # effectiveness-tested once it has user-facing functionality.
  case "$PHASE" in
    0|8) echo "[effectiveness-gate] no scenarios tagged for phase $PHASE (acceptable)"; exit 0 ;;
    *)   echo "[effectiveness-gate] no scenarios tagged for phase $PHASE — REQUIRED. Add at least one to tests/scenarios/*.yaml."; exit 1 ;;
  esac
fi

scripts/run-effectiveness-evaluator.sh "$PHASE"
rc=$?
exit "$rc"
