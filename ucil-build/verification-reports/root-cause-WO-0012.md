---
analyst_session: rca-wo0012-2026-04-17-a
work_order: WO-0012
branch: feat/WO-0012-kg-crud-and-hot-staging
head_commit: 3887fe5ec627b3508757291800ec623fb47155a4
features: [P1-W4-F02, P1-W4-F08]
prior_attempts: 1
analysed_at: 2026-04-17T13:00:00Z
remediation_owner: executor
confidence: 0.95
---

# Root Cause Analysis: WO-0012 (kg CRUD + bi-temporal + hot-staging)

**Analyst session**: `rca-wo0012-2026-04-17-a`
**Work-order**: `WO-0012` — `ucil-build/work-orders/0012-kg-crud-and-hot-staging.json`
**Features on the hook**: `P1-W4-F02` (CRUD + bi-temporal), `P1-W4-F08` (hot-staging)
**Branch**: `feat/WO-0012-kg-crud-and-hot-staging` (local only — never pushed)
**HEAD**: `3887fe5` == `main` (zero commits past merge-base)
**Attempts before this RCA**: 1 (1 verifier rejection + 1 critic BLOCKED)

## Failure pattern

**Single-rejection, single-mode**: the executor produced **zero commits** and
**zero `.rs` diff** against `main`.  The only surviving artefact in the
worktree is one **uncommitted** 6-line edit to the workspace-root
`Cargo.toml` that adds `chrono` to `[workspace.dependencies]` without any
consumer crate wiring it in.  No `ucil-build/work-orders/0012-ready-for-review.md`
was written, so the critic was spawned against a pre-implementation state
and correctly returned `BLOCKED`; the verifier then correctly returned
`REJECT` (retry 1).

Evidence (re-confirmed by this analyst from the worktree itself):

```
$ cd /home/rishidarkdevil/Desktop/ucil-wt/WO-0012
$ git log --oneline main..HEAD
(empty — 0 commits)
$ git status --porcelain
 M Cargo.toml
$ git diff main -- 'crates/**/*.rs' | wc -l
0
$ git diff main -- Cargo.lock | wc -l
0
$ git reflog -3
3887fe5 HEAD@{0}: reset: moving to HEAD
3887fe5 HEAD@{1}: (branch creation from main)
$ grep -RIn 'test_hot_staging_writes|test_upsert_and_get_entity|stage_hot_observation|checkpoint_wal' \
    crates/ucil-core/src/ | wc -l
0
$ ls ucil-build/work-orders/ | grep 0012
0012-kg-crud-and-hot-staging.json   # envelope only; no ready-for-review marker
```

Cross-check: `cargo build --workspace` on this exact worktree succeeds
cleanly (3.15 s warm).  That matters — it proves there is **no technical
blocker** (no missing precondition, no broken tooling, no compile-break
on `main`, no network dep, no `Cargo.lock` churn).  The environment is
healthy.  The executor simply did not do the work.

The critic already pinned this verdict in
`ucil-build/critic-reports/WO-0012.md` (5 blockers, 2 warnings,
6 trivial-passes).  The verifier confirmed in
`ucil-build/rejections/WO-0012.md` ("empty branch, no implementation").
Both agents named the same root fact; this RCA exists to explain **why**
and to give the retry executor a concrete step-by-step plan so the next
attempt does not repeat the pattern.

## Root cause (hypothesis, 95 % confidence)

**Session-abandonment by the executor.**  The prior executor session
opened the WO envelope, correctly identified that `chrono` would be
needed for `get_entity_as_of(at: chrono::DateTime<Utc>)`, edited the
workspace-root `Cargo.toml` to add the dep, and then ended the session
**without implementing any of the 12 `scope_in` bullets and without
committing the one change it did make**.

Supporting evidence for the hypothesis:

1. **Shape of the single change** — a workspace-dep add is a
   *preparation* step, not a *destination* step.  It is what a freshly-
   booted executor does first before touching the real module.  There
   is no partial `Entity` struct, no partial test, no scrap comment —
   consistent with "opened file, added dep, then stopped" rather than
   "mid-implementation crash".  A crash would typically leave a
   half-edited `knowledge_graph.rs`.
2. **Git reflog** shows one `reset: moving to HEAD` and the branch
   creation, nothing else — the branch HEAD has never moved past
   `main`.  A genuinely-stuck executor that had tried commits and
   rolled them back would leave reflog traces (commits, amends,
   resets to non-HEAD SHAs).  This reflog is pristine.
