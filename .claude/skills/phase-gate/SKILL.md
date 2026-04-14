---
name: phase-gate
description: Run scripts/gate-check.sh for phase N. Report pass/fail with details. Does not modify state.
allowed-tools: Bash, Read
---

# /phase-gate <N>

Check whether phase N is complete (all features pass, phase-N-specific checks green, no flake-quarantined tests).

## Steps

1. Run `scripts/gate-check.sh $N` and capture the full output.
2. If exit code 0: print a green "PHASE N GATE PASSED" banner with the feature count and next-step suggestion (`/phase-ship N`).
3. If non-zero: print a red "PHASE N GATE FAILED" banner followed by the script's output, then a summary of what to fix (failing features, failing criteria).

## Arguments

- `$1` — phase number (0-8). Optional; defaults to `jq .phase ucil-build/progress.json`.

## Notes

- Read-only. Does not flip features. Does not modify progress.json.
- Safe to run at any time; diagnostic only.
