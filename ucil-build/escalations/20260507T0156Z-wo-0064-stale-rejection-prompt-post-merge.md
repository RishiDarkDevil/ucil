---
severity: harness-config
blocks_loop: false
requires_planner_action: false
agent: executor
work_order: WO-0064
feature_ids: [P2-W8-F04]
---

# WO-0064 — executor re-invoked against a stale "rejection" prompt; work is verified PASS and already merged

## Summary

The orchestrator dispatched another executor turn for WO-0064 with the
synthesised "rejection retry" prompt, instructing me to apply remediation
from two artefacts that **do not exist**:

- `ucil-build/rejections/WO-0064.md` — does not exist
- `ucil-build/verification-reports/root-cause-WO-0064.md` — does not exist

This is the **same recurring harness bug** previously documented for
WO-0063 in:

- `ucil-build/escalations/20260506T2340Z-wo-0063-merge-gap-prompt-stale.md`
- `ucil-build/escalations/20260506T2358Z-wo-0063-stale-rejection-prompt-recurrence.md`
- `ucil-build/escalations/20260507T0032Z-wo-0063-stale-prompt-respawn-r4.md`

The recommended harness-fix in those escalations (a precondition guard in
the executor entry-point) is still pending; until it lands, every
post-merge re-dispatch burns one executor + critic + verifier turn for
zero work product.

## Ground truth on `main` HEAD `bbe645d`

| Fact | Evidence |
|------|----------|
| Branch already merged | commit `bbe645d merge: WO-0064 lancedb-chunk-indexer (orphan → main) — P2-W8-F04 closure` |
| Implementation present on main | `crates/ucil-daemon/src/lancedb_indexer.rs` exists on `main` |
| P2-W8-F04 already `passes=true` | `jq '.features[] \| select(.id == "P2-W8-F04")'` → `passes: true, last_verified_by: verifier-7f7ea48e-..., last_verified_ts: 2026-05-07T01:52:36Z, attempts: 0` |
| Verifier verdict PASS | `ucil-build/verification-reports/WO-0064.md` line 7: `**Verdict**: **PASS**` |
| Verifier flip commit | `5c36efd chore(verifier): WO-0064 PASS — flip P2-W8-F04 + reports` |
| Critic verdict CLEAN | `ucil-build/critic-reports/WO-0064.md` line 8: `**Verdict**: **CLEAN**` |
| Critic commit | `ed09fe1 chore(critic): WO-0064 critic report — CLEAN` |
| RFR marker present | `ucil-build/work-orders/0064-ready-for-review.md` (final-sha `e737892`) |
| Mutation contract verified | AC27-AC30 in verification-report: M1 → SA2 panic, M2 → SA4 panic, M3 → SA4 panic, M4 → SA2 panic — all four mutations fired the predicted failure mode and restored cleanly |
| Coverage gate green | `[coverage-gate] PASS — ucil-daemon line=89% branch=n/a` (line 49 of RFR) |
| Workspace tests | `cargo test --workspace --no-fail-fast` PASS, zero `FAILED` lines (AC21) |

The work-order is fully verified and merged into trunk. There is no diff
to apply.

## Why I did not write code or rewrite the RFR marker

Per root `CLAUDE.md` anti-laziness contract and the executor protocol:

1. **No rejection or RCF artefact exists to remediate from.** Following
   non-existent recommendations would be fabrication — exactly the
   failure mode the verifier protocol is designed to catch.
2. **`feature-list.json:P2-W8-F04.passes` is already `true`.** That
   field is verifier-only, whitelisted, and monotone; the executor
   cannot un-flip it, and there is no rejection to revert to.
3. **No new diff is possible.** The feature branch HEAD `e737892`
   has been fully merged into `main` at `bbe645d`. Any "remediation"
   commit would either be a no-op (duplicating verified content) or
   invent scope outside the work-order, which the
   anti-laziness contract forbids.