3. **No escalation file exists** for WO-0012 (`ls
   ucil-build/escalations/ | grep 0012` = empty).  A truly-blocked
   executor should have written one per root-`CLAUDE.md` §Escalation
   triggers.  Neither "I don't know how" nor "upstream broken" nor
   "spec contradicts WO-0011" was filed.
4. **`cargo build --workspace` is green right now** on the untouched
   worktree.  The `chrono` dep is orphan (no crate consumes it) so the
   build does not pull it; but more importantly there is no compile
   error, no missing type, no API mismatch that could have prevented
   the executor from laying down a single `Entity` struct.
5. **WO is unambiguous**: 12 `scope_in` bullets, 7 named tests, 8
   acceptance commands, 10 `scope_out` guardrails, 4 `lessons_applied`
   entries, and a reference implementation of the same pattern sitting
   right next door (`ucil-build/work-orders/0011-ready-for-review.md`,
   already merged at `c42cd94`/`f0683d1`).  There is no spec hole to
   freeze on.
6. **Similar pattern in prior escalations** — the project already has
   `ucil-build/escalations/20260415-0220-wo-WO-0003-attempts-exhausted.md`
   and `20260415-1856-wo-WO-0006-attempts-exhausted.md` on file, which
   (per their frontmatter) were also caused by executor-behavior
   failures rather than technical blockers.  Same failure mode, different
   WOs.  This is a known recurring risk class in the current harness.

Alternative hypotheses considered and rejected:

- **(5 %) Executor OOM or hard timeout before first commit.**  The
  reflog has no half-commits, the worktree has no scratch files, the
  single `Cargo.toml` edit is complete (valid TOML + sensible version
  string + accurate leading comment) — inconsistent with an
  interrupt-mid-write.  More importantly, an OOM/timeout should have
  tripped the Stop-hook which refuses to end a turn with a dirty tree
  OR a branch ahead of upstream (root `CLAUDE.md` §Commit+push
  cadence); the fact that the session ended with a dirty tree and a
  branch that was NOT ahead of upstream means either the Stop-hook was
  bypassed or the executor exited before the hook ran — both consistent
  with "executor decided it was done and stopped" rather than a crash.
- **(<5 %) Spec ambiguity.**  The WO envelope is the most detailed in
  the entire `work-orders/` directory to date; it cites master-plan
  §12.1 lines 1127-1318, §11 lines 1108-1117, §18 Phase-1 lines
  1749-1754, DEC-0005, DEC-0007, and the prior-WO `0011-ready-for-
  review.md`.  The `entities` table shape, the `UNIQUE(qualified_name,
  file_path, t_valid_from)` constraint, the BEGIN IMMEDIATE chokepoint,
  and the exact test names are all spelled out.  No ADR is needed.
- **(<5 %) Environment problem.**  `cargo build --workspace` succeeds
  (verified by this analyst on the worktree, 2026-04-17T13:00Z).
  `grep -n execute_in_transaction crates/ucil-core/src/knowledge_graph.rs`
  returns hits on lines 15, 31, 409, 430, 436, 548, 550 — the
  reusable helper WO-0011 established is in place.

## Remediation

**Who**: `executor`
**Where**: worktree `/home/rishidarkdevil/Desktop/ucil-wt/WO-0012` on
branch `feat/WO-0012-kg-crud-and-hot-staging`.
**What**: carry out the 12 `scope_in` bullets in
`ucil-build/work-orders/0012-kg-crud-and-hot-staging.json:14-25` using
the commit plan below.  Push after every commit.  Do not flip
`passes`.

The single existing `M Cargo.toml` edit is **fine to keep** — fold it
into commit #1 below rather than discarding it.  (Discarding would be
wasteful; the decision to promote `chrono` to a workspace dep is
correct and matches how `uuid` is already organised at
`Cargo.toml:69-70`.)  The only thing missing from that edit is the
consumer-crate line `chrono = { workspace = true }` in
`crates/ucil-core/Cargo.toml` — commit #1 adds it.

### Commit plan (~8 commits, per WO envelope `estimated_commits: 8`)

