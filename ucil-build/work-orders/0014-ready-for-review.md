---
work_order: WO-0014
feature_ids: [P1-W5-F03]
branch: feat/WO-0014-lsp-diagnostics-bridge-skeleton
final_commit: c00e76bf1537b2f4db3a7d2538a42f0867c48451
executor_date: 2026-04-18
---

# WO-0014 ready for review — `ucil-lsp-diagnostics` bridge skeleton

**Feature**: `P1-W5-F03` — `ucil-lsp-diagnostics` crate implements
`LspDiagnosticsBridge`: connects to Serena-managed LSP server
instances; no duplicate LSP processes when Serena active.

**Final commit**: `c00e76bf1537b2f4db3a7d2538a42f0867c48451`

**Branch**: `feat/WO-0014-lsp-diagnostics-bridge-skeleton` (pushed to
`origin`).

## Commit sequence (4)

1. `9c98ab3` — `build(lsp-diagnostics): add serde workspace dep for Language enum derives`
2. `869c82e` — `feat(lsp-diagnostics): add Language, LspEndpoint, LspTransport, Diagnostic types`
3. `109bea4` — `feat(lsp-diagnostics): add LspDiagnosticsBridge skeleton with serena_managed state`
4. `c00e76b` — `test(lsp-diagnostics): add module-root bridge:: acceptance tests for F03`

Each commit carries the `Phase: 1` / `Feature: P1-W5-F03` /
`Work-order: WO-0014` trailers per `.claude/rules/commit-style.md`.
Cadence target of 4 commits met. Each commit is self-contained and
compiles cleanly on its own (verified between commits).

## What I verified locally (verifier should re-verify from scratch)

* **AC1 — frozen selector**: `cargo nextest run -p ucil-lsp-diagnostics bridge:: --no-fail-fast` — 4 tests matched, 4 passed, 0 skipped. Two of those (`bridge::test_bridge_with_serena_managed_has_no_own_endpoints` and `bridge::test_bridge_without_serena_has_no_endpoints_until_f07`) are at module root; two are in `bridge::tests::…` as supporting (non-selector-frozen) tests.
* **AC2 — full-crate tests**: `cargo nextest run -p ucil-lsp-diagnostics --no-fail-fast` — 4 tests run, 4 passing.
* **AC3 — workspace build**: `cargo build --workspace` — exit 0.
* **AC4 — clippy**: `cargo clippy -p ucil-lsp-diagnostics --all-targets -- -D warnings` — exit 0. Pedantic + nursery clean.
* **AC5 — fmt**: `cargo fmt --check -p ucil-lsp-diagnostics` — exit 0.
* **AC6 — docs**: `cargo doc -p ucil-lsp-diagnostics --no-deps` — exit 0. No broken intra-doc links.
* **AC7 — no stubs**: `grep -RInE 'todo!\(|unimplemented!\(|NotImplementedError|raise NotImplementedError' crates/ucil-lsp-diagnostics/src/` — no matches.
* **AC8 — no skipped tests**: `grep -RInE '#\[ignore\]|\.skip\(|xfail|it\.skip' crates/ucil-lsp-diagnostics/src/` — no matches.
* **AC9 — module-root placement**: both frozen-aligned tests live outside `mod tests { … }` in `bridge.rs` (tests at lines 208, 223; `mod tests {` starts at line 235).
* **AC10 — no `ucil-daemon` dep** (DEC-0008 cycle-free): `grep -nE 'ucil-daemon\s*=' crates/ucil-lsp-diagnostics/Cargo.toml` — no matches.
* **AC11 — serde dep present**: `grep -nE 'serde\s*=' crates/ucil-lsp-diagnostics/Cargo.toml` — 1 match at line 18 (`serde = { workspace = true }`).
* **AC12 — 7 public types**: `grep -rnE 'pub (struct|enum) (LspDiagnosticsBridge|Language|LspEndpoint|LspTransport|Diagnostic|DiagnosticSeverity|BridgeError)' crates/ucil-lsp-diagnostics/src/` — 7 matches (`BridgeError`, `LspDiagnosticsBridge` in `bridge.rs`; `Language`, `LspTransport`, `LspEndpoint`, `Diagnostic`, `DiagnosticSeverity` in `types.rs`).

