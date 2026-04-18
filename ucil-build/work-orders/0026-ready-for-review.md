# WO-0026 ready for review

**Feature**: P1-W3-F02 — `ucil-daemon` file watcher (notify + debouncer, 100 ms, PostToolUse fast path)
**Branch**: `feat/WO-0026-file-watcher-notify-debounce`
**Final commit**: `345018c9a27a6678561787768f6c1bfe350a1137`
**Commits on branch (5)**:

```
345018c feat(daemon): watcher shutdown test closes the F02 contract
6311ec0 feat(daemon): watcher hook-bypass tests pin down §14 fast path
2cb5688 feat(daemon): watcher notify-path tests with real tempdir writes
f348b27 feat(daemon): add watcher module with FileEvent types + FileWatcher
2ea70c4 build(workspace): add notify + notify-debouncer-full workspace deps
```

## What I verified locally (fresh worktree, `../ucil-wt/WO-0026`)

### Test oracle (6 module-root flat tests, `watcher::` selector)
```
$ cargo nextest run -p ucil-daemon 'watcher::' --status-level=pass --hide-progress-bar
    Starting 6 tests across 4 binaries (49 tests skipped)
        PASS [   0.003s] ucil-daemon watcher::test_event_kind_mapping_covers_create_modify_remove
        PASS [   0.029s] ucil-daemon watcher::test_post_tool_use_hook_bypasses_debounce
        PASS [   0.107s] ucil-daemon watcher::test_watcher_shutdown_is_clean
        PASS [   0.205s] ucil-daemon watcher::test_notify_emits_event_after_debounce
        PASS [   0.355s] ucil-daemon watcher::test_hook_event_source_is_distinct
        PASS [   0.710s] ucil-daemon watcher::test_notify_debounces_editor_writes
     Summary [   0.711s] 6 tests run: 6 passed, 49 skipped
```

- `cargo build --workspace` — OK
- `cargo clippy -p ucil-daemon --all-targets -- -D warnings` — clean (pedantic + nursery)
- `cargo doc -p ucil-daemon --no-deps` — no `^warning` / `^error` lines; `#![deny(rustdoc::broken_intra_doc_links)]` respected
- `grep -rn 'todo!\|unimplemented!\|#\[ignore\]' crates/ucil-daemon/src/watcher.rs` — empty
- `grep -qE '^mod\s+tests\s*\{' crates/ucil-daemon/src/watcher.rs` — empty (module-root flat tests per DEC-0005)
- `notify` + `notify-debouncer-full` present in both `Cargo.toml` (workspace) and `crates/ucil-daemon/Cargo.toml` (via `{ workspace = true }`)

### Forbidden-paths guard
```
crates/ucil-core/           → 0 lines of diff
crates/ucil-treesitter/     → 0 lines of diff
crates/ucil-lsp-diagnostics/ → 0 lines of diff
crates/ucil-embeddings/     → 0 lines of diff
crates/ucil-agents/         → 0 lines of diff
crates/ucil-cli/            → 0 lines of diff
adapters/                   → 0 lines of diff
ml/                         → 0 lines of diff
```

### Reality-check — documented fallback taken

`scripts/reality-check.sh P1-W3-F02` trips the known brand-new-module false-positive
(escalation `20260415-1630-reality-check-per-file-rollback.md`; precedent:
WO-0014 / WO-0015 / WO-0016 / WO-0023): when `watcher.rs` is stashed the nextest
selector `watcher::` matches 0 tests, and the script interprets ZERO tests as
"fake-green" even though the test selector depending on the stashed module is the
correct mutation-check semantics.

The work-order explicitly anticipates this (`acceptance[10]` and
`lessons_applied[11]`). I performed the **manual two-step mutation probe** the
work-order specifies, in this worktree, and confirmed the oracle is real:

1. Roll `crates/ucil-daemon/src/lib.rs` + `crates/ucil-daemon/src/watcher.rs` back to
   the `origin/main` snapshot (`git show origin/main:...`, then delete `watcher.rs`
   since it did not exist on `main`).
2. `cargo nextest run -p ucil-daemon 'watcher::'` → **0 tests matched** (selector
   empty, exit 4). Proves the `watcher::` test oracle depends on the new code.
3. Restore both files from `HEAD`. Re-run: **6 tests pass**. Working tree clean.

Please apply the same manual two-step during verification.

### Anti-abandonment checklist (lessons_applied[11])

- (a) commits on branch: **5** (≥ 4 required)
- (b) `git diff origin/main..HEAD -- crates/ucil-daemon/src/watcher.rs | wc -l` → **644** (> 150 required)
- (c) `git status --porcelain` → **empty** (clean tree)
- (d) `git rev-parse HEAD` == `git rev-parse @{upstream}` → **both `345018c9`** (pushed)
- (e) this ready-for-review marker exists (you are reading it)

## Scope deltas from the planner's commit plan

Planner estimated 6 commits; this branch lands in **5** (plus this marker commit).
`feat(daemon): add watcher module` was a single DEC-0005-style module-introduction
commit (~396 lines including the kind-mapping unit test) rather than split into
(types) → (FileWatcher::new) → (map_notify_kind). Splitting would have produced
the dead-code / empty-Vec-stub intermediate states DEC-0005 explicitly allows
bundling to avoid.

No ADR needed: `notify` 6 + `notify-debouncer-full` 0.3 resolved cleanly on this
toolchain.

## Architectural notes for the verifier

- The `FileWatcher` struct holds `Debouncer<notify::RecommendedWatcher, FileIdMap>`,
  not `NoCache` as the work-order's `scope_in` suggested — `new_debouncer` (the
  helper the same `scope_in` bullet says to use) returns `Debouncer<_, FileIdMap>`.
  `NoCache` would require `new_debouncer_opt` + an explicit `notify::Config`
  argument; the short helper is the idiomatic pick and the `FileIdMap` cache is
  harmless for Phase-1 F02 (we own no rename-tracking).
- The std-side `std::sync::mpsc::Receiver<DebounceEventResult>` is drained from a
  `tokio::task::spawn_blocking` forwarder (not a regular `tokio::spawn`) — a
  blocking `recv` in a tokio task would starve the runtime worker. This mirrors
  the work-order's §"lessons_applied[4]" guidance.
- Hook fast path uses `Sender::try_send` so `notify_hook_event` is sync and
  non-blocking, callable from any context (including a future HTTP/Unix-socket
  hook receiver). `Full` and `Closed` both collapse to `WatcherError::ChannelClosed`
  for F02 — backpressure is a follow-up WO (§"scope_out").
