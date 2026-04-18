---
work_order: WO-0038
slug: lsp-bridge-integration-test-suite
feature_ids: [P1-W5-F08]
branch: feat/WO-0038-lsp-bridge-integration-test-suite
final_commit: 971f10bc676f2b7839c0df7aec05ea623c99a479
ready_at: 2026-04-18
author: executor
---

# WO-0038 — ready for review

Phase 1 Week 5 feature **P1-W5-F08 — LSP bridge integration test suite**
is implemented and every acceptance criterion in the work-order passed
locally in the worktree at `/home/rishidarkdevil/Desktop/ucil-wt/WO-0038`.

## Final commit

`971f10bc676f2b7839c0df7aec05ea623c99a479` on
`feat/WO-0038-lsp-bridge-integration-test-suite` (pushed to origin).

Commits on branch (chronological):

1. `754b6b9` — `build(workspace): add tests/integration as workspace member`
2. `6968a0e` — `build(tests-integration): land ucil-tests-integration skeleton`
3. `971f10b` — `test(tests-integration): ship test_lsp_bridge across four fixtures`

## Files landed by this work-order

| Path | Change | Notes |
|------|--------|-------|
| `Cargo.toml` | modified | appended `"tests/integration"` to `[workspace].members` |
| `Cargo.lock` | modified | registers new `ucil-tests-integration` crate (`971f10b`) |
| `tests/integration/Cargo.toml` | new | manifest with `[[test]] name = "test_lsp_bridge" path = "test_lsp_bridge.rs"` |
| `tests/integration/src/lib.rs` | new | placeholder `lib` target; re-exports nothing (DEC-0010) |
| `tests/integration/test_lsp_bridge.rs` | new | 743 lines; 4 fixture tests + fixture-coverage guard |
| `tests/integration/.gitkeep` | removed | superseded by `src/lib.rs` + `test_lsp_bridge.rs` |

No files under `tests/fixtures/**` were modified (verified with
`git diff --stat main..HEAD -- tests/fixtures/` → empty).

## What I verified locally

### Acceptance commands (from WO §acceptance_criteria)

- `cargo nextest run --test test_lsp_bridge --no-fail-fast` → **5 tests run: 5 passed, 0 skipped**
- `cargo test --test test_lsp_bridge --no-fail-fast` → **test result: ok. 5 passed; 0 failed; 0 ignored**
- `cargo clippy --workspace --all-targets -- -D warnings` → clean (finished without warnings)
- `cargo fmt --all --check` → exit 0
- `cargo doc --workspace --no-deps` → clean (no `unresolved link` / `broken_intra_doc_links` warnings)

### Broader regression checks

- `cargo nextest run --workspace --no-fail-fast` → **276 tests run: 276 passed, 0 skipped**
  (no pre-existing Phase-1 test regressed when the new binary was added)

### File-layout / content criteria (from WO §acceptance_criteria)

- ✅ `tests/integration/Cargo.toml` exists and declares `[[test]] name = "test_lsp_bridge" path = "test_lsp_bridge.rs"` (per DEC-0010).
- ✅ `tests/integration/src/lib.rs` exists (so `[lib]` target has a source file per Cargo expectations).
- ✅ `tests/integration/test_lsp_bridge.rs` exists with **four `#[tokio::test]` functions** — one per Phase-1 fixture:
  - `test_rust_project_diagnostics_and_calls`
  - `test_python_project_diagnostics_and_calls`
  - `test_typescript_project_diagnostics_and_calls`
  - `test_mixed_project_diagnostics_and_calls`
- ✅ One additional sync `#[test] test_suite_covers_four_fixtures` walks `tests/fixtures/` and asserts the binary's four LSP tests cover each of the four Phase-1 fixture projects (regression fuse for fixture renames).
- ✅ Each test exercises **both** `persist_diagnostics` and `persist_call_hierarchy_incoming` (grep-verified: each test body contains both call sites).
- ✅ `impl SerenaClient for LocalScriptedFake` is the only `SerenaClient` implementation in this binary; no `rstest`/`mockall`/`wiremock` crate is pulled in (grep-verified against `Cargo.toml`).
- ✅ All fixture paths resolve through `CARGO_MANIFEST_DIR`-rooted joins — no absolute paths or `$HOME` in the test bodies.
- ✅ Every `.await` that reaches the bridge is wrapped in `tokio::time::timeout(BRIDGE_AWAIT_BUDGET, ...)` where `BRIDGE_AWAIT_BUDGET = Duration::from_secs(10)`.
- ✅ No `#[ignore]`, no `.skip()`, no commented-out assertions (grep-verified).
- ✅ No `todo!()`, no `unimplemented!()`, no `NotImplementedError`, no `panic!("TODO")` (grep-verified).
- ✅ Root `Cargo.toml` `[workspace].members` list includes `"tests/integration"`.

### Forbidden-path check

- ✅ No writes under `tests/fixtures/**` (per `git diff --stat main..HEAD`).
- ✅ No writes under `ucil-build/feature-list.json` (verifier-only).

## Deviations documented inline

The work-order text referenced `lsp_types::Uri` for the trait parameter,
but the actual `SerenaClient` trait signature takes `lsp_types::Url`
(re-export of `url::Url`).  The tests use `Url` as required by the actual
trait — the deviation is documented in:

- Commit `971f10b` body (final paragraph).
- The module-level rustdoc in `test_lsp_bridge.rs` (heading “Why `Url`
  and not `Uri`”).

This is a trivial text-vs-code mismatch; no ADR was required.

## Ready for

- `critic` (pre-verifier review of the diff).
- `verifier` (fresh-session cold-cache run + `passes` flip on `P1-W5-F08`).
