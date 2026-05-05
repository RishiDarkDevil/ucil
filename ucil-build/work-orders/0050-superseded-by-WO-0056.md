# WO-0050 superseded by WO-0056

**Status**: superseded — abandoned with zero commits ahead of `main`
**Superseded at**: 2026-05-06T20:35:00Z
**Superseding WO**: WO-0056 (`ucil-build/work-orders/0056-g2-rrf-fusion-redo.json`)
**Superseded by**: planner

## What happened

WO-0050 (`g2-rrf-fusion`, target P2-W7-F03) was emitted on 2026-05-05T07:00:00Z and a worktree branch `feat/WO-0050-g2-rrf-fusion` was cut from `main`. The branch is byte-identical to `main` — no commits, no `0050-ready-for-review.md` was ever written, no `verification-reports/WO-0050.md` exists. The orchestrator moved past WO-0050 to ship WO-0051 → WO-0055 (P2-W7-F04 / F07 / F08 / F09 all flipped) without ever returning to F03.

Per the WO-0051 lessons-learned block (line 412 in `ucil-build/phase-log/02-phase-2/CLAUDE.md`), the WO-0051 verifier discovered a leftover stash labelled `wo-0051-vrf-restash-WO0050-leftovers` containing partial WO-0050 work (`crates/ucil-core/src/lib.rs` modified + `crates/ucil-core/src/fusion.rs` untracked). That stash bleed is the only artifact of any WO-0050 execution attempt; it was never committed and is now ancient under cross-worktree shared `.git/refs/stash`. The WO-0056 executor MUST start from a clean main and ignore any remaining stash entry — re-implement the F03 surface from the WO spec verbatim.

## Why supersede instead of resume

- The WO-0050 branch is at zero commits ahead of main, so there is no executor work to recover.
- The WO-0050 stash (per WO-0051 line 412) was ad-hoc and predates the current `main` (e.g. WO-0055 added `pub use scip::*` re-exports in `lib.rs`); replaying that stash on today's `main` would conflict.
- Drift counter `1` was set during the gap, then reset to `0` after WO-0055 closed; this is not a `/replan` situation but a clean re-emit.

## What WO-0056 changes

- Fresh `WO-0056` ID, fresh slug `g2-rrf-fusion-redo`, fresh branch `feat/WO-0056-g2-rrf-fusion-redo` so `feat/WO-0050-g2-rrf-fusion` stays as a historical artefact and the executor doesn't re-use the stale worktree.
- Plan content is **functionally identical** to WO-0050 (same RRF math, same 5 enum variants, same 7-sub-assertion test, same 3-mutation table) with the WO-0055 lessons-learned (workspace build precondition, three-dot diff allow-list now standing protocol for 8 consecutive WOs, `clippy::doc_markdown` pre-flight grep) layered on the executor + verifier guidance.
- Allow-list updated: `0056-ready-for-review.md` instead of `0050-ready-for-review.md`.

## Action for harness

- This file resolves any orchestrator-side reference to WO-0050 as the "next" pending WO. Future planner cycles should pick up WO-0056 instead.
- The local `feat/WO-0050-g2-rrf-fusion` branch can be left as-is; cleanup is non-blocking.

resolved: true
