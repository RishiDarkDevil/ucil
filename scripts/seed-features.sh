#!/usr/bin/env bash
# One-shot planner run to seed ucil-build/feature-list.json from the master plan.
# After this runs, the user MUST review the file and commit with message
# "freeze: feature oracle v1.0.0". Post-commit, only whitelisted fields are mutable.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

if [[ -f ucil-build/feature-list.json ]] && [[ "$(jq -r '.features | length' ucil-build/feature-list.json 2>/dev/null)" -gt 0 ]]; then
  echo "feature-list.json already seeded. Refusing to overwrite."
  echo "If you really need to re-seed, write an ADR, then:"
  echo "  rm ucil-build/feature-list.json"
  echo "  $0"
  exit 1
fi

if ! command -v claude >/dev/null 2>&1; then
  echo "ERROR: 'claude' CLI not found in PATH." >&2
  exit 3
fi

if [[ -f .env ]]; then
  set -a
  source .env
  set +a
fi

if [[ -z "${CLAUDE_CODE_OAUTH_TOKEN:-}" && -z "${ANTHROPIC_API_KEY:-}" ]]; then
  echo "ERROR: neither CLAUDE_CODE_OAUTH_TOKEN nor ANTHROPIC_API_KEY is set in .env." >&2
  echo "Get an OAuth token with: claude setup-token" >&2
  exit 3
fi

PROMPT=$(cat <<'EOF'
You are the UCIL Planner, in SEEDING mode. Your one-shot task is to read
ucil-master-plan-v2.1-final.md and produce ucil-build/feature-list.json
conforming to ucil-build/schema/feature-list.schema.json.

Instructions:

1. Read ucil-master-plan-v2.1-final.md in full (it is 121KB; read in chunks
   if needed).
2. For every week-level bullet in §18 "Phase-wise implementation plan"
   (Phases 0, 1, 2, 3, 3.5, 4, 5, 6, 7, 8), derive one or more feature-list
   entries. Use the naming convention: P<phase>-W<week>-F<nn>.
   - Phase 3.5 uses "3.5" literally as the phase number.
3. For each entry, write:
   - id
   - phase (integer or 3.5)
   - week (integer)
   - crate: the crate/adapter/module this touches (use master plan §17 for names)
   - description: precise, testable, one sentence
   - acceptance_tests: 1-3 of these kinds, all executable:
       {kind: "cargo_test", selector: "..."}       # e.g., "-p ucil-treesitter tag_cache::"
       {kind: "pytest",     selector: "..."}       # e.g., "tests/test_embed.py::test_latency"
       {kind: "vitest",     selector: "..."}       # e.g., "adapters/test/claude-code.test.ts"
       {kind: "script",     path: "scripts/verify/P1-W2-F03.sh", exit: 0}
       {kind: "bench",      script: "scripts/bench-tagcache.sh", assert: "p95_warm_us<2000"}
   - dependencies: list of feature IDs that must be passing=true first
   - passes: false
   - last_verified_ts: null
   - last_verified_by: null
   - last_verified_commit: null
   - attempts: 0
   - blocked_reason: null
4. Also derive entries from §4.1-§4.8 for every P0/P1 tool integration
   (one feature per "install + wire into group + pass smoke test").
5. Also derive entries from §19.1 for every test category the master plan
   mandates (one feature for "test suite exists and passes for category X").
6. Output the final JSON to ucil-build/feature-list.json. Schema envelope:
   {
     "version": "1.0.0",
     "frozen_at": "<current ISO-8601 timestamp>",
     "frozen_commit": null,
     "source_plan_sha256": "<sha256 of ucil-master-plan-v2.1-final.md>",
     "features": [...]
   }
7. Also write stub scripts under scripts/verify/<feature-id>.sh for every
   feature whose acceptance_tests include a kind:"script". Each stub is:
       #!/usr/bin/env bash
       echo "TODO: implement acceptance test for <feature-id>"
       exit 1
   Executor agents will implement these properly during the feature's
   work-order.
8. Validate the final JSON against the schema:
       jq . ucil-build/feature-list.json  # must succeed
9. Print a summary: total features, breakdown by phase.
10. DO NOT commit. The user must review and commit with message
    "freeze: feature oracle v1.0.0" manually.
11. Target roughly 200-300 features total. If yours is <150 or >400,
    revisit granularity.
EOF
)

echo "[seed-features] Starting one-shot planner over master plan..."
echo "[seed-features] This may take 5-15 minutes and will consume ~2M tokens."
echo ""

# UCIL_SEEDING=1 bypasses the feature-list write guards for this one-shot run.
UCIL_SEEDING=1 \
CLAUDE_SUBAGENT_NAME=planner \
claude -p "$PROMPT" \
  --append-system-prompt "$(cat .claude/agents/planner.md)"

echo ""
echo "[seed-features] Done. Now:"
echo "  1. Review ucil-build/feature-list.json for obvious gaps or over-decomposition."
echo "  2. Apply manual corrections if needed."
echo "  3. Commit: UCIL_SEEDING=1 git add ucil-build/feature-list.json && UCIL_SEEDING=1 git commit -m 'freeze: feature oracle v1.0.0'"
echo "  4. Push: git push -u origin main"
echo "  5. Update progress.json: jq '.seeded = true' ucil-build/progress.json | sponge ucil-build/progress.json"
echo "  6. Run: /phase-start 0"
