---
work_order: WO-0049
feature: P2-W7-F05
branch: feat/WO-0049-find-references-and-g1-source-production-wiring
ref: 54fccce0f80b303448d1eb5c229b94c5385a8424
attempts_before_rca: 1
rca_session: rca-WO-0049-2026-05-05-stale-revisit
remediation_owner: none — already shipped
status: RESOLVED — feature P2-W7-F05 flipped passes=true at 9b596ed
---

# Root Cause Analysis: WO-0049 (find_references MCP tool + 4 G1Source production-shape impls — `P2-W7-F05`)

**Analyst session**: `rca-WO-0049-2026-05-05-stale-revisit`
**Feature**: `P2-W7-F05`
**Branch**: `feat/WO-0049-find-references-and-g1-source-production-wiring`
**Branch HEAD**: `54fccce0f80b303448d1eb5c229b94c5385a8424` (8 commits ahead of `main`'s pre-merge state)
**`main` HEAD at RCA time**: `9b596ed` (`verify(daemon): WO-0049 PASS — flip P2-W7-F05 …`)
**Attempts before RCA**: 1 (retry-1 rejection on the original `f197ad1`)

## STATUS — already resolved

This RCA was triggered on a **stale rejection record**. Between the retry-1 rejection
at `7e8ff5a` (`chore(verifier): REJECT WO-0049 retry 1 — find_references feature
absent on branch`, 2026-05-05 06:36 IST) and now, the full remediation cycle
has executed:

| Time (IST) | Commit | Event |
|------------|--------|-------|
| 05:22 | `3b51c78` | Planner emits WO-0049 |
| 05:28 | `f197ad1` | Executor lands ONLY scope_in steps 7+8 (the two foundational refactors) — single +25/−4 commit |
| 05:39 | `6955d19` | Critic verdict **BLOCKED** — 14 of 16 scope_in items missing |
| 06:36 | `7e8ff5a` | Verifier formally **REJECTS** retry 1 |
| 06:42 | `3d20569` | **Prior RCA committed** — H1 hypothesis: executor session ended after the easy refactors; remediation owner: executor; concrete commit plan with file:line landing-spots |
| 06:48 | `ddb1e3d` | Executor resumes — `feat(daemon): add g1_sources module with 4 production-shape G1Source impls` (+569 LOC) |
| 06:49 | `e382e57` | `docs(daemon): wire WO-0049 preamble + 5 g1_sources re-exports in lib.rs` |
| 06:51 | `0f04589` | `feat(daemon): wire find_references MCP tool + handle_find_references handler` |
| 06:53 | `342bc16` | `refactor(daemon): G1SourceFactory closure-based interior + from_builder` |
| 06:56 | `caa12a6` | `test(daemon): three module-root acceptance tests for find_references` |
| 06:57 | `70b82bc` | `test(verify): add scripts/verify/P2-W7-F05.sh` |
| 07:00 | `5a59c3a` | `chore(daemon): remove .expect literal from executor.rs invariant comment` |
| 07:02 | `54fccce` | `chore(work-order): WO-0049 ready for review (retry 2)` |
| 07:14 | `063d2e6` | Critic verdict **CLEAN** (retry 2 review): "feature deliverable now present and substantive; +1,458 / −5 across 6 files; verifier may proceed" |
| 07:36 | `9b596ed` | Verifier session `vrf-d249db74-…` flips `P2-W7-F05.passes` to `true` after running 35 acceptance criteria green from a clean slate; `last_verified_commit: 54fccce0…` |

**Current `feature-list.json` state for `P2-W7-F05`**:
```json
{
  "id": "P2-W7-F05",
  "passes": true,
  "last_verified_by": "verifier-d249db74-379b-468b-b88d-5ac3141992df",
  "last_verified_commit": "54fccce0f80b303448d1eb5c229b94c5385a8424",
  "attempts": 1
}
```

**The rejection record at `ucil-build/rejections/WO-0049.md` (frontmatter
`retry: 1`, rejected_at `2026-05-05T01:03:38Z`) is the historical retry-1
artifact, not a fresh rejection.** The orchestrator appears to have
re-spawned this root-cause-finder against the residual-on-disk rejection
file even though the verifier has subsequently flipped the feature green.
There is nothing to remediate.

## Confirmation that the prior RCA's H1 hypothesis was correct

The prior RCA's H1 (90% confidence: "executor session terminated after
landing scope_in steps 7 and 8 — the smallest, safest refactors at the
bottom of the scope_in list — without continuing on to the headline body
of work") is now **confirmed**:

- The executor's retry-2 run reproduced the 9-step commit plan from the
  prior RCA almost verbatim (compare `ucil-build/verification-reports/root-cause-WO-0049.md`
  in `3d20569` against the actual commits `ddb1e3d` → `54fccce`):
  - Prior RCA commit 2 plan: "Add `g1_sources.rs` skeleton + `G1SourceFactory` + `KgG1Source` (~120 LOC)" → actual `ddb1e3d` shipped the full 569-LOC g1_sources.rs in a single commit (deviation: 4 G1Source impls bundled into one commit instead of split across 3, but DEC-0005 module-coherence permits it).
  - Prior RCA commit 4 plan: "Wire `mod g1_sources` + 5 re-exports + lib.rs preamble (~10 LOC)" → actual `e382e57` shipped exactly 14 LOC across `lib.rs`.
  - Prior RCA commit 5 plan: "Extend `McpServer` + add `find_references` route (~80 LOC)" → actual `0f04589` shipped the route + handler.
  - Prior RCA commit 6 plan: "`handle_find_references` method on `McpServer` (~80 LOC)" → folded into `0f04589` (deviation: bundled with the route).
  - Prior RCA commit 7 plan: "Three module-root acceptance tests (~300 LOC)" → actual `caa12a6` shipped exactly that.
  - Prior RCA commit 8 plan: "`scripts/verify/P2-W7-F05.sh`" → actual `70b82bc` shipped the script.
  - Prior RCA commit 9 plan: "`ucil-build/work-orders/0049-ready-for-review.md`" → actual `54fccce` shipped the marker.
- The prior RCA's H2 (the `KnowledgeGraph::find_references_by_name` /
  `references` table naming defect) **was load-bearing in the actual
  retry-2 run**: the executor's `KgG1Source::execute` rustdoc (now in
  `crates/ucil-daemon/src/g1_sources.rs`) documents the divergence
  exactly as the prior RCA recommended ("use `resolve_symbol` +
  `list_relations_by_target`, not the spec's literal name") rather than
  silently inventing a new method or escalating for an ADR. The
  in-rustdoc divergence note was the correct lightest-touch remediation.
- The prior RCA's H3 (`G1FusionEntry` vs `G1FusedEntry` confusion) did
  not surface as a defect — the executor consumed both types correctly.

## What the prior RCA got slightly wrong

The prior RCA estimated "H1 confidence 90%" with the falsification
fall-back of "if the executor stops AGAIN at a partial commit, escalate
to Bucket E and split WO into 3 sub-WOs". The retry-2 executor session
did NOT stop at a partial commit — it pushed all 8 commits in 14
minutes (06:48 → 07:02), suggesting the original session-ending event
on retry-0 was either a turn-budget exhaustion or a session-ending side
effect, not a token-budget exhaustion (the work fit easily inside one
session when re-attempted).

The prior RCA's "If the H1 hypothesis is wrong" planner-rescope
recommendation is therefore not needed — H1 was correct as stated, and
the WO size (~740 LOC) is fine for a single executor session in
practice.

## Lessons captured (for the planner / harness, not the executor)

1. **Stale-rejection re-spawning**: when a verifier flips a feature to
   `passes: true`, the rejection file from the prior retry should
   either be moved to an archive subdirectory or annotated with a
   trailing `## Resolution` block, so the next root-cause-finder
   invocation does not re-run on stale state. Current behavior:
   `ucil-build/rejections/WO-0049.md` still sits in the active folder
   with no resolution marker even though the underlying feature has
   shipped. Recommend: harness post-flip step appends a one-liner
   `Resolved by verifier session vrf-d249db74-… at 9b596ed` to the
   rejection file when the matching feature flips. Bucket-B fix in
   `scripts/verify/post-flip-cleanup.sh` (does not exist yet); not
   required for any feature to land but reduces wasted RCA cycles.

2. **The "easy-refactor warm-up" antipattern**: the executor on the
   first attempt landed the two smallest items (steps 7 and 8) and
   stopped. A future planner-side guard could refuse to emit a
   work-order whose scope_in begins with the trivial items unless
   they are interleaved with substantive items, OR could emit a
   `min_diff_lines` hint that the executor's session-end self-check
   reads ("you have 25 lines of diff against an estimated 740-LOC
   work-order; do not declare the WO ready for review yet"). Not
   urgent — retry-2 succeeded — but worth noting for WO-size budgeting.

## No further remediation required

- ✅ `feature-list.json` flip persisted at `9b596ed`.
- ✅ Branch is up-to-date with origin (`git rev-parse HEAD == @{u}`).
- ✅ Working tree clean (`git status --porcelain` empty in `/home/rishidarkdevil/Desktop/ucil`).
- 🟡 The branch `feat/WO-0049-find-references-and-g1-source-production-wiring`
  itself has not been merged into `main` per the WO-0048 precedent
  (`19a4a1d merge: WO-0048 g1-result-fusion (feat → main)`), but this
  is the orchestrator's next step (not the root-cause-finder's). Nothing
  to RCA.
- 🟡 The retry-1 rejection record at `ucil-build/rejections/WO-0049.md`
  remains in the active folder. If the harness's stale-detection
  catches this and triggers further RCA spawns, this report itself
  is the resolution: there is no current failure for `P2-W7-F05`.

## Cross-references

- Verifier flip: `9b596ed` (`verify(daemon): WO-0049 PASS — flip P2-W7-F05 …`)
- Critic CLEAN verdict: `063d2e6` → `ucil-build/critic-reports/WO-0049.md` (verdict CLEAN, session `crt-WO-0049-2026-05-05-retry1-9b4f3e21`)
- Verifier retry-1 rejection (HISTORIC): `7e8ff5a` → `ucil-build/rejections/WO-0049.md` (verdict REJECT, session `vrf-162f0721-…`, rejected_at `2026-05-05T01:03:38Z`)
- Critic retry-0 BLOCKED verdict (HISTORIC): `6955d19`
- Prior RCA (HISTORIC, this file's predecessor): `3d20569` `chore(rca): WO-0049 root-cause — executor under-delivered, 14 of 16 scope_in items missing`
- Branch HEAD on remediation completion: `54fccce0f80b303448d1eb5c229b94c5385a8424`
- Work-order: `ucil-build/work-orders/0049-find-references-and-g1-source-production-wiring.json`
- Feature: `ucil-build/feature-list.json` entry `P2-W7-F05` (`passes: true`, `attempts: 1`, `last_verified_commit: 54fccce0…`)
- Foundational refactors (preserved through the remediation):
  - `crates/ucil-daemon/src/executor.rs` — `#[tracing::instrument(name = "ucil.group.structural.source", …)]` on `run_g1_source` (closes WO-0047 lessons line 296).
  - `crates/ucil-daemon/src/executor.rs` — `flatten() + debug_assert_eq!` replacement of the `.expect(...)` (closes WO-0047 lessons line 308).
- Remediation deliverables (all landed):
  - `crates/ucil-daemon/src/g1_sources.rs` — 569 LOC NEW file: 4 `G1Source` impls (`KgG1Source`, `SerenaG1Source`, `AstGrepG1Source`, `DiagnosticsG1Source`) + `G1SourceFactory`.
  - `crates/ucil-daemon/src/server.rs` — `pub g1_sources: Option<Arc<G1SourceFactory>>` field on `McpServer`, `with_g1_sources` builder, `find_references` route in `handle_tools_call`, `handle_find_references` async method, 3 module-root acceptance tests (`test_find_references_tool`, `test_find_references_tool_unknown_symbol`, `test_find_references_tool_missing_name_param`).
  - `crates/ucil-daemon/src/lib.rs` — `mod g1_sources;` + 5-symbol re-export line + WO-0049 preamble sentence.
  - `scripts/verify/P2-W7-F05.sh` — 109 LOC verify script.
  - `ucil-build/work-orders/0049-find-references-and-g1-source-production-wiring-ready-for-review.md` — 123 LOC ready-for-review marker.

**Total remediation diff**: +1,458 / −5 across 6 files; 8 commits.

## Closing note for the orchestrator

This RCA does **not** require executor action. The feature has shipped
and the next harness step is the per-WO `merge: WO-0049 …` commit on
`main` (executed by the verifier or the gate-check workflow, not the
root-cause-finder). If a fresh failure surfaces — e.g. a phase-gate
regression on `P2-W7-F05` discovered after a downstream WO breaks
something — the harness should write a NEW rejection record under
`ucil-build/rejections/WO-0049-retry-3.md` (or similar) with a fresh
`rejected_at` timestamp and re-spawn the root-cause-finder against
THAT, not against the historic retry-1 record.
