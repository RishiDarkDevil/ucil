---
name: phase-ship
description: Ship phase N — verify gate, tag release, write post-mortem stub, advance progress.json. Only runs if gate is green.
allowed-tools: Bash, Read, Write, Task
---

# /phase-ship <N>

Ship phase N. This is a terminal action for the phase — after it, progress.json moves to phase N+1.

## Steps

1. Run `scripts/gate-check.sh $N`. If non-zero, ABORT with "gate failed, fix before shipping".
2. Create annotated git tag `phase-$N-complete`:
   ```
   git tag -a phase-$N-complete -m "Phase $N of UCIL complete. Features <M>/<M> passing."
   git push origin phase-$N-complete
   ```
3. Spawn the `docs-writer` subagent to draft `ucil-build/post-mortems/phase-$N.md` from verification reports and git log. Wait for it to commit.
4. Update `ucil-build/progress.json`:
   ```json
   { "phase": <N+1>, "week": 1, "last_shipped_phase": <N>, "last_shipped_at": "<iso-ts>" }
   ```
5. Commit progress.json, push.
6. Print summary: "Phase $N shipped. Review ucil-build/post-mortems/phase-$N.md. When ready, /phase-start $((N+1))."

## Arguments

- `$1` — phase number (0-8). Required.

## Pre-flight checks

- Gate must be green.
- Working tree must be clean.
- Branch must be up-to-date with upstream.