| # | Type+scope | Subject | Target lines | Files |
|---|------------|---------|-------------:|-------|
| 1 | `build(workspace)` | add `chrono` workspace dep + wire into `ucil-core` | ~10 | `Cargo.toml`, `crates/ucil-core/Cargo.toml` |
| 2 | `feat(core)` | add `Entity` + `Relation` structs with serde derives, re-export from `lib.rs` | ~90 | `crates/ucil-core/src/knowledge_graph.rs`, `crates/ucil-core/src/lib.rs` |
| 3 | `feat(core)` | `upsert_entity` + `get_entity_by_qualified_name` + `list_entities_by_file` + 3 module-root tests | ~130 | `crates/ucil-core/src/knowledge_graph.rs` |
| 4 | `feat(core)` | `upsert_relation` + `list_relations_by_source` + 1 module-root test | ~70 | `crates/ucil-core/src/knowledge_graph.rs` |
| 5 | `feat(core)` | bi-temporal `get_entity_as_of` + 1 module-root test | ~50 | `crates/ucil-core/src/knowledge_graph.rs` |
| 6 | `feat(core)` | `HotObservation` + `stage_hot_observation` (BEGIN IMMEDIATE) + 1 module-root test — **frozen selector `test_hot_staging_writes`** | ~55 | `crates/ucil-core/src/knowledge_graph.rs`, re-export in `lib.rs` |
| 7 | `feat(core)` | `stage_hot_convention_signal` + `stage_hot_architecture_delta` (no dedicated tests required; mutation oracle + grep guard suffice) | ~45 | `crates/ucil-core/src/knowledge_graph.rs` |
| 8 | `feat(core)` | `WalCheckpointMode` enum + `checkpoint_wal` + 1 module-root test — **`test_wal_checkpoint_truncates`** | ~40 | `crates/ucil-core/src/knowledge_graph.rs`, re-export in `lib.rs` |

Target diff total ≈ 490 lines, matching the WO envelope's
`estimated_diff_lines: 450` within ~10 %.  Commit-style:
Conventional-Commits with `Phase: 1`, `Feature: P1-W4-F02` (or `-F08`
on commits 6 and 8), `Work-order: WO-0012` trailers — see
`.claude/rules/commit-style.md`.

### Non-negotiable invariants for the retry

Pulled from the WO envelope + the two frozen selectors — the retry
executor must not violate any of these or the verifier will reject
again:

1. **Module-root tests only.**  Every test below lives at module
   root as `#[cfg(test)] #[test] fn test_<name>() { ... }`, **NOT**
   inside `mod tests { ... }`.  The frozen nextest selector
   `knowledge_graph::test_hot_staging_writes` expects the module-root
   form, matching the `test_schema_creation` pattern WO-0011 already
   established at `crates/ucil-core/src/knowledge_graph.rs:472-474`.
   This is repeatedly the WO-0006/WO-0007 class of verifier reject —
   do not regress it.  See
   `ucil-build/decisions/DEC-0005-WO-0006-module-coherence-commits.md`
   and `scope_in` line 23 of the WO envelope.

   Required test names (exact, NO mod-tests prefix):

   - `test_upsert_and_get_entity`
   - `test_bi_temporal_as_of`
   - `test_list_entities_by_file`
   - `test_upsert_relation_and_list`
   - `test_entity_unique_constraint_updates`
   - `test_hot_staging_writes`    ← **F08 selector; frozen**
   - `test_wal_checkpoint_truncates`

   Plus the pre-existing `test_schema_creation` stays untouched at
   line 474; the new ≥6-passing test count on `knowledge_graph::`
   (F02) is satisfied once 6 of the 7 new ones compile and pass.

2. **INIT_SQL is byte-for-byte frozen.**  `scope_out` line 31:
   *"No changes to WO-0011 schema DDL — INIT_SQL stays byte-for-byte
   identical. If a new column or table is needed, STOP and write an
   ADR."*  The schema at `crates/ucil-core/src/knowledge_graph.rs:110-305`
   already has every column and every table WO-0012 needs — cross-
   reference:

   - `entities.qualified_name`, `entities.file_path`, `entities.t_valid_from`,
     `entities.t_valid_to`, `entities.t_last_verified`, `entities.access_count`,
     `entities.importance`, `entities.source_tool`, `entities.source_hash`,
     `UNIQUE(qualified_name, file_path, t_valid_from)` — all present at
     `knowledge_graph.rs:111-126`.
   - `relations.source_id`, `relations.target_id`, `relations.kind`,
     `relations.weight`, `relations.t_valid_from`, `relations.t_valid_to`,
     `relations.source_tool`, `relations.confidence` — all present at
     `knowledge_graph.rs:128-137`.
   - `hot_observations.raw_text`, `.session_id`, `.related_file`,
     `.related_symbol` — all present at `knowledge_graph.rs:190-198`.
   - `hot_convention_signals` at `knowledge_graph.rs:200-207`;
     `hot_architecture_deltas` at `knowledge_graph.rs:209-216`.

   Do NOT add columns.  Do NOT add indexes.  Do NOT re-run the schema
   DDL from the test — just call `KnowledgeGraph::open(tempdir)`.

