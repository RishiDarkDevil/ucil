---
severity: harness-config
blocks_loop: false
requires_planner_action: false
agent: verifier
work_order: WO-0064
feature_ids: [P2-W8-F04]
resolved: true
---

# WO-0064 — verifier re-spawned post-merge (recurrence #3); WO is verified PASS + already merged

## Summary

A fresh `verifier` subagent was dispatched at `2026-05-07T02:20Z` with the
prompt:

> You are the UCIL verifier. Target to verify: WO-0064.
> Run all acceptance tests from a clean slate. Flip passes=true only if
> everything is green and the mutation check confirms.

This is the **third** stale verifier-prompt recurrence on WO-0064
post-merge. r1 was filed at `20260507T0205Z…` and r2 at
`20260507T0213Z…` — both already self-resolved (Bucket A) with the same
ground truth I document below.

## Ground truth on `main` HEAD `db771f0`

| Fact | Evidence |
|------|----------|
| Feature already `passes=true` | `jq '.features[] \| select(.id=="P2-W8-F04")' ucil-build/feature-list.json` → `passes: true, last_verified_by: verifier-7f7ea48e-…, last_verified_commit: e737892…, attempts: 0, blocked_reason: null` |
| Verifier report present, PASS | `ucil-build/verification-reports/WO-0064.md` line 7 → `**Verdict**: **PASS**` (verifier session `vrf-7f7ea48e-8254-4118-b031-ea2a01b5d1d5`, dated `2026-05-07T01:52:36Z`) |
| Verifier flip commit | `5c36efd chore(verifier): WO-0064 PASS — flip P2-W8-F04 + reports` |
| Critic verdicts CLEAN | `ed09fe1` (initial), `4c08d29` (retry-1), `593268f` (retry-2) — all CLEAN |
| Branch merged into main | `bbe645d merge: WO-0064 lancedb-chunk-indexer (orphan → main) — P2-W8-F04 closure` |
| `git merge-base --is-ancestor feat/WO-0064-lancedb-chunk-indexer origin/main` | exits `0` → already merged |
| `rejections/WO-0064.md` | does not exist (never rejected) |
| Working tree | clean; `HEAD == origin/main == db771f0` |

## Why this verifier session is a no-op

Per `.claude/agents/verifier.md` and the root anti-laziness contract:

1. **`passes` is monotone.** `false → true` only; `true → true` would be a
   no-op write. The `.githooks/pre-commit-feature-list` hook + the
   verifier-only `flip-feature.sh` enforce this — the second flip just
   churns `last_verified_*` fields without changing audit truth.
2. **No path from `passes=true` back to `passes=false`** without an ADR +
   re-seed (rare and painful, per `ucil-build/CLAUDE.md`).
3. **Source edits are forbidden.** Verifier hard rule.
4. **The feature branch is fully merged** at `bbe645d`. The worktree
   `feat/WO-0064-lancedb-chunk-indexer @ e737892` is identical to a subset
   of trunk's history; re-running acceptance tests would reproduce the
   exact same outcome already captured in the canonical PASS report.
5. **Spawning a fresh verifier session against a `passes=true` WO is a
   harness bug**, not a missing test pass. The verifier protocol assumes
   the feature is in `passes=false` state at session start.

## What I am NOT doing in this turn

- **No source-code edits** (forbidden per `.claude/agents/verifier.md`
  hard rules).
- **No `feature-list.json` writes** (re-flipping `true → true` is a no-op
  that pollutes the audit trail with a stale `last_verified_ts` /
  `last_verified_commit` for state that already reflects truth at
  `e737892`).
- **No new `verification-reports/WO-0064.md`** (the canonical PASS report
  from `vrf-7f7ea48e` is the ground truth at `e737892`/`bbe645d`;
  overwriting it would lose the original 37-row AC table + four-mutation
  audit trail and replace `e737892` with a stale main-tip reference).
- **No new `rejections/WO-0064.md`** (nothing failed; the work is verified
  PASS and merged — writing a rejection would be false).
- **No re-run of `cargo clean && cargo test --workspace`** (the original
  vrf-7f7ea48e session burned ~30 wall-min on this gauntlet; repeating it
  for a verified+merged WO is pure waste with zero state-change
  potential).
- **No new RFR marker, no new critic-report, no new flip commit.**
- **No `git push --force` / `--amend` / `--no-verify`.** Forbidden.

## Why the existing `c6609b9` post-rejection guard does NOT cover this case

The guard at `scripts/run-phase.sh:306-334` (added in commit `c6609b9`)
fires **after** the verifier rejection branch and **after**
`run-root-cause-finder.sh`. It correctly catches the rejection-retry
executor/critic dispatch — but it does NOT cover the **initial verifier
dispatch** for a WO whose feature_ids are already `passes=true` on main.
Concretely, the orchestrator currently:

1. Spawns the verifier first;
2. Inspects feature-list state;
3. Maybe dispatches RCF + executor-retry.

The verifier-side guard fires too late: by the time we reach line 329,
the verifier has already burned a turn. r2 (T0213Z) proposed hoisting the
guard above the verifier-spawn block; that fix has not yet landed, which
is why we now have r3.