## DEC-0008 invariants held

* `LspDiagnosticsBridge::new(serena_managed: bool) -> Self` is the sole constructor. No reference to `ucil-daemon`'s `PluginManager`. `ucil-lsp-diagnostics` stays cycle-free from `ucil-daemon`.
* When `serena_managed = true`, `endpoints` stays empty (test
  `test_bridge_with_serena_managed_has_no_own_endpoints` asserts this).
* When `serena_managed = false`, `endpoints` also stays empty at F03 (test `test_bridge_without_serena_has_no_endpoints_until_f07` asserts this — F07 will populate).
* `LspTransport::Standalone { command, args }` is declared as a placeholder variant so F07 can populate without enum surgery.
* `LspTransport::DelegatedToSerena` documents the Serena-active branch of `DEC-0008` explicitly.
* `Diagnostic` + `DiagnosticSeverity` cover the minimum LSP surface the §13.3 diagram requires (file, line, column, severity, message, source).

## Reality-check notes (escalation 20260415-1630 caveat applies)

Ran `scripts/reality-check.sh P1-W5-F03` after the final commit. The
"stashed code ⇒ tests fail" phase reported `OK: tests failed with code
stashed (as expected)`, confirming the skeleton is a real
contribution (not a fake-green).

The "restore code ⇒ tests pass" phase tripped the known pre-existing
stash-stack bug (escalation 20260415-1630): the script ended up
popping an *unrelated* stash entry (`stash@{0}` belonging to an
in-flight WO-0012 branch that had been parked on the stash stack
from a previous resume) which introduced a merge conflict on the
workspace `Cargo.toml`. This is a **harness issue**, not a test
failure — I reset `Cargo.toml` with `git checkout HEAD -- Cargo.toml`
(leaving the unrelated stash intact on the stack so the WO-0012
executor can still reclaim it) and re-ran the acceptance tests
manually:

```
cargo nextest run -p ucil-lsp-diagnostics bridge:: --no-fail-fast
# -> 4 passed, 0 skipped
```

Verifier should treat the automated reality-check as *partially
inconclusive* (stashed phase passed, restored phase tripped the
documented harness bug) and manually confirm restoration by:

1. Checking `git status` is clean on this branch (it is — working
   tree fully clean after `git checkout HEAD -- Cargo.toml`).
2. Running `cargo nextest run -p ucil-lsp-diagnostics bridge::` —
   should be 4/4 green.

No source was re-edited after the reality-check; only the unrelated
Cargo.toml was reset to `HEAD`.

## Dependencies and scope boundaries held

* `forbidden_paths` respected: no edits under `crates/ucil-daemon/**`, `crates/ucil-treesitter/**`, `crates/ucil-core/**`, `crates/ucil-cli/**`, `crates/ucil-agents/**`, `crates/ucil-embeddings/**`, `adapters/**`, `ml/**`, `plugins/**`, `tests/fixtures/**`, or any other forbidden path.
* No edits to `ucil-build/feature-list.json`, `scripts/gate/**`, `scripts/flip-feature.sh`, `.githooks/**`, or `.claude/hooks/**`.
* `ucil-master-plan-v2.1-final.md` untouched.
* No edits to other work-orders.

## Judgment calls (WO-0012 session-abandonment lesson applied)

The WO flagged one ambiguity risk: the `LspTransport` enum shape.
I defaulted to the two variants listed in `scope_in`
(`DelegatedToSerena` + `Standalone { command, args }`). No
clarification required — both variants compile clean, both are
rustdoc'd, and `F04`/`F07` can extend without re-engineering the
enum. No session stall; first commit landed within 10 minutes of
executor start.

## Known follow-ups (out of scope for this WO)

* Daemon wiring (reading `PluginManager::registered_runtimes()` → passing bool → constructing bridge) — reserved for the progressive-startup integration WO once WO-0008 merges.
* LSP JSON-RPC client + `SerenaClient` trait — `P1-W5-F04`.
* Standalone-subprocess spawning — `P1-W5-F07`.
* `DashMap` promotion for the diagnostics cache — `P1-W5-F04`'s call once concurrent access lands.
