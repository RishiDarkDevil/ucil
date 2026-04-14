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

## Recovery after a crash, PC shutdown, or manual pause

**One-shot:**
```bash
./scripts/resume.sh          # interactive: prompt before restarting the loop
./scripts/resume.sh --yes    # clean up and auto-restart run-phase.sh
./scripts/resume.sh --check  # clean up only (useful before manual inspection)
```

`resume.sh` performs these idempotent cleanup steps in order:
1. Aborts any in-progress `git merge` / `git rebase` in main and in every `../ucil-wt/WO-*` worktree.
2. Removes `ucil-build/.verifier-lock` if no `claude -p` verifier process is alive.
3. Resets `.ucil-triage-pass.phase-*` counters so triage gets a fresh 3 passes.
4. **Warns** about uncommitted changes in worktrees — never auto-commits or auto-discards (you decide).
5. Refuses to auto-start if main itself is dirty.
6. Pushes any local-only commits to origin so no work is left behind.
7. Pulls main (fast-forward) to pick up anything agents pushed before the crash.
8. Prints a state summary (phase, features passing, WOs, open escalations, rejections, main HEAD).
9. Re-execs `scripts/run-phase.sh <current-phase>` if you confirm (or `--yes`).

## What survives a shutdown

| Artifact | Survives? | Resume effect |
|---|---|---|
| Committed git state (pushed) | ✓ | Authoritative source of truth |
| Uncommitted changes in a worktree | ✓ (as untracked) | `resume.sh` warns; you commit-with-`wip:`-prefix or discard |
| `ucil-build/` tracked files | ✓ | Picked up by next iteration |
| `ucil-build/.verifier-lock` | ✓ (stale) | `resume.sh` clears it |
| `.ucil-triage-pass.phase-*` | ✓ | `resume.sh` resets them |
| Session `.jsonl` logs under `~/.claude/projects/` | ✓ up to last write | Post-mortem only; can't resume a turn |
| Running `claude -p` process | ✗ killed | Next agent spawns fresh |
| `.git/MERGE_HEAD` / `.git/REBASE_HEAD` | ✓ (frozen) | `resume.sh` aborts |

## What gets lost (and it's OK)

- **The specific in-flight agent turn.** The agent's plan-of-the-moment is gone. But every agent commits and pushes at every logical checkpoint, so the *work* is recoverable from origin.
- **Parts of a partially-generated file the executor hadn't committed yet.** These sit in the worktree untracked. `resume.sh` surfaces them; you decide keep-or-discard.

## Manual recovery if `resume.sh` is unhappy

If `resume.sh` refuses to auto-start (e.g., dirty main, ambiguous state):

```bash
# 1. Inspect
git status
git worktree list
ls ucil-build/escalations/
ls ucil-build/rejections/

# 2. If a worktree has half-done work worth preserving:
git -C ../ucil-wt/WO-NNNN commit -am 'wip: resume checkpoint'
git -C ../ucil-wt/WO-NNNN push

# 3. If discardable (e.g. executor crashed mid-experiment):
git -C ../ucil-wt/WO-NNNN reset --hard
git -C ../ucil-wt/WO-NNNN clean -fd

# 4. Then re-run:
./scripts/resume.sh --yes
```

All state is in git. To recover on a **fresh machine** (e.g. disk failure), clone the repo, fill `.env`, run `./scripts/install-prereqs.sh`, then `./scripts/resume.sh` — the harness picks up from the last pushed commit.

## Not for casual editing

**Do not hand-edit `feature-list.json`.** The pre-commit hook will reject anything other than the six mutable fields. If you truly need to change the spec (e.g., a feature should be split), write an ADR and reseed.