## Recurrence accounting

| # | Date | Subagent | WO | Net effect |
|---|------|----------|----|------------|
| 1 | 2026-05-06T23:40Z | executor (stale RFR) | WO-0063 | wasted turn |
| 2 | 2026-05-06T23:58Z | executor (recurrence) | WO-0063 | wasted turn |
| 3 | 2026-05-07T00:32Z | executor (r4) | WO-0063 | wasted turn |
| 4 | 2026-05-07T01:56Z | executor (post-merge) | WO-0064 | wasted turn |
| 5 | 2026-05-07T02:05Z | verifier (post-merge) | WO-0064 | wasted turn (r1) |
| 6 | 2026-05-07T02:07Z | executor (post-merge r2) | WO-0064 | wasted turn |
| 7 | 2026-05-07T02:13Z | verifier (post-merge r2) | WO-0064 | wasted turn |
| **8** | **2026-05-07T02:20Z** | **verifier (post-merge r3)** | **WO-0064** | **wasted turn (this file)** |

The `harness-fixer` invocation is now critical. The recommended remediation
remains the **pre-verifier guard** described in r2 (T0213Z), pseudocode
reproduced for convenience:

```bash
# In scripts/run-phase.sh, immediately before the verifier-spawn block:
_verified_and_merged=1
for _fid in $WO_FEATURES; do
  _p=$(jq -r --arg id "$_fid" '.features[] | select(.id==$id) | .passes' \
       ucil-build/feature-list.json 2>/dev/null)
  if [[ "$_p" != "true" ]]; then
    _verified_and_merged=0
    break
  fi
done
_feat_branch="feat/${WO_ID}-$(jq -r '.slug' "$LATEST_WO")"
if git rev-parse --verify "$_feat_branch" >/dev/null 2>&1; then
  if ! git merge-base --is-ancestor "$_feat_branch" "origin/main" 2>/dev/null; then
    _verified_and_merged=0
  fi
fi
if [[ "$_verified_and_merged" -eq 1 ]]; then
  echo "[run-phase] ${WO_ID}: all feature_ids passes=true AND feat branch merged into main — skipping spurious verifier re-dispatch."
  continue
fi
```

This complements the existing post-rejection guard at line 329-334. Both
guards together close the full post-merge re-dispatch loop:

1. **Pre-verifier guard (proposed)** → skip verifier spawn entirely when
   WO is already verified+merged.
2. **Post-rejection guard (already in `c6609b9`)** → if a verifier
   somehow does run and (incorrectly) "rejects" a verified+merged WO, do
   not dispatch the rejection-retry executor/critic.

## Recommended remediation (Bucket A — auto-resolve this file; Bucket B — harness-fix the underlying bug)

This escalation, like r1 (T0205Z) and r2 (T0213Z), is auto-resolvable on
its own merits — the underlying WO is verified + merged, no remediation is
required against the WO itself. The file is marked `resolved: true` in
the frontmatter.

The forward-looking action is the **pre-verifier guard** harness-fix
(Bucket B candidate, ~30 LOC in `scripts/run-phase.sh`, well within the
< 120 LOC Bucket B limit). A `harness-fixer` invocation to apply it is
**critically overdue** — every recurrence costs ~$1 of LLM spend +
~10 min of wall time + one wasted turn.

## Cross-references

- r1 escalation: `ucil-build/escalations/20260507T0205Z-wo-0064-stale-verifier-prompt-post-merge.md`
- r2 escalation: `ucil-build/escalations/20260507T0213Z-wo-0064-stale-verifier-prompt-post-merge-r2.md`
- Sibling executor-side r1: `ucil-build/escalations/20260507T0156Z-wo-0064-stale-rejection-prompt-post-merge.md`
- Sibling executor-side r2: `ucil-build/escalations/20260507T0207Z-wo-0064-stale-executor-prompt-post-merge-r2.md`
- Original verifier-PASS report: `ucil-build/verification-reports/WO-0064.md`
- Original critic-CLEAN report: `ucil-build/critic-reports/WO-0064.md`
- Verifier flip commit: `5c36efd`
- Merge-to-main commit: `bbe645d`
- Existing post-rejection guard: `scripts/run-phase.sh:306-334`
- Harness-fix that landed the post-rejection guard: commit `c6609b9`
- WO-0063 stale-prompt precedents:
  - `ucil-build/escalations/20260506T2340Z-wo-0063-merge-gap-prompt-stale.md`
  - `ucil-build/escalations/20260506T2358Z-wo-0063-stale-rejection-prompt-recurrence.md`
  - `ucil-build/escalations/20260507T0032Z-wo-0063-stale-prompt-respawn-r4.md`

## Resolution

Auto-resolved (Bucket A) — underlying WO is verified PASS + merged at
`bbe645d`; `P2-W8-F04.passes` is `true`; no source/feature-list edits
made by this verifier session. The `harness-fixer` Bucket B remediation
(pre-verifier guard) is filed as the forward-looking action to prevent
recurrences r4+.

resolved: true