3. **Route every writer through `execute_in_transaction`.**  The helper
   at `crates/ucil-core/src/knowledge_graph.rs:430-440` already uses
   `TransactionBehavior::Immediate`, so `upsert_entity`,
   `upsert_relation`, `stage_hot_observation`,
   `stage_hot_convention_signal`, `stage_hot_architecture_delta` must
   all go through it — do NOT call `self.conn.execute(...)` directly.
   `get_entity_by_qualified_name`, `get_entity_as_of`,
   `list_entities_by_file`, `list_relations_by_source` are reads and
   can use `self.conn` via `.prepare` + `.query_map` — no transaction
   needed.

4. **`upsert_entity` must honour `ON CONFLICT DO UPDATE`.**  The WO
   scope_in line 15: the UNIQUE constraint is
   `(qualified_name, file_path, t_valid_from)` — see
   `knowledge_graph.rs:125`.  The ON CONFLICT branch must set
   `t_last_verified = datetime('now')` **and** increment `access_count`
   (`access_count = access_count + 1`), returning the existing row's
   `id`.  SQLite's `INSERT ... RETURNING id` on both the insert and
   conflict branches gives the caller the inserted-or-updated row
   without a second SELECT.

5. **`get_entity_as_of` time comparison format.**  The bi-temporal
   `WHERE t_valid_from <= ?1 AND (t_valid_to IS NULL OR t_valid_to > ?1)`
   comparison is a **string comparison in SQLite** (TEXT-typed
   columns).  Pick ONE timestamp format and use it everywhere:

   - Recommended: `chrono::DateTime<Utc>::to_rfc3339()` →
     `"2026-04-17T12:00:00+00:00"` — lexicographically sortable,
     unambiguous.
   - Tests that write an `Entity` should set `t_valid_from` to
     `chrono::Utc::now().to_rfc3339()` (or a fixed test value like
     `"2026-04-17T00:00:00+00:00"`).

   Do NOT mix `datetime('now')` (which produces
   `"2026-04-17 12:00:00"`, space separator) with RFC-3339
   (`T` separator, offset suffix) on the *same* column used in a
   range query — SQLite will string-compare these incorrectly.  The
   existing `t_ingested_at DEFAULT (datetime('now'))` is fine
   *because `t_ingested_at` is never range-queried in this WO*.

6. **Re-exports.**  `crates/ucil-core/src/lib.rs` line 22 currently
   re-exports only `KnowledgeGraph, KnowledgeGraphError`.  Add
   `Entity, Relation, HotObservation, WalCheckpointMode` per the WO
   `scope_in` line 25.

7. **Clippy pedantic + nursery are in effect.**  Line 7 of
   `crates/ucil-core/src/lib.rs`: `#![warn(clippy::all,
   clippy::pedantic, clippy::nursery)]` — WO-0011 already had to land
   commit `e29b927` to clear them.  Expect to hit `doc_markdown`,
   `missing_const_for_fn`, `missing_errors_doc`, `must_use_candidate`,
   and `option_if_let_else` on this WO's additions.  Fix, do not
   blanket-allow.

8. **No mocking of `rusqlite::Connection`.**  Tests open a real
   tempfile-backed SQLite db via `tempfile::TempDir::new()` then
   `KnowledgeGraph::open(&tempdir.path().join("k.db"))` — same
   pattern as `test_schema_creation` at
   `knowledge_graph.rs:499-503`.  `tempfile` is already a `dev-
   dependency` at `crates/ucil-core/Cargo.toml:27`, so no dep add is
   needed.

### Acceptance (from the WO, unchanged)

1. `cargo nextest run -p ucil-core 'knowledge_graph::'` → ≥ 6 new
   passing tests + the pre-existing `test_schema_creation`.
2. `cargo nextest run -p ucil-core knowledge_graph::test_hot_staging_writes`
   → exactly 1 test run, 1 passed.
3. `cargo build --workspace` → exit 0.
4. `cargo clippy -p ucil-core -- -D warnings` → exit 0 (pedantic +
   nursery clean).
5. `cargo doc -p ucil-core --no-deps` → exit 0, no broken intra-doc
   links.
6. `scripts/reality-check.sh P1-W4-F02` and `scripts/reality-check.sh
   P1-W4-F08` — stashing the new knowledge_graph.rs contents must
   make the F02/F08 tests fail; popping must make them pass.
