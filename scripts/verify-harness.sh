#!/usr/bin/env bash
# Smoke-test the harness: every hook/script/config exists, is executable,
# and parses. Does NOT run Claude; run after bootstrap to confirm wiring.
set -uo pipefail

cd "$(git rev-parse --show-toplevel 2>/dev/null || pwd)"

PASS=0
FAIL=0
WARN=0

check() {
  local name="$1"; shift
  if "$@"; then
    echo "  [OK]   $name"
    PASS=$((PASS+1))
  else
    echo "  [FAIL] $name"
    FAIL=$((FAIL+1))
  fi
}

warn_check() {
  local name="$1"; shift
  if "$@"; then
    echo "  [OK]   $name"
    PASS=$((PASS+1))
  else
    echo "  [WARN] $name"
    WARN=$((WARN+1))
  fi
}

echo ""
echo "=== Harness verification ==="
echo ""

echo "[1/7] Root files"
check ".env exists"                   test -f .env
check "CLAUDE.md exists"              test -f CLAUDE.md
check "ucil-master-plan present"      test -f ucil-master-plan-v2.1-final.md
check ".gitignore excludes .env"      grep -q '^\.env$' .gitignore

echo ""
echo "[2/7] Git hooks"
check "core.hooksPath set"            bash -c '[[ "$(git config core.hooksPath)" == ".githooks" ]]'
check "pre-commit executable"         test -x .githooks/pre-commit
check "pre-commit-feature-list exec." test -x .githooks/pre-commit-feature-list
check "pre-commit-no-ignore exec."    test -x .githooks/pre-commit-no-ignore
check "pre-commit-secret-scan exec."  test -x .githooks/pre-commit-secret-scan
check "pre-push exec."                test -x .githooks/pre-push

echo ""
echo "[3/7] Claude settings + agents"
check "settings.json parses"          bash -c 'jq . .claude/settings.json >/dev/null 2>&1'
for a in planner executor verifier critic integration-tester root-cause-finder docs-writer; do
  check "agent: $a"                    test -f ".claude/agents/${a}.md"
done

echo ""
echo "[4/7] Claude hooks"
for h in session-start/dashboard.sh user-prompt-submit/surface-escalations.sh pre-tool-use/block-dangerous.sh pre-tool-use/path-guard.sh post-tool-use/feature-list-guard.sh post-tool-use/format.sh post-tool-use/secret-scan.sh stop/gate.sh; do
  check "hook: $h"                    test -x ".claude/hooks/${h}"
done

echo ""
echo "[5/7] Skills"
for s in phase-start phase-gate phase-ship feature-flip escalate replan; do
  check "skill: /$s"                   test -f ".claude/skills/${s}/SKILL.md"
done

echo ""
echo "[6/7] Harness brain"
check "progress.json parses"          bash -c 'jq . ucil-build/progress.json >/dev/null 2>&1'
check "feature-list schema parses"    bash -c 'jq . ucil-build/schema/feature-list.schema.json >/dev/null 2>&1'
check "ucil-build/CLAUDE.md present"  test -f ucil-build/CLAUDE.md

echo ""
echo "[7/7] Scripts + external tools"
for sc in install-prereqs.sh bootstrap.sh verify-harness.sh gate-check.sh seed-features.sh spawn-verifier.sh flip-feature.sh run-phase.sh run-all.sh reality-check.sh; do
  check "script: $sc"                  test -x "scripts/${sc}" || test -f "scripts/${sc}"
done
for tool in git jq rustc cargo node npm python3 docker gh; do
  warn_check "tool: $tool"             command -v "$tool" >/dev/null 2>&1
done
for tool in gitleaks trufflehog semgrep trivy ast-grep ollama; do
  warn_check "tool (optional): $tool"  command -v "$tool" >/dev/null 2>&1
done

echo ""
echo "=== Summary: PASS=$PASS FAIL=$FAIL WARN=$WARN ==="
if [[ "$FAIL" -gt 0 ]]; then
  echo "Harness verification FAILED — fix the [FAIL] items before running seed-features.sh."
  exit 1
fi
echo "Harness verification passed. WARNs are optional tools (install them to unlock specialist agents)."
exit 0
