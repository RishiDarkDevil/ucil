# WO-0028 — ready for review

**Work-order:** `ucil-build/work-orders/0028-progressive-startup.json`
**Feature:** `P1-W3-F08` — Progressive startup (MCP server LIVE within 2 s + priority indexing queue)
**Branch:** `feat/WO-0028-progressive-startup`
**Final commit:** `22a3f1dd6bc385c3a9d5f9fbe164d177ec9d0284`
**Commits on branch:** 4 (all pushed to origin; branch HEAD == @{upstream})

## What shipped

1. `crates/ucil-daemon/src/priority_queue.rs` — new module (~280 lines). `QueueEntry { path, last_queried: Instant }` with reversed `Ord` so a `BinaryHeap<QueueEntry>` (max-heap) pops newest-Instant first. `PriorityIndexingQueue` wraps a `std::sync::Mutex<BinaryHeap<QueueEntry>>` with `new` / `enqueue` / `touch` / `pop` / `peek` / `len` / `is_empty`. `touch` is an alias for `enqueue` (pushes a fresh Instant); stale older entries for the same path remain in the heap and surface in their original order. `PoisonError` handled internally via `into_inner()`. `#[tracing::instrument]` spans `ucil.daemon.priority_queue.enqueue` / `.touch` per master-plan §15.2.

2. `crates/ucil-daemon/src/startup.rs` — new module (~400 lines). `STARTUP_DEADLINE: Duration = 2000 ms` (master-plan §21.2 Phase 1 budget). `StartupError` via `thiserror` + `#[non_exhaustive]` with `DeadlineExceeded { elapsed_ms, budget_ms }` / `ServerTaskJoin(#[from] tokio::task::JoinError)` / `Mcp(#[from] McpError)`. `ProgressiveStartup::new(server, Arc<queue>)` + `start<R,W>(reader, writer) -> (JoinHandle, ReadyHandle)` spawns `McpServer::serve` on a tokio task, wrapping the writer in a private `ReadyProbeWriter` that tracks two booleans across `poll_write` calls (`seen_id`, `seen_newline`) and fires a `oneshot::Sender<Duration>` the moment both are true. `ReadyHandle::wait` wraps the oneshot in `tokio::time::timeout(STARTUP_DEADLINE + 500ms, ...)`. `handle_call_for_priority(&queue, &arguments)` walks `arguments.current_task.files_in_context` (CEQP §8.2) and calls `queue.touch(path)` per string entry.

3. `crates/ucil-daemon/src/server.rs` — one new module-root test `test_progressive_startup` (+ ~137 lines) added after `test_ceqp_params_on_all_tools`. Drives a real `tokio::io::duplex(64 * 1024)` pair, reads the response concurrently with awaiting `ReadyHandle::wait` (so the server's trailing-`\n` `poll_write` is not backpressured by the duplex buffer), asserts measured duration < `STARTUP_DEADLINE`, re-asserts the 22-tool catalogue, and pins the priority-ordering invariant.

4. `crates/ucil-daemon/src/lib.rs` — added `pub mod priority_queue;`, `pub mod startup;`, and matching `pub use` re-exports (alphabetical); extended the module-level rustdoc with one sentence per new module citing §18 Phase 1 Week 3 line 1745 + §21.2 lines 2196-2204.

## What I verified locally (from `../ucil-wt/WO-0028`)

- `cargo nextest run -p ucil-daemon server::test_progressive_startup` — **1 pass** (the frozen P1-W3-F08 selector).
- `cargo nextest run -p ucil-daemon 'priority_queue::'` — **5 passes** (the five module-root tests: ord-is-newest-first, enqueue+pop, touch-reorders, len-tracking, peek-does-not-consume).
- `cargo nextest run -p ucil-daemon 'startup::'` — **4 passes** (deadline constant, handle_call_for_priority happy + missing-key, ready-handle-resolves-on-first-write).
- `cargo nextest run -p ucil-daemon --no-fail-fast` — **65 passes, 0 skipped** (no regression to the pre-existing 62+; the +3 comes from the two new module tests + the one new server.rs test that nextest can now select).
- `cargo build --workspace` — clean.
- `cargo clippy -p ucil-daemon --all-targets -- -D warnings` — clean (pedantic + nursery).
- `cargo doc -p ucil-daemon --no-deps` — clean (no `^warning` / `^error`; `lib.rs` has `#![deny(rustdoc::broken_intra_doc_links)]`).
- `cargo fmt --check` — clean.
- `bash scripts/reality-check.sh P1-W3-F08` — **green** (stashed → tests fail as expected; restored → tests pass).
- No `todo!()` / `unimplemented!()` / `#[ignore]` in the new sources.
- No `mod tests { }` wrapper in `priority_queue.rs`, `startup.rs`, or the `server.rs` test addition (DEC-0005 module-root flat convention honoured).
- Forbidden-path guard: `git diff origin/main..HEAD` returns **0 lines** for every deny-listed crate/file (`ucil-core`, `ucil-treesitter`, `ucil-lsp-diagnostics`, `ucil-embeddings`, `ucil-agents`, `ucil-cli`, `adapters`, `ml`, `watcher.rs`, `session_manager.rs`, `session_ttl.rs`, `lifecycle.rs`, `main.rs`).

## Notes for the verifier / critic

- **Duplex buffer bumped to 64 KiB.** The pre-existing `test_all_22_tools_registered` uses `duplex(16 * 1024)`, which works because it reads responses synchronously in a linear flow. My new `test_progressive_startup` awaits `ReadyHandle::wait` first, so I drain the response concurrently via `tokio::join!`. The `64 * 1024` bump (~4× the measured 18 KiB `tools/list` response) is belt-and-braces against transient stalls; it is a **test-local** constant and does not change any production code path.
- **Probe adapter tracks two bools across `poll_write` calls.** The server's `serve()` loop writes responses as two `write_all` calls (JSON body, then `\n`). The WO planner's original "last byte `\n` AND buf contains `"id"` on the same poll_write" heuristic never fires because these markers arrive in separate buffers. The landed implementation tracks `seen_id` and `seen_newline` across calls and fires the oneshot the moment both are true — unchanged observable contract, robust wire behaviour. See commit `e2179be` for the rationale.
- **Reality-check fallback** noted in the WO (escalation `20260415-1630-reality-check-per-file-rollback.md`): `scripts/reality-check.sh P1-W3-F08` ran cleanly on this branch with no manual intervention (stashed → fail; restored → pass). If the verifier's rerun from `main` trips the multi-commit-rollback heuristic, the manual two-step mutation should target the four files listed above (`priority_queue.rs`, `startup.rs`, the test addition in `server.rs`, and the `lib.rs` module + re-export lines).
- **`ProgressiveStartup` is not yet wired into `main.rs` / `Lifecycle`.** Out of scope for this WO per the planner — deferred to a later integration WO. `main.rs` is byte-for-byte unchanged.

## Commits on this branch

```
22a3f1d test(daemon): server::test_progressive_startup acceptance over real duplex
e2179be fix(daemon): startup probe — track seen_id + seen_newline across writes
c28e895 feat(daemon): startup — ProgressiveStartup + ReadyHandle + priority-touch helper
a482dbf feat(daemon): priority_queue — recency-ordered PriorityIndexingQueue
```
