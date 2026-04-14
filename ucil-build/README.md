# ucil-build/ — UCIL Build Harness

This directory is the **brain of the autonomous build** — separate from UCIL's runtime (`.ucil/`) and from UCIL's source (`crates/`, `adapters/`, `ml/`).

Read `CLAUDE.md` in this directory for the harness contract (immutability, roles, escalation protocol).

## Top-level files

- `feature-list.json` — immutable oracle. Seed once, mutate only `passes`/`last_verified_*`/`attempts`/`blocked_reason` via `scripts/flip-feature.sh`. See `schema/feature-list.schema.json`.
- `progress.json` — phase/week state. Orchestrator scripts mutate.
- `CLAUDE.md` — rules of engagement for every agent working under this harness.

## Subdirectories

- `work-orders/` — planner → executor. One file per batch, JSON.
- `verification-reports/` — verifier output per work-order.
- `rejections/` — verifier rejections.
- `critic-reports/` — critic findings.
- `escalations/` — pages to human. Surfaced at session start.
- `decisions/` — ADRs, append-only markdown.
- `post-mortems/` — per-phase retrospectives, written by docs-writer at phase-ship.
- `phase-log/NN-phase-N/` — per-phase scoped CLAUDE.md + session logs.
- `schema/` — JSON Schema definitions.

## Recovery

If the host crashes mid-build:
1. `git status` — commit or discard anything in the worktree that looks half-done.
2. `cat ucil-build/progress.json` — confirm the phase/week.
3. `ls ucil-build/work-orders/` — find the latest work-order.
4. `git log --oneline -10` — see where the branch left off.
5. Resume with `/phase-start <N>` or `/replan`.

All state is in git. Pull from origin to recover on a fresh machine.

## Not for casual editing

**Do not hand-edit `feature-list.json`.** The pre-commit hook will reject anything other than the six mutable fields. If you truly need to change the spec (e.g., a feature should be split), write an ADR and reseed.
