#!/usr/bin/env bash
# Full user-journey smoke — simulates a new user picking up UCIL from
# scratch and using it on a real task. This is THE end-to-end acceptance
# test for "UCIL is ready to drop into my projects."
#
# Contract (implement by Phase 8):
#   1. Fresh Ubuntu/Debian docker container OR a dedicated $HOME sandbox.
#   2. Run the published install script (`scripts/install.sh` from UCIL
#      v0.1.0 release tag).
#   3. `cd /some/real-project`; run `ucil init`.
#      Assert: .ucil/ created with init_report.json, health=all P0 OK.
#   4. Install the Claude Code plugin via the documented command.
#   5. Start a headless `claude -p <real task>` session pointed at the
#      project. Task must:
#        - require semantic code search (UCIL's search_code)
#        - require symbol navigation (find_definition / find_references)
#        - require convention awareness (get_conventions)
#   6. Assert the session produces a correct result within a reasonable
#      time + token budget.
#   7. Run `ucil status` and assert all subsystems report HEALTHY.
#   8. `ucil export-brain` and `ucil import-brain` round-trip.
#   9. Tear down sandbox.
#
# This test IS Phase 8's sign-off. If this works, UCIL is ready to ship
# to users.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
PHASE="${1:-$(jq -r .phase ucil-build/progress.json)}"

case "$PHASE" in
  8) ;;
  *) echo "[user-journey] phase $PHASE: not required"; exit 0 ;;
esac

echo "[user-journey] phase=$PHASE"
echo "[user-journey] TODO: full end-to-end new-user flow in a clean sandbox."
echo "           This is THE gate for v0.1.0 release. Required by Phase 8."
exit 1
