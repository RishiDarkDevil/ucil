---
timestamp: 2026-05-05T20:19:10Z
type: verifier-rejects-exhausted
work_order: WO-0053
verifier_attempts: 4
max_feature_attempts: 1
severity: high
blocks_loop: true
---

# WO-0053 hit verifier-reject cap

Verifier ran 4 times on WO-0053; at least one feature has
attempts=1.

Latest rejection: ucil-build/rejections/WO-0053.md
Latest root-cause: ucil-build/verification-reports/root-cause-WO-0053.md (if present)

If the rejection cites harness-script bugs (reality-check.sh,
flip-feature.sh, a hook, a launcher), triage may auto-resolve this
escalation via Bucket B before the loop halts.

## Resolution

Resolved 2026-05-06 by triage (phase 2, pass 1) — Bucket A.

This is the SECOND stale-invocation duplicate. The underlying feature
P2-W7-F09 already passes in HEAD (verifier-f1555418, commit dfd07727).
The verifier-attempts counter went 3 → 4 because the orchestrator spawned
yet another verifier on the already-flipped feature; the verifier
session correctly logged itself as STALE_INVOCATION_NO_OP v2 in
`ucil-build/verification-reports/WO-0053-stale-noop-v2.md` (commit
`87fbceb`).

Current state:

```
$ jq '.features[] | select(.id=="P2-W7-F09") | {id, passes, last_verified_by, last_verified_commit, attempts}' \
    ucil-build/feature-list.json
{
  "id": "P2-W7-F09",
  "passes": true,
  "last_verified_by": "verifier-f1555418-4f01-4a5b-93b6-ff3b063560b5",
  "last_verified_commit": "dfd07727469daf95d96a73b486436eb8831b8a0f",
  "attempts": 1
}
```

The feature-list-level invariants are intact:
- `passes: true`, set by a fresh-session verifier (not executor).
- `attempts: 1` — well below the 3-attempts cap.
- `last_verified_by` starts with `verifier-`, satisfying gate(N).

The WO-level `verifier_attempts: 4` counter is an orchestrator artefact
that does NOT correspond to any real verifier work after the first PASS.
No code, harness, ADR, or planner action required.

The recommended permanent fix (refuse to spawn verifier when
`feature-list.json[<feat>].passes == true`) is documented in the v2
addendum and should be picked up the next time someone visits
`scripts/run-phase.sh`. That is intentionally out of triage scope this
pass; this duplicate has the same Bucket A treatment as
`9588cdc chore(escalation): resolve WO-0053-attempts-exhausted — bucket A`.

resolved: true
