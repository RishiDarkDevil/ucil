#!/usr/bin/env bash
# Real OSS repo smoke test — UCIL must index a real, bounded OSS
# repository and answer a query against it. Synthetic fixtures don't
# prove robustness.
#
# Contract (implement by Phase 2):
#   1. Clone one of:
#        - https://github.com/BurntSushi/ripgrep (Rust, ~50K LOC)
#        - https://github.com/fastify/fastify (TS, ~30K LOC)
#        - https://github.com/psf/requests (Python, ~15K LOC)
#      into /tmp/ucil-realrepo-<tag>/ .
#   2. `ucil init` and wait for full index.
#   3. Run a handful of canned queries (find_definition of a known symbol,
#      find_references, search_code semantic query).
#   4. Assert response shape + that results actually reference the known
#      correct files.
#   5. Tear down the clone.
#
# Gate budget: should complete in <10 min. If slower, that's a separate
# escalation (performance regression).
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
PHASE="${1:-$(jq -r .phase ucil-build/progress.json)}"

case "$PHASE" in
  0|1) echo "[real-repo-smoke] phase $PHASE: not required"; exit 0 ;;
  *) ;;
esac

echo "[real-repo-smoke] phase=$PHASE"
echo "[real-repo-smoke] TODO: clone + index + query a real OSS repo."
echo "             Required by Phase 2 gate once UCIL's indexer is live."
exit 1
