---
id: DEC-0006
title: Defer parallel executor worktrees until Phase 2 throughput demands it
date: 2026-04-17
status: accepted
raised_by: orchestrator
blocks_work_order: none
---

# DEC-0006: Defer parallel executor worktrees

## Context

Tier-3 audit item #3.2 proposed spawning multiple executors concurrently —
one per work-order — so the orchestrator processes N WOs in parallel rather
than the serial `planner → executor → critic → verifier → merge` path.

Today, one phase iteration takes ~25 minutes wall-clock end-to-end
(measured on Phase 1 WO-0001…WO-0006). At 34 remaining features in Phase 1
and ~3 features per WO, that's ~10 WOs × 25 min = ~4 hours of actual
autonomous loop time. API spend is a much tighter ceiling than wall clock:
`$113/day observed` on a `$50/day cap` means the bottleneck is cost, not
throughput.

Parallel executors would add:

1. **Merge serialization:** `merge-wo.sh` would need a flock on `main`'s
   HEAD to prevent races. Fast-forward merges already do implicit lockout
   via `git push` remote rejection, but races on `safe_git_pull` and the
   feature-list-snapshot inside `flip-feature.sh` would need explicit
   locks.
2. **.ucil-state conflicts:** each worktree's integration tests create
   their own `.ucil/branches/<branch>/`, but the `.ucil/shared/` LMDB env
   is single-writer. Running two executors' `cargo nextest` in parallel
   would require env-var indirection to distinct `UCIL_DATA_DIR`s per
   worktree.
3. **Verifier lock:** `ucil-build/.verifier-lock` is currently a single
   file. N parallel verifiers need N lock files (per-WO) or a mutex, which
   the current feature-list-guard hook doesn't account for.
4. **Daily-cost-cap coordination:** two executors racing to spend against
   a shared cap need a cross-process counter, not just a per-process one.

## Decision

Keep the orchestrator serial through Phase 1. Re-evaluate at Phase 2 entry.

Phase 2 introduces embeddings (Python ONNX), LanceDB, and first-party
plugins — the feature-list gets wide enough (~30 features per week vs.
Phase 1's 8–10) that serial WO cadence may materially slow delivery.
At that point, revisit:

- Run a one-week measurement to confirm serial throughput is the bottleneck
  (vs. cost cap).
- If confirmed, introduce `scripts/run-phase-parallel.sh` that:
  - Takes `UCIL_PARALLEL_EXECUTORS=N` (default 1).
  - Maintains an `ucil-build/.merge-lock` flock.
  - Sharded `UCIL_DATA_DIR=/tmp/ucil-wt-{WO_ID}` per worktree.
  - Per-WO verifier lock file `.verifier-lock-{WO_ID}`.
  - Coordinated cost-cap via a single `_cost-budget.sh safe_check_daily_budget`
    call before each executor spawn (already cross-process since it reads
    from `~/.claude/projects/*/*.jsonl`).
  - Serial critic → verifier → merge-wo step per WO (so merges remain
    ordered and predictable).

## Rationale

- **Risk over speed:** the harness is 14 days young and has hit three
  different halts already (merge-failure, verifier-exhausted,
  critic-blocks-on-commit-size). Adding concurrency multiplies the
  state-space of potential halts, and triage isn't battle-tested enough
  yet to handle race conditions.
- **Cost, not time, is the binding constraint:** `$113/day observed` on a
  `$50/day cap`. Parallelism = more cost, not more throughput. Effective
  throughput is already rate-limited by the cap.
- **YAGNI.** Phase 1 throughput (serial) completes Phase 1 in ~4 hours of
  actual loop time. That's fine.

## Consequences

- Tier-3 audit item #3.2 remains deferred. No harness code change until
  Phase 2 entry.
- If a phase-1 feature scope grows (e.g., W4 has 10 features), we revisit
  earlier.
- `run-phase.sh` remains the single-executor orchestrator.
- `scripts/run-phase-parallel.sh` is NOT created until Phase 2 entry.

## Revisit trigger

At Phase 2 entry: measure one week of serial throughput vs. feature flow.
If WO completion is < 60% of planned-per-week at the end of Phase 2 week 1,
implement parallel execution per the sketch in the Decision section.
