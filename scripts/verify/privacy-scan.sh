#!/usr/bin/env bash
# Privacy / data-locality audit.
#
# Master plan §1.2 principle 8: "Fully local. Zero cloud dependencies."
# This verification proves it.
#
# Contract (implement by Phase 5):
#   1. Start ucild on a test fixture.
#   2. Monitor outbound network connections for 10 minutes of mixed
#      query load (use `ss -tnp` sampled, or an eBPF probe).
#   3. Assert:
#        - NO connections to external hosts except when an explicit
#          LLM-provider is configured (Ollama localhost is OK; Claude
#          API / OpenAI only if user opted in).
#        - NO DNS queries for telemetry / analytics / metrics providers
#          we don't expect.
#        - .ucil/ contents don't contain obvious PII leakage from user's
#          system (running `trufflehog filesystem .ucil/` returns 0 hits).
#   4. Run a secret-scan on UCIL's own source + config templates for
#      accidentally-committed keys:
#        - `gitleaks detect --no-git`
#        - `trufflehog filesystem . --exclude-dir=.git --exclude-dir=target --exclude-dir=node_modules`
#   5. Fail on any finding.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
PHASE="${1:-$(jq -r .phase ucil-build/progress.json)}"

case "$PHASE" in
  5|6|7|8) ;;
  *) echo "[privacy] phase $PHASE: not required"; exit 0 ;;
esac

echo "[privacy] phase=$PHASE"

# Partial implementation now: run the secret-scan portion on committed source.
# Fail if obvious secrets exist. Network egress checks need Phase 5's daemon
# running and will be fleshed out then.
if command -v gitleaks >/dev/null 2>&1; then
  echo "[privacy] running gitleaks detect..."
  gitleaks detect --no-git --redact --no-banner --exit-code 1 -v || {
    echo "[privacy] gitleaks found secrets in tracked files."
    exit 1
  }
fi

echo "[privacy] TODO: egress monitoring + .ucil/ PII scan; required by Phase 5 gate."
exit 1