7. `grep -rn '#\[ignore\]\|todo!\|unimplemented!' crates/ucil-core/src/knowledge_graph.rs`
   → 0 lines.
8. No mocks of `rusqlite::Connection` anywhere in the diff.

### Risk / known pitfalls for the retry

1. **If commit #1 accidentally drops the existing uncommitted
   `Cargo.toml` edit**, commit #3 / commit #5 / commit #8 will not
   compile because `chrono::DateTime<Utc>` will be unresolved.  Stage
   the existing `Cargo.toml` diff first (`git add Cargo.toml`), then
   additionally stage `crates/ucil-core/Cargo.toml` with the
   `chrono = { workspace = true }` line, then commit both in commit #1.
2. **If tests end up inside `#[cfg(test)] mod tests { ... }`** the
   frozen selector `knowledge_graph::test_hot_staging_writes` will
   resolve to `knowledge_graph::tests::test_hot_staging_writes`
   instead, acceptance criterion #2 will fail, verifier will REJECT.
   This is the exact WO-0006 / WO-0007 failure mode DEC-0005
   catalogued — do not regress it.
3. **If `upsert_entity` does not return the row id on the conflict
   branch**, `test_entity_unique_constraint_updates` (which is meant
   to insert twice with the same `(qualified_name, file_path,
   t_valid_from)` triple and assert the second call returns the same
   id + increments `access_count`) will flake.  Use `INSERT ...
   ON CONFLICT(qualified_name, file_path, t_valid_from) DO UPDATE SET
   t_last_verified = datetime('now'), access_count = access_count +
   1 RETURNING id`.
4. **If `stage_hot_observation` writes through `self.conn.execute`
   rather than `self.execute_in_transaction`**, the Stop-hook's diff
   audit (root `CLAUDE.md` §Anti-laziness: "All shell invocations use
   `tokio::process::Command`"... wait, that's async; for sync SQLite
   the corresponding invariant is master-plan §11 line 1117 BEGIN
   IMMEDIATE) will not fire, but the `test_hot_staging_writes` test
   can still pass by accident under single-writer contention — the
   mutation-oracle reality-check *won't* catch it, and the critic's
   §2 will flag it as a design regression.  Always go through
   `execute_in_transaction`.
5. **If the `chrono` default-features are kept on**, the `oldtime`
   transitive dep and its `time 0.1` chain get pulled in, which
   emits clippy-deny warnings on some Rust versions.  The uncommitted
   edit already has `default-features = false, features = ["clock",
   "serde", "std"]` — keep that.

## If this hypothesis is wrong

If the retry executor finds a concrete blocker the moment it starts —
e.g. `chrono = { workspace = true }` fails to resolve in
`crates/ucil-core/Cargo.toml`, or `rusqlite::params!` macro breaks on
`chrono::DateTime<Utc>`, or the tempfile crate is missing — STOP and
write `ucil-build/escalations/YYYYMMDD-HHMM-wo-WO-0012-precondition-broken.md`
rather than grinding.  The analyst's 95 % confidence in "session-
abandonment" is staked on `cargo build --workspace` being green
*right now*.  If that changes between this report and the retry, the
hypothesis is wrong and a fresh RCA is warranted.

Fallback investigation if the retry also fails:

- Enable `tracing::debug!` in the executor's session-loop transcript
  (orchestrator logs at `ucil-build/phase-log/01-phase-1/session-*.jsonl`)
  to see if the executor is mis-reading the WO envelope (e.g.
  confusing the frozen F08 selector with a `mod tests` wrapper).
- Verify the Stop-hook's dirty-tree / ahead-of-upstream checks
  actually ran on the prior session — if they didn't, that's a
  harness bug, not an executor bug, and a separate Bucket-B
  triage fix is warranted.

## Next step for the orchestrator

Route this report to the executor as supplementary context for the
next retry on `feat/WO-0012-kg-crud-and-hot-staging`.  The executor
must:

1. Start in the worktree `/home/rishidarkdevil/Desktop/ucil-wt/WO-0012`.
2. Run commit #1 first (chrono dep + consumer line).  Push.
3. Implement commits #2-#8 per the table above, pushing after each.
4. Write `ucil-build/work-orders/0012-ready-for-review.md` mirroring
   `0011-ready-for-review.md`'s shape.
5. Do NOT flip `passes` in `feature-list.json` — verifier only.
6. The critic will re-run against a non-empty range, and the verifier
   will re-run from this rejection point.  The WO's `attempts`
   counter will be bumped by the next successful verifier pass (per
   the rejection report's §Feature attempts counters note).
