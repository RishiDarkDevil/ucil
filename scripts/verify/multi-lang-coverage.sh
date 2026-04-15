#!/usr/bin/env bash
# Multi-language coverage check — UCIL must work across Rust, Python,
# TypeScript, Go (and by Phase 5, also Java or C/C++).
#
# Contract (fill in during phase work, must exit 0 by phase gate):
# For each language in the required set for this phase, run a probe
# query through UCIL's MCP that exercises:
#   - symbol-find (find_definition)
#   - cross-file references (find_references)
#   - structural search (search_code)
# against the matching tests/fixtures/<lang>-project/ fixture.
#
# Fail if any language's probes fail OR if a required language's fixture
# is missing.
#
# Usage: scripts/verify/multi-lang-coverage.sh <phase>
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
PHASE="${1:-$(jq -r .phase ucil-build/progress.json)}"

# Required languages per phase
case "$PHASE" in
  1|2) REQUIRED="rust python typescript" ;;
  3|3.5|4) REQUIRED="rust python typescript go" ;;
  5|6|7|8) REQUIRED="rust python typescript go" ;;
  *) REQUIRED="" ;;
esac

if [[ -z "$REQUIRED" ]]; then
  echo "[multi-lang] no languages required for phase $PHASE"
  exit 0
fi

echo "[multi-lang] phase=$PHASE required=$REQUIRED"
echo "[multi-lang] TODO: implement MCP probes per language. Expected to be"
echo "             fleshed out by the executor during Phase 1+ as the MCP"
echo "             server and its tools come online. See scope note above."
# Placeholder — fail until actual implementation lands.
# Remove this exit 1 and implement real probes when Phase 1's MCP server
# exists. An executor WO covering `multi_lang_probes_integration_test`
# should replace this stub.
exit 1
