# WO-0052 Ready for review (retry 2)

- **Work-order**: WO-0052 (P2-W7-F04 — session-scoped deduplication)
- **Branch**: `feat/WO-0052-session-dedup`
- **Final commit**: `36b1a3fbaa55e38216789198213624fa3d42f7cb`
- **Retry**: 2
- **Carries**: 7 commits since `main` (original 5 + retry-2's `fix(daemon)` + retry-2's `docs(adr)`)

## Substantive contribution

`crates/ucil-daemon/src/session_manager.rs` — same content the
retry-1 verifier confirmed PASS on AC01–AC10 + AC12 + AC14–AC19 at
HEAD `901c8b5`:

- `pub async fn dedup_against_context(&self, id: &SessionId, candidates: Vec<PathBuf>) -> Vec<PathBuf>`
  (line 306) — read-locks `sessions`, filters `candidates` against
  `info.files_in_context`; on missing-session returns `candidates`
  unchanged (structural realisation of the §5.2 line 459
  "session-scoped dedup store cleared on session expiry" invariant
  via the existing `purge_expired` retain).
- `pub async fn add_files_to_context(&self, id: &SessionId, files: &[PathBuf]) -> Option<()>`
  (line 358) — single write-lock bulk insert via
  `BTreeSet::extend(files.iter().cloned())`.
- `#[tokio::test] async fn test_session_dedup()` at module root
  (line 646) per DEC-0007 — 5 sub-assertions exercising
  empty-context / partial-overlap / bulk-add-accumulation /
  full-overlap / session-expiry-purges-dedup-state. Each
  assertion message quotes the actual observed `Vec<PathBuf>` via
  `{:?}` per WO-0051 lessons.
- 3 pre-baked function-body mutations (M1: filter→pass-through,
  M2: filter→`Vec::new()`, M3: extend→`Some(())`-no-op) each fire
  the matching sub-assertion panic when applied; restore
  via `git checkout`.

## Retry-2 additions

Retry-1 was rejected on a single criterion — **AC11**
(`cargo doc -p ucil-daemon --no-deps`) — which surfaced 5
broken/private intra-doc links in `crates/ucil-daemon/src/executor.rs`
inherited from WO-0048's `feat(daemon): add fuse_g1 + authority_rank`
commit. The errors pre-existed on `main` and on every prior WO branch
since `67fd2eb`. WO-0048's verifier did not run `cargo doc`;
WO-0049/0050/0051 did not either; WO-0052's planner was the first to
add AC11.

The root-cause-finder
(`ucil-build/verification-reports/root-cause-WO-0052.md`) recommended
a Bucket-D micro-WO targeting `executor.rs` as the preferred path,
with the alternative that "the executor can rebase WO-0052 onto the
post-fix main and submit a fresh ready-for-review marker". On retry-2
spawn no upstream fix was on `main` and no Bucket-D micro-WO had been
emitted, so the user explicitly authorised bundling the RCF's exact
prescribed fix into this branch.

DEC-0013 records the carve-out and pins the fix to the RCF's
prescribed plain-backticks form (no `pub` promotions, no API
surface change). Two new commits were added on retry 2:

| Commit    | Subject                                                                | LOC   |
|-----------|------------------------------------------------------------------------|-------|
| `032ebc0` | `fix(daemon): repair 5 broken intra-doc links in executor.rs`          | +5/-5 |
| `36b1a3f` | `docs(adr): DEC-0013 — WO-0052 bundles executor.rs rustdoc-link fix`   | +129  |

Final diff against `main`:

```
crates/ucil-daemon/src/executor.rs                                     (+5 / -5)
crates/ucil-daemon/src/session_manager.rs                              (+~250 lines: methods + test + module preamble)
ucil-build/decisions/DEC-0013-WO-0052-bundles-executor-rustdoc-fix.md  (new, +129)
ucil-build/work-orders/0052-ready-for-review.md                        (this file)
```

## Acceptance criteria — local re-run on retry-2 HEAD `36b1a3f`

