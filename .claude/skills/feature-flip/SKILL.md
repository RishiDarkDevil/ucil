---
name: feature-flip
description: Verifier-only shortcut. Spawn a fresh verifier session to verify ONE feature and flip passes=true if it checks out.
allowed-tools: Bash, Read, Task
---

# /feature-flip <FEATURE-ID>

Spawn a fresh verifier session to verify a single feature and flip `passes=true` if it passes.

## Steps

1. Validate that `$1` exists in `feature-list.json`.
2. Validate that the feature's `dependencies` are all `passes=true`.
3. Run `scripts/spawn-verifier.sh $1` — this creates a new Claude Code session with `--no-resume`, writes the session marker to `ucil-build/.verifier-lock`, and runs the verifier subagent with the feature ID.
4. The verifier runs the feature's `acceptance_tests`, runs the mutation check, then calls `scripts/flip-feature.sh $1 pass <commit>` on success or writes a rejection.
5. Print the verifier's verdict and verification report path.

## Arguments

- `$1` — feature ID (e.g., `P1-W2-F03`). Required.

## Notes

- This is useful for one-off verification outside the normal work-order flow (e.g., after a manual fix).
- The session must be new (not resume) — `scripts/spawn-verifier.sh` enforces this.
