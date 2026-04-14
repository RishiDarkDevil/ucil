# ucil-build/ — Harness Brain Instructions

This directory is the **build harness**, not UCIL's runtime. UCIL's runtime uses `.ucil/` (created per-project, not here).

## Contents

| Path | Purpose | Mutators |
|------|---------|----------|
| `feature-list.json` | Immutable oracle; seed once, then only `passes`/`last_verified_*`/`attempts`/`blocked_reason` may change | verifier via `scripts/flip-feature.sh` |
| `feature-list.schema.json` | JSON Schema for validation | one-shot, never change post-freeze |
| `progress.json` | Phase/week/branch state | orchestrator scripts |
| `CLAUDE.md` (this file) | Harness contract | planner (rare) |
| `work-orders/` | Planner → executor envelopes | planner |
| `verification-reports/` | Verifier output per work-order | verifier |
| `rejections/` | Verifier rejections | verifier |
| `critic-reports/` | Critic findings | critic |
| `escalations/` | Pages to user | any agent |
| `decisions/` | ADRs refining the spec (append-only) | planner |
| `post-mortems/` | Per-phase retrospectives | docs-writer |
| `phase-log/NN-phase-N/` | Per-phase scoped CLAUDE.md + session logs | planner |
| `drift-counters.json` | Counter of consecutive executor turns with no flip | orchestrator |
| `.verifier-lock` | Session marker proving verifier is active (gitignored) | `scripts/spawn-verifier.sh` |

## Immutability of feature-list.json

After the one-time seed commit (message `freeze: feature oracle v1.0.0`), only these fields may change:

- `passes` (false → true, never back)
- `last_verified_ts`
- `last_verified_by`
- `last_verified_commit`
- `attempts` (integer, monotonically increasing)
- `blocked_reason` (nullable string)

The git pre-commit hook `.githooks/pre-commit-feature-list` rejects any diff that touches other fields. If the spec legitimately needs amendment, write an ADR, get user confirmation, and re-seed with a bumped schema version (rare and painful — avoid).

## Decisions (ADRs)

Every non-obvious choice the planner or the user makes lives as `decisions/DEC-NNNN-<slug>.md`:

```markdown
# DEC-0007: LMDB vs sled for tag cache

**Status**: accepted
**Date**: 2026-04-15
**Context**: Phase 1 Week 2 requires a tag cache keyed by (path, mtime).
**Decision**: Use LMDB via heed, not sled.
**Rationale**: heed has a stabler API, LMDB's performance is better-understood
for this access pattern, rust-analyzer uses it.
**Consequences**: adds `heed` dep; `sled` evaluation doc lives in this file.
**Revisit trigger**: if sled's API stabilizes AND benchmarks show >20% win.
```

Append-only. Never delete an ADR; supersede with a new one that references the old.

## Phase log

For each phase, `phase-log/NN-phase-N/CLAUDE.md` is planner-synthesized at phase start and contains:
- Goals summary (one paragraph from master plan §18 Phase N)
- Features in scope (list of IDs)
- Gate criteria (references to scripts/gate/phase-N.sh)
- Deps required (docker images, external services)
- Risks carried from previous phase

Session transcripts and intermediate artifacts go in `phase-log/NN-phase-N/session-YYYYMMDD.jsonl`.

## Escalation protocol

When blocked, write `escalations/YYYYMMDD-HHMM-<slug>.md` (see `.claude/skills/escalate/SKILL.md`). Escalations are surfaced at next session-start via the user-prompt-submit hook.

### Escalation frontmatter conventions

- `blocks_loop: true|false` — if `false`, triage may auto-resolve when the underlying condition is already fixed in HEAD.
- `severity: low|harness-config|high|critical` — `harness-config` is triage's sweet spot for Bucket B fixes.
- `requires_planner_action: true|false` — when `true`, triage defaults to halt.
- Always end the file with `resolved: true` (frontmatter field OR trailing line) when the issue is closed — the outer loop and triage both detect this marker.

### Who handles what

The outer loop (`scripts/run-phase.sh`) does NOT halt on escalations immediately. It first spawns the **triage subagent** (`.claude/agents/triage.md`), which classifies each unresolved escalation:

- **Bucket A (auto-resolve)**: admin/benign escalation whose condition is already resolved in HEAD → triage appends a `## Resolution` note, sets `resolved: true`, commits, pushes.
- **Bucket B (fix + resolve)**: concrete < 120-line fix in `.githooks/`, `.claude/hooks/` (except `stop/gate.sh`), `scripts/` (except `gate/**` and `flip-feature.sh`), or `ucil-build/schema/` (except `feature-list.schema.json`) → triage applies the fix, runs a local smoke check, commits, appends a resolution note with the fix-commit sha, sets `resolved: true`, pushes.
- **Bucket C (halt + page user)**: anything else — UCIL source, ADR-required, ≥3 prior attempts, deny-list file, drift, OOM, cost-cap, low confidence → leave unresolved, append one line to `triage-log.md`, the outer loop halts.

Triage runs up to 3 times per phase. On pass 3 it defaults everything to Bucket C to prevent thrashing.

The outer loop ALSO halts unconditionally (regardless of triage) when:
- Same feature fails verifier 3× (`attempts` field in feature-list)
- Drift counter ≥ 4 (consecutive no-flip iterations) — auto-filed as an escalation
- `attempts >= 10` on any feature
- Cross-feature conflict
- OOM / timeout twice consecutively
- Any commit touches this file or `feature-list.json` outside the whitelist
- Mutation-check failure
- User manually `/escalate`s

## Read before you do anything

1. `cat ucil-build/progress.json` — know the current phase/week.
2. `ls ucil-build/escalations/` — resolve open escalations first.
3. `ls -t ucil-build/decisions/` — know the recent ADRs.
4. `cat ucil-build/phase-log/NN-phase-N/CLAUDE.md` (if it exists) — phase-scoped rules.