| AC    | Result | Evidence |
|-------|--------|----------|
| AC01 (`dedup_against_context` signature)            | PASS | `grep -nE '^[[:space:]]*pub async fn dedup_against_context' …session_manager.rs` → line 306 |
| AC02 (`add_files_to_context` signature)             | PASS | line 358 |
| AC03 (test at module root, column 0)                | PASS | line 646 (un-indented) |
| AC04 (`session_manager::test_session_dedup` passes) | PASS | `1 passed; 0 failed; 125 filtered out` |
| AC05 (`test_session_state_tracking` regression)     | PASS | `1 passed` |
| AC06 (`session_manager::` full surface)             | PASS | `9 passed; 0 failed; 117 filtered out` |
| AC07 (`pub use session_manager::` unchanged)        | PASS | `lib.rs:137` unchanged |
| AC08 (`cargo test --workspace --no-fail-fast`)      | PASS | 0 `test result: FAILED` lines |
| AC09 (`cargo clippy -p ucil-daemon -- -D warnings`) | PASS | 0 errors / 0 warnings |
| AC10 (`cargo fmt --check`)                          | PASS | exit 0 |
| **AC11 (`cargo doc -p ucil-daemon --no-deps` clean)** | **PASS** | **0 errors, 0 warnings — was the retry-1 rejection cause; now fixed via the 5 plain-backticks edits in `032ebc0`** |
| AC12 (no `*.toml` changes)                          | PASS | `git diff --name-only main...HEAD -- '*.toml' \| wc -l` = 0 |
| AC13 (diff scope)                                   | RELAXED | by design under DEC-0013 — diff now contains executor.rs (rustdoc fix) + ADR alongside the F04 source + marker.  No behaviour-affecting drift.  AC12 (the bash subset of AC13) still PASSES. |
| AC14 (no `#[ignore]/todo!()/unimplemented!()`)       | PASS | 0 hits in `session_manager.rs` |
| AC15 (M1 mutation fires SA2)                         | PASS | retry-1 verifier confirmed; same code on retry-2 |
| AC16 (M2 mutation fires SA1)                         | PASS | retry-1 verifier confirmed; same code on retry-2 |
| AC17 (M3 mutation fires SA3)                         | PASS | retry-1 verifier confirmed; same code on retry-2 |
| AC18 (rustdoc cites `§6.3 line 666`)                 | PASS | `grep` hits 3 lines (preamble + 2 method docs) |
| AC19 (commit subject ≤70 chars)                      | SOFT-WARN | retry-1 had 2 subjects > 70 (44aa648 = 92, 50c0bdd = 84).  retry-2's two new commits are both ≤70 chars (`fix(daemon): repair 5 broken intra-doc links in executor.rs` = 60, `docs(adr): DEC-0013 — WO-0052 bundles executor.rs rustdoc-link fix` = 65). The retry-1 subjects are immutable post-push per CLAUDE.md "no `--amend` after push" rule.  Soft-target violation, not a hard structural error per `.claude/rules/commit-style.md`.  Same disposition as the retry-1 verifier's AC19 reading (rejection.md note A). |

## Mutation discipline — re-confirmed by retry-1 verifier (carries forward)

Retry-1's verifier ran the M1/M2/M3 mutation cycle by hand against
`session_manager.rs`'s `dedup_against_context` and
`add_files_to_context` bodies; each fired the expected
sub-assertion panic at the documented line/message
(rejection.md table at lines 165–168).  The mutations target
`session_manager.rs` only — retry-2's executor.rs edits do not
intersect them.

## Mismatch surfaces vs the WO

- **WO-0052 `forbidden_paths` line 99 (`crates/ucil-daemon/src/executor.rs`)**
  is structurally violated for the 5-line rustdoc fix. DEC-0013
  documents the carve-out and the user's retry-2 authorisation.
- **WO-0052 `scope_in[6]`** ("No `lib.rs` re-export changes") still
  holds — `lib.rs` is unchanged.
- **WO-0052 `scope_in[5]`** ("4 path strings in the test … no modification
  to fixtures / no TempDir") still holds — `tests/fixtures/**` untouched.

## Cargo doc — sample of the new clean output

```
$ cargo doc -p ucil-daemon --no-deps 2>&1 | grep -cE '^(error|warning):'
0
$ echo $?
0
```

(Compare retry-1's HEAD `901c8b5`: 6 errors, 0 warnings.)

## Operator note

The retry-2 verifier should:

1. `cd /home/rishidarkdevil/Desktop/ucil-wt/WO-0052`
2. `git fetch origin && git rev-parse HEAD` — expect `36b1a3f…`
3. `cargo clean && cargo test -p ucil-daemon session_manager::test_session_dedup` — expect `1 passed`
4. `cargo doc -p ucil-daemon --no-deps 2>&1 | grep -cE '^(error|warning):'` — expect `0`
5. Re-run AC15/AC16/AC17 manual mutations on `session_manager.rs`
   per the same procedure retry-1's verifier used — each should
   fire the documented sub-assertion panic.
6. Re-confirm AC19 disposition per retry-1's note A (soft-target
   violation, not hard fail).
7. If all green, flip `passes=true` for P2-W7-F04 via
   `scripts/flip-feature.sh`.
8. Optional: open a follow-up tracking issue / Bucket-D WO to
   verify the executor.rs fix is preserved through any future
   re-baselining of the daemon module.

— executor (retry 2), 2026-05-05
