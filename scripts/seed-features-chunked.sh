#!/usr/bin/env bash
# Chunked fallback for seed-features.sh: generate the oracle one phase at a
# time, then merge. More reliable than one monolithic 234-feature generation.
#
# Produces ucil-build/feature-list.parts/phase-N.json for each phase, then
# merges into ucil-build/feature-list.json at the end.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

if [[ -f ucil-build/feature-list.json ]] && [[ "$(jq -r '.features | length' ucil-build/feature-list.json 2>/dev/null)" -gt 0 ]]; then
  echo "feature-list.json already seeded. Refusing to overwrite. Remove it first to reseed."
  exit 1
fi

if ! command -v claude >/dev/null 2>&1; then
  echo "ERROR: claude CLI not in PATH." >&2
  exit 3
fi

if [[ -f .env ]]; then
  set -a
  source .env
  set +a
fi

if [[ -z "${CLAUDE_CODE_OAUTH_TOKEN:-}" && -z "${ANTHROPIC_API_KEY:-}" ]]; then
  echo "ERROR: no auth in .env." >&2
  exit 3
fi

mkdir -p ucil-build/feature-list.parts
PLAN_SHA256=$(sha256sum ucil-master-plan-v2.1-final.md | awk '{print $1}')
FROZEN_AT=$(date -u +%Y-%m-%dT%H:%M:%SZ)

PHASES=(0 1 2 3 3.5 4 5 6 7 8)
# Plan section offsets for each phase from master plan §18 (rough)
declare -A SECTION
SECTION[0]="Phase 0 — Project bootstrap (Week 1)"
SECTION[1]="Phase 1 — Daemon core + tree-sitter + Serena + diagnostics bridge (Weeks 2–5)"
SECTION[2]="Phase 2 — Plugins + G1/G2 + embeddings (Weeks 6–8)"
SECTION[3]="Phase 3 — Orchestration + all groups + warm processors (Weeks 9–11)"
SECTION[3.5]="Phase 3.5 — Agent layer (Weeks 12–13)"
SECTION[4]="Phase 4 — Host adapters + Claude Code plugin (Weeks 14–15)"
SECTION[5]="Phase 5 — Knowledge evolution + compaction + security (Weeks 16–18)"
SECTION[6]="Phase 6 — Performance + observability (Weeks 19–20)"
SECTION[7]="Phase 7 — Database + infrastructure integration (Week 21)"
SECTION[8]="Phase 8 — Documentation + release (Weeks 22–24)"

for P in "${PHASES[@]}"; do
  OUT="ucil-build/feature-list.parts/phase-$P.json"
  if [[ -f "$OUT" ]] && [[ "$(jq -r '. | length' "$OUT" 2>/dev/null || echo 0)" -gt 0 ]]; then
    echo "[phase-$P] already generated, skip"
    continue
  fi

  echo ""
  echo "=== Seeding phase $P ==="
  PROMPT=$(cat <<EOF
You generate a JSON array (no envelope, no commentary, just the array) of
UCIL feature entries for ONLY phase $P. Read
ucil-master-plan-v2.1-final.md, search for the section titled
"${SECTION[$P]}", and derive feature entries from every week-level bullet
in that section plus any tool/test-category items specific to the phase.

Each feature entry must conform to this schema (all fields required):
{
  "id": "P${P}-W<week>-F<NN>",         // NN 01..99
  "phase": $P,                           // number (or 3.5)
  "week": <int>,
  "crate": "<crate or module>",
  "description": "<one precise testable sentence>",
  "acceptance_tests": [
    // one or more of:
    {"kind":"cargo_test","selector":"..."},
    {"kind":"pytest","selector":"..."},
    {"kind":"vitest","selector":"..."},
    {"kind":"script","path":"scripts/verify/P${P}-W<w>-F<nn>.sh","exit":0},
    {"kind":"bench","script":"scripts/bench-<x>.sh","assert":"<cond>"}
  ],
  "dependencies": ["<prior feature id>", ...],
  "passes": false,
  "last_verified_ts": null,
  "last_verified_by": null,
  "last_verified_commit": null,
  "attempts": 0,
  "blocked_reason": null
}

Target 15-35 features for this phase. Output the JSON array to
$OUT using the Write tool. Do NOT commit.
Do NOT write any other file. Exit cleanly when done.
EOF
)

  UCIL_SEEDING=1 \
  CLAUDE_SUBAGENT_NAME=planner \
  claude -p "$PROMPT" \
    --dangerously-skip-permissions \
    --append-system-prompt "$(cat .claude/agents/planner.md)" \
    2>&1 | tail -40

  if [[ ! -f "$OUT" ]] || [[ "$(jq -r '. | length' "$OUT" 2>/dev/null || echo 0)" -lt 5 ]]; then
    echo "[phase-$P] FAILED — $OUT missing or <5 features" >&2
    exit 2
  fi
  echo "[phase-$P] done: $(jq -r '. | length' "$OUT") features"
done

# Merge
echo ""
echo "=== Merging parts into feature-list.json ==="
jq -s --arg version "1.0.0" \
      --arg frozen_at "$FROZEN_AT" \
      --arg plan_sha "$PLAN_SHA256" \
      '{version: $version, frozen_at: $frozen_at, frozen_commit: null, source_plan_sha256: $plan_sha, features: (. | add)}' \
  ucil-build/feature-list.parts/phase-*.json \
  > ucil-build/feature-list.json

TOTAL=$(jq -r '.features | length' ucil-build/feature-list.json)
echo "Total features: $TOTAL"

# Create stub scripts for every script-kind acceptance test
mkdir -p scripts/verify
COUNT=0
for id in $(jq -r '.features[] | select(.acceptance_tests[]? | .kind=="script") | .id' ucil-build/feature-list.json); do
  target="scripts/verify/$id.sh"
  if [[ ! -f "$target" ]]; then
    cat > "$target" <<STUB
#!/usr/bin/env bash
echo "TODO: implement acceptance test for $id"
exit 1
STUB
    chmod +x "$target"
    COUNT=$((COUNT+1))
  fi
done
echo "Stub scripts created: $COUNT"

echo ""
echo "Done. Next:"
echo "  1. Review ucil-build/feature-list.json"
echo "  2. UCIL_SEEDING=1 git add ucil-build/feature-list.json scripts/verify/"
echo "  3. UCIL_SEEDING=1 git commit -m 'freeze: feature oracle v1.0.0'"
echo "  4. git push"
