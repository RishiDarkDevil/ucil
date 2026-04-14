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

The outer loop (`scripts/run-phase.sh`) halts when:
- Same feature fails verifier 3×
- Drift counter ≥ 4
- OOM / timeout twice consecutively
- Cross-feature conflict
- Any commit touches this file or feature-list.json outside the whitelist
- `attempts >= 10` on any feature
- Mutation-check failure
- User manually `/escalate`s

## Read before you do anything

1. `cat ucil-build/progress.json` — know the current phase/week.
2. `ls ucil-build/escalations/` — resolve open escalations first.
3. `ls -t ucil-build/decisions/` — know the recent ADRs.
4. `cat ucil-build/phase-log/NN-phase-N/CLAUDE.md` (if it exists) — phase-scoped rules.