4. **The marker file the prompt expects** (`0064-lancedb-chunk-indexer-ready-for-review.md`)
   does not match the marker that the original execution wrote and
   that the critic + verifier reviewed (`0064-ready-for-review.md`).
   Writing a new marker under a different name would not constitute
   remediation; it would just leave two RFR files for the same WO and
   confuse the next reader.

## Worktree state

- Main repo (`/home/rishidarkdevil/Desktop/ucil`): `HEAD == origin/main == bbe645d`. Tree clean.
- WO-0064 worktree (`../ucil-wt/WO-0064`): `HEAD == feat/WO-0064-lancedb-chunk-indexer == e737892` (the verified-PASS tip; merged into `main` at `bbe645d` via the orphan-merge path).

No uncommitted modifications anywhere; nothing to clean up.

## Recommended remediation (Bucket A — admin/benign auto-resolve)

The underlying condition is already fixed in HEAD. Triage should:

1. Append a `## Resolution` note pointing to commit `bbe645d` (merge)
   and `5c36efd` (verifier flip).
2. Set `resolved: true` (already set in this file's footer).
3. Commit + push.

## The recurring harness bug (still pending fix)

Something in the orchestrator continues to dispatch
`scripts/run-executor.sh` with a synthesised rejection-retry prompt for
work-orders that have ALREADY been verified-PASS and merged. The
guardrail recommended in the WO-0063 escalations is still applicable
verbatim:

```bash
# In scripts/run-executor.sh (or wherever the prompt is composed):
if [[ "$prompt_kind" == "rejection-retry" ]]; then
  rejection_file="ucil-build/rejections/${WO_ID}.md"
  rcf_file="ucil-build/verification-reports/root-cause-${WO_ID}.md"
  if [[ ! -f "$rejection_file" ]] && [[ ! -f "$rcf_file" ]]; then
    echo "ABORT: rejection-retry dispatched for $WO_ID but no rejection/RCF exists" >&2
    exit 64
  fi
fi
```

A complementary guard should also short-circuit any retry against a WO
whose feature_ids are all `passes=true` AND whose feature branch HEAD is
already an ancestor of `origin/main`. Both conditions hold for WO-0064
right now.

This is now the **second WO** (after WO-0063) hit by the same bug. A
`harness-fixer` invocation is overdue.

## What I am NOT doing in this turn

- **No source-code edits.** The work is verified and merged.
- **No `feature-list.json` writes.** Whitelist-protected; already
  `passes=true`.
- **No new ready-for-review marker file.** The original
  (`0064-ready-for-review.md`) was reviewed by the critic and accepted
  by the verifier at `e737892`.
- **No `git push --force` / `--amend` / `--no-verify`.** Forbidden.
- **No retry of the verifier protocol.** A second verifier pass against
  a WO that is already `passes=true` cannot do anything useful — the
  field is monotone.

## Cross-references

- `ucil-build/work-orders/0064-lancedb-chunk-indexer.json` — work-order
- `ucil-build/work-orders/0064-ready-for-review.md` — RFR marker (final-sha `e737892`)
- `ucil-build/critic-reports/WO-0064.md` — CLEAN critic verdict
- `ucil-build/verification-reports/WO-0064.md` — PASS verifier verdict (vrf-7f7ea48e)
- `feat/WO-0064-lancedb-chunk-indexer @ e737892` — verified feature-branch HEAD
- `main @ bbe645d` — merge of WO-0064 (P2-W8-F04 closure)
- WO-0063 stale-prompt precedents:
  - `ucil-build/escalations/20260506T2340Z-wo-0063-merge-gap-prompt-stale.md`
  - `ucil-build/escalations/20260506T2358Z-wo-0063-stale-rejection-prompt-recurrence.md`
  - `ucil-build/escalations/20260507T0032Z-wo-0063-stale-prompt-respawn-r4.md`

resolved: true
