# DEC-0013: WO-0052 bundles the executor.rs rustdoc-link fix

**Status**: accepted
**Date**: 2026-05-05
**Supersedes / superseded-by**: none

## Context

WO-0052 (P2-W7-F04, session-scoped deduplication) was retried after
verifier session `vrf-def33d94` rejected on AC11
(`cargo doc -p ucil-daemon --no-deps`). Root-cause analysis at
`ucil-build/verification-reports/root-cause-WO-0052.md` confirmed:

- AC11 fails on **5 inherited rustdoc errors** in
  `crates/ucil-daemon/src/executor.rs`, all originating from WO-0048
  (commit `67fd2eb feat(daemon): add fuse_g1 + authority_rank for G1
  result fusion`).
- WO-0048's verifier did not run `cargo doc`; WO-0049/0050/0051 did
  not either; WO-0052's planner is the first to add AC11, surfacing
  the latent breakage.
- WO-0052's substantive contribution
  (`crates/ucil-daemon/src/session_manager.rs` — `dedup_against_context`,
  `add_files_to_context`, `test_session_dedup`) is otherwise clean: AC01–
  AC10 + AC12 + AC14–AC19 all PASS, mutation discipline AC15/16/17 fires
  as designed, coverage 94.61% on the diff target / 89.88% crate aggregate.
- `crates/ucil-daemon/src/executor.rs` is in WO-0052's
  `forbidden_paths` (`ucil-build/work-orders/0052-session-dedup.json:99`).

The RCF lays out three resolution paths:

1. **Recommended** — emit a Bucket-D micro-WO with `feature_ids: []`
   targeting `executor.rs`, land it on `main`, then rebase WO-0052.
2. **Less-preferred fallback** — amend AC11 to grandfather `executor.rs`
   past the rustdoc check via ADR.
3. **Strongly-disfavoured fallback** — re-emit WO-0052 with `executor.rs`
   in `scope_in`.

## Decision

Bundle the 5 plain-backticks edits to `executor.rs` into the existing
`feat/WO-0052-session-dedup` branch, structurally selecting RCF
fallback (3) but applying the **exact concrete fix the RCF recommends
under path (1)** (5 link-syntax edits, 0 `pub` promotions, ≤10 LOC).

The user explicitly authorised this path on retry 2 ("Apply the RCF's
recommended remediation, … reuse the existing worktree").  The
authorisation effectively performs the planner action that path (1)
would otherwise require — re-scoping `executor.rs` from
`forbidden_paths` for this single retry.

## Rationale

**Why not path (1) Bucket-D micro-WO**: at retry-2 spawn time no
Bucket-D micro-WO had been emitted by the planner and no upstream
fix existed on `main`. Spinning up a separate micro-WO + executor +
critic + verifier + merge cycle just to land 5 link-syntax edits
would burn a full retry-cost iteration on rebasing `feat/WO-0052-…`
afterward, and the user has explicit visibility into the situation
through the rejection + RCF reports.

**Why not path (2) amend AC11**: papers over the underlying main
bug, leaves AC11 weaker for every subsequent ucil-daemon WO, and
violates the master plan's crate-wide
`#![deny(rustdoc::broken_intra_doc_links)]` discipline at
`crates/ucil-daemon/src/lib.rs:96`.

**Why path (3) is acceptable here despite RCF's "strongly
disfavoured" framing**: the RCF's disfavour was on the assumption
that the executor would re-derive WO-0048's symbol-visibility
rationale (and might erroneously promote private symbols to `pub`).
This decision pins the fix to the RCF-prescribed plain-backticks
form, eliminating that risk.  The five RCF edits are mechanical
text substitutions — no API surface change, no behaviour change.

## Edit list (frozen, matches RCF table at lines 128–132 of
`ucil-build/verification-reports/root-cause-WO-0052.md`)

| # | Location           | Before                                                       | After                                                       |
|---|--------------------|--------------------------------------------------------------|-------------------------------------------------------------|
| 1 | `executor.rs:952`  | `acceptance test [`test_g1_parallel_execution`] supplies`    | `acceptance test `test_g1_parallel_execution` supplies`     |
| 2 | `executor.rs:1073` | `Build one boxed future per source via [`run_g1_source`] and`| `Build one boxed future per source via `run_g1_source` and` |
| 3 | `executor.rs:1221` | `location are resolved by [`authority_rank`]`                | `location are resolved by `authority_rank``                 |
| 4 | `executor.rs:1230` | `higher-authority source per [`authority_rank`];`            | `higher-authority source per `authority_rank`;`             |
| 5 | `executor.rs:1326` | `source wins via [`authority_rank`] and a [`G1Conflict`]`    | `source wins via `authority_rank` and a [`G1Conflict`]`     |

Note edit #5 retains the working pub-link `[`G1Conflict`]` and
only de-links the private `authority_rank`.  No symbol is promoted
to `pub`; no API surface changes.

## Consequences

- WO-0052's diff now spans **two source files** under
  `crates/ucil-daemon/src/`:
  - `session_manager.rs` (the F04 implementation, 4 commits)
  - `executor.rs` (the rustdoc-link fix, 1 commit)
- AC13 of WO-0052 (verbal "diff returns ONLY session_manager.rs +
  marker") is intentionally relaxed for retry 2.  The bash-list
  AC at `acceptance_criteria` index 12 (no `*.toml` mutations)
  still passes.
- `cargo doc -p ucil-daemon --no-deps` now exits clean on every
  branch the fix carries to: `feat/WO-0052-session-dedup` and
  every subsequent ucil-daemon WO.  AC11 is no longer a latent
  cliff for future verifiers.
- Future bisects on the F04 dedup implementation are minimally
  affected: the executor.rs commit lands as a separate commit
  (`fix(daemon): repair 5 broken/private intra-doc links in
  executor.rs`) so `git bisect` over WO-0052's commits cleanly
  separates session-manager work from the gate fix.
- The next P2 work-order's `forbidden_paths` should NOT include
  `executor.rs` for any rustdoc-related scope, since it is now
  on a clean rustdoc footing.

## Revisit trigger

If a future audit reveals additional broken intra-doc links in
`executor.rs` introduced after this commit, write a separate
Bucket-D micro-WO per the original RCF preferred path — do not
bundle into the next feature WO.  This decision is a one-time
retry-2 carve-out, not a precedent for routine in-WO gate fixes.

## Cross-references

- `ucil-build/rejections/WO-0052.md` — verifier retry-1 rejection
- `ucil-build/verification-reports/root-cause-WO-0052.md` — RCF analysis
- `ucil-build/work-orders/0052-session-dedup.json` — the WO this
  retry serves; `forbidden_paths` line 99 lists `executor.rs`
- `ucil-build/decisions/DEC-0007-remove-cargo-mutants-per-wo-gate.md`
  — frozen-selector / pre-baked-mutation policy that AC15/16/17
  inherit from; unaffected by this decision
