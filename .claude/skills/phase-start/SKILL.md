---
name: phase-start
description: Start or resume work on phase N. Loads phase-scoped context, updates progress.json, spawns the planner subagent to emit the next work-order.
allowed-tools: Read, Bash, Write, Task
---

# /phase-start <N>

Start (or resume) work on UCIL phase N.

## Steps

1. If `$1` (phase number) is provided and differs from `jq .phase ucil-build/progress.json`, update progress.json with the new phase and reset week to 1.
2. Ensure `ucil-build/phase-log/NN-phase-N/CLAUDE.md` exists. If not, ask the planner to generate it from the master plan's §18 Phase N section.
3. Print the phase dashboard: which features, how many passing, dependency graph.
4. Spawn the `planner` subagent with input:
   - Phase number: N
   - Current week: from progress.json
   - Feature-list state
5. Planner emits one work-order (1-5 features) to `ucil-build/work-orders/`.
6. Print the work-order path and suggest: "Next: spawn the executor with `claude -p 'implement work-order NNNN'` or issue /phase-gate N when ready to check completion."

## Arguments

- `$1` — phase number (0-8). Optional; defaults to `.phase` in progress.json.
- `$2` — week number. Optional; defaults to `.week` in progress.json.

## Notes

- This skill does not itself run executors — it only gets the planner moving. The outer loop (`scripts/run-phase.sh`) orchestrates the full planner → executor → critic → verifier cycle.
