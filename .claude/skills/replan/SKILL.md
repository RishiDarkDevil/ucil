---
name: replan
description: Discard stale work-orders and spawn the planner fresh. Use when drift is detected (no features flipped across N turns) or when the user wants to change direction.
allowed-tools: Bash, Read, Write, Task
---

# /replan

Reset the current planning state and ask the planner to start over from the current progress.json.

## Steps

1. Show the user what's about to happen:
   - List open work-orders in `ucil-build/work-orders/` that haven't been completed.
   - List drift counter state from `ucil-build/drift-counters.json` if present.
2. Mark stale work-orders as `blocked_reason: "replanned"` (don't delete — keep the history).
3. Reset drift counter to 0.
4. Spawn the `planner` subagent fresh with current progress.json state.
5. Print the new work-order path.

## Arguments

- None (uses current progress.json state).

## Notes

- This does not revert code or feature flips. It only discards planning artifacts.
- Use when the critic/verifier keeps rejecting the same approach — maybe the decomposition was wrong.
