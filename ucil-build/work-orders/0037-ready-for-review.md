# WO-0037 ready for review

- Work order: `ucil-build/work-orders/0037-serena-g1-hover-fusion.json`
- Feature: `P1-W5-F02`
- Branch: `feat/WO-0037-serena-g1-hover-fusion`
- Final commit sha: `c065799` (`feat(daemon): re-export hover fusion surface and fix rustdoc links`)

## What I verified locally

All acceptance criteria from the work-order JSON executed from the worktree root and reported PASS:

1. `cargo nextest run -p ucil-daemon 'executor::test_serena_g1_fusion' --no-fail-fast` — 1 test run, 1 passed (the frozen F02 acceptance selector).
2. `cargo nextest run -p ucil-daemon --no-fail-fast` — 115 tests run, 115 passed, 0 skipped (no regressions to the existing executor / IngestPipeline / server / session / plugin_manager / storage / watcher / startup suites).
3. `cargo clippy -p ucil-daemon --all-targets -- -D warnings` — clean, no `^error` lines in the log (clippy::pedantic + nursery lints pass).
4. `cargo doc -p ucil-daemon --no-deps` — exit 0, no `^error` / `^warning: unresolved` lines (rustdoc gate; private-item intra-doc links were downgraded to backtick code spans to satisfy `rustdoc::private_intra_doc_links` under the crate's `#![deny(warnings)]`).
5. `cargo fmt --check` — clean.
6. Grep-based acceptance:
   - `pub trait SerenaHoverClient` in `crates/ucil-daemon/src/executor.rs` — present.
   - `pub struct HoverDoc` in `crates/ucil-daemon/src/executor.rs` — present.
   - `pub enum HoverSource` in `crates/ucil-daemon/src/executor.rs` — present.
   - `pub async fn enrich_find_definition` in `crates/ucil-daemon/src/executor.rs` — present.
   - `pub use executor::… SerenaHoverClient … enrich_find_definition …` in `crates/ucil-daemon/src/lib.rs` — present (single-line `#[rustfmt::skip]`).
   - `fn test_serena_g1_fusion` in `crates/ucil-daemon/src/executor.rs` — present (module-root, per DEC-0005; exact selector `executor::test_serena_g1_fusion`).
7. Mutation check (per root `CLAUDE.md` "What done looks like" rubric): manually stubbed `enrich_find_definition` to always return `hover: None`, re-ran `cargo nextest run -p ucil-daemon 'executor::test_serena_g1_fusion'` — Scenario A assertion failed with the expected `left: None, right: Some(HoverDoc { …, source: Serena })` diff. Reverted the mutation, re-ran the test — PASS. Working tree is clean post-revert.

## Scope adherence

- No edits to forbidden paths (`server.rs`, `plugin_manager.rs`, `lifecycle.rs`, `session_manager.rs`, `session_ttl.rs`, `storage.rs`, `startup.rs`, `priority_queue.rs`, `text_search.rs`, `understand_code.rs`, `watcher.rs`, `main.rs`, any file under `crates/ucil-core/**`, `crates/ucil-lsp-diagnostics/**`, `tests/fixtures/**`, `ucil-build/feature-list.json`, `ucil-master-plan-v2.1-final.md`, `scripts/gate/**`, `scripts/flip-feature.sh`, `adapters/**`, `ml/**`, `plugins/**`).
- Edits in scope: `crates/ucil-daemon/Cargo.toml` (one new `[dependencies]` entry: `async-trait = { workspace = true }`), `crates/ucil-daemon/src/executor.rs` (new public surface + scripted fake + frozen test), `crates/ucil-daemon/src/lib.rs` (extended module rustdoc paragraph + single-line `pub use executor::{…}` re-export).
- No `#[ignore]`, `.skip()`, `todo!()`, `unimplemented!()`, or `pass`-only bodies.
- No `mockall` / `mockito` / `wiremock` — the `ScriptedFakeSerenaHoverClient` is a hand-written impl of UCIL's own trait, the DEC-0008 dependency-inversion seam (same pattern as `ucil-lsp-diagnostics::{call_hierarchy,quality_pipeline}::fake_serena_client`, already passes verifier).
- No changes to `tests/fixtures/**`, `feature-list.json`, master plan, or `flip-feature.sh`.
- No new ADR required — DEC-0008 already establishes the dependency-inversion pattern for Serena integration.

## Key design notes

- `SerenaHoverClient` is an `#[async_trait::async_trait]` trait with `Send + Sync` bounds so trait objects can live in `Arc<dyn SerenaHoverClient>` inside the daemon's long-lived server state (the live-wiring WO constructs the `Arc` on startup).
- `HoverFetchError` is a `thiserror`-backed `#[non_exhaustive]` enum whose variant payloads are `String` (not concrete wrapped errors) — this keeps the crate cycle-free from `ucil-lsp-diagnostics` and MCP-client internals; the live-wiring WO converts from native errors via `.to_string()`.
- `HoverSource` is `#[non_exhaustive]` with `Serena` / `Lsp` / `None` variants so a later WO can add `TreeSitter` fallback without a `SemVer` break.
- `enrich_find_definition` logs hover-fetch errors at `warn!` (via the `ucil.daemon.executor.enrich_find_definition` tracing span per master-plan §15.2) and yields `hover: None`, so a Serena outage never breaks a G1 response — master-plan §13.4 best-effort contract applies.
- Scripted-fake responses are wrapped in `Arc<Result<_, _>>` because `HoverFetchError` is not `Clone` (and we do not want to add a derive that would force cloning large transport error strings at callsites); the `Arc` sidesteps the restriction without widening trait bounds.

## Out-of-scope follow-ups (explicitly deferred per work-order)

- Wiring `enrich_find_definition` into `server::handle_find_definition` — the P1-W4-F05 frozen acceptance selector `server::test_find_definition_tool` asserts on the current `_meta` JSON shape, so adding `_meta.hover` requires an ADR-gated envelope extension. A follow-up WO lands this.
- Live `SerenaHoverClient` impl against the Serena MCP channel — requires `tools/call` plumbing + `initialize` handshake that exceeds single-WO scope. A separate integration WO will land the live impl.

Verifier: please run `scripts/flip-feature.sh P1-W5-F02` on the FF merge after the full acceptance battery and mutation check pass from a clean session.
