# Root Cause Analysis: WO-0004 — retry 2 (init pipeline + CI)

**Analyst session**: rca-WO-0004-retry2-20260415
**Feature**: WO-0004 (P0-W1-F04, P0-W1-F05, P0-W1-F06, P0-W1-F08)
**Attempts before this RCA**: 2
**Branch**: feat/WO-0004-init-pipeline-and-ci @ HEAD `cd1b423`

> **Note**: This report supersedes the prior RCA (rca-WO-0004-20260415), which
> identified the B1 timeout issue (missing `tokio::time::timeout` on IO await).
> That issue was fixed in commit `d2af2f9`. The current rejection (retry 2) is
> caused by a **different set of structural issues** described below.

---

## Failure pattern

**Rejection 1** (vrf-41a07ee5, retry 0): B1 — missing `tokio::time::timeout` on
`output().await` in `verify_plugin_health()`. Fixed in `d2af2f9`. ✓

**Rejection 2** (vrf-8b2d3f91, retry 2): All 6 acceptance criteria PASS. Clippy
clean. Stub scan clean. Rejection is caused entirely by **mutation check failures**:

- P0-W1-F04 — `reality-check.sh` exits 1: 0 tests run with code stashed.
- P0-W1-F05 — `reality-check.sh` exits 1: 0 tests run with code stashed.
- P0-W1-F06 — `reality-check.sh` exits 3: no commit tagged `Feature: P0-W1-F06`.
- P0-W1-F08 — exits 0 trivially (no `.rs/.ts/.py` source diff → skipped).

Two distinct root causes compound to produce the F04/F05 failures.

---

## Verification

All three issues confirmed via independent inspection of the worktree at
`/home/rishidarkdevil/Desktop/ucil-wt/WO-0004` (HEAD `cd1b423`).

### Issue A — Broken selectors (confirmed)

`cargo nextest list -p ucil-cli` output:
```
ucil-cli::bin/ucil commands::init::tests::test_init_report_json
ucil-cli::bin/ucil commands::init::tests::test_llm_provider_selection
ucil-cli::bin/ucil commands::init::tests::test_plugin_health_verification
```

Frozen selectors in `ucil-build/feature-list.json`:
```json
{ "kind": "cargo_test", "selector": "-p ucil-cli commands::init::test_llm_provider_selection" }
{ "kind": "cargo_test", "selector": "-p ucil-cli commands::init::test_plugin_health_verification" }
{ "kind": "cargo_test", "selector": "-p ucil-cli commands::init::test_init_report_json" }
```

The selectors omit `::tests::`. Running with the broken selector:
```
$ cargo nextest run -p ucil-cli "commands::init::test_llm_provider_selection"
Starting 0 tests across 2 binaries (8 tests skipped)
error: no tests to run
```

Root cause: tests are declared inside a named `mod tests { }` block at
`init.rs:329`, which inserts `::tests::` into the nextest path.
The frozen `acceptance_tests` field cannot be edited (anti-laziness contract).

### Issue B — Co-located tests (confirmed)

All F04/F05/F06 test bodies live in `mod tests { }` inside
`crates/ucil-cli/src/commands/init.rs` (lines 329–510). When
`reality-check.sh` rolls `init.rs` back to its introducing-commit parent
(`a5a5470^`), both the implementation AND the test functions are removed.
Even if Issue A were fixed (selectors corrected), 0 tests would match after
the rollback because the tests no longer exist in the rolled-back file.

The only fix that satisfies both constraints (correct selector path AND tests
survive rollback) is to move the F04/F05/F06 test bodies to a **separate file**
(`crates/ucil-cli/tests/init.rs`) structured so that nextest paths match the
frozen selectors.

### Issue C — Missing `Feature: P0-W1-F06` tag (confirmed)

```
$ git log --grep="Feature: P0-W1-F06" --format='%H %s'
(empty)
```

- `a5a5470` carries `Feature: P0-W1-F04` ✓
- `d2af2f9` carries `Feature: P0-W1-F05` ✓
- No commit carries `Feature: P0-W1-F06`

`reality-check.sh` exits 3 ("No commit found for feature P0-W1-F06") and
`reality-check.sh P0-W1-F06` fails the mutation gate.

---

## Root cause hypotheses (ranked)

| # | Hypothesis | Confidence | Location |
|---|-----------|-----------|---------|
| H1 | Tests inside `mod tests {}` make nextest paths `::tests::test_*`; frozen selectors lack the `::tests::` component; co-located tests also vanish on rollback | 100% | `init.rs:329`; feature-list.json selectors |
| H2 | No `Feature: P0-W1-F06` commit trailer → `reality-check.sh` exits 3 for F06 | 100% | `git log` on branch |
| H3 | `lib.rs` does not expose `pub mod commands;` → integration test file cannot import from the crate | 100% | `lib.rs:1-9` (empty body) |

H1 and H3 are coupled: H3 must be fixed at the same time as H1 for the
integration-test approach to compile.

---

## Remediation

### Fix 1: Expose library API (prerequisite)

**Who**: executor  
**File**: `crates/ucil-cli/src/lib.rs`  
**Change**: Add `pub mod commands;` to the library root so that integration
tests in `tests/` can access the implementation.

`commands/mod.rs` already has `pub mod init;` — no change needed there.

**Diff** (≤10 lines):
```rust
// src/lib.rs — add after the existing deny/warn attributes:
pub mod commands;
```

### Fix 2: Create integration test file with correct module structure

**Who**: executor  
**File**: `crates/ucil-cli/tests/init.rs` (new file)

The file must declare `mod commands { mod init { ... } }` so that nextest
assigns paths `commands::init::test_*`, exactly matching the frozen selectors.

```rust
//! Integration tests for `ucil init` — P0-W1-F04, P0-W1-F05, P0-W1-F06.
//!
//! Module structure mirrors the frozen selectors in feature-list.json:
//!   commands::init::test_llm_provider_selection
//!   commands::init::test_plugin_health_verification
//!   commands::init::test_init_report_json

mod commands {
    mod init {
        use tempfile::TempDir;
        use ucil_cli::commands::init::{
            verify_plugin_health, InitArgs, LlmProvider, P0_PLUGINS, PluginStatusKind,
        };

        fn tmp() -> TempDir {
            TempDir::new().expect("temp dir")
        }

        // ── F04 — LLM provider selection ─────────────────────────────────────

        #[tokio::test]
        async fn test_llm_provider_selection() {
            let dir = tmp();
            let args = InitArgs {
                dir: dir.path().to_path_buf(),
                llm_provider: Some(LlmProvider::Ollama),
                no_install_plugins: true,
            };
            ucil_cli::commands::init::run(args).await.expect("init should succeed");

            let toml_str =
                std::fs::read_to_string(dir.path().join(".ucil/ucil.toml")).expect("ucil.toml");
            assert!(
                toml_str.contains("provider = \"ollama\""),
                "ucil.toml must contain provider = \"ollama\"; got:\n{toml_str}"
            );

            let dir2 = tmp();
            let args2 = InitArgs {
                dir: dir2.path().to_path_buf(),
                llm_provider: None,
                no_install_plugins: true,
            };
            ucil_cli::commands::init::run(args2).await.expect("init (no provider) should succeed");
            let toml_str2 =
                std::fs::read_to_string(dir2.path().join(".ucil/ucil.toml")).expect("ucil.toml");
            assert!(
                toml_str2.contains("provider = \"none\""),
                "ucil.toml must default to provider = \"none\"; got:\n{toml_str2}"
            );
        }

        // ── F05 — Plugin health verification ─────────────────────────────────

        #[tokio::test]
        async fn test_plugin_health_verification() {
            // Test verify_plugin_health() directly (it is pub).
            let statuses = verify_plugin_health().await;
            assert_eq!(
                statuses.len(),
                P0_PLUGINS.len(),
                "must return one entry per P0 plugin"
            );
            for s in &statuses {
                let valid =
                    matches!(s.status, PluginStatusKind::Ok | PluginStatusKind::Degraded);
                assert!(
                    valid,
                    "status for '{}' must be Ok or Degraded from verify_plugin_health",
                    s.name
                );
            }

            // Test skipped behavior through run() (skipped_plugin_health is private).
            let dir = tmp();
            let args = InitArgs {
                dir: dir.path().to_path_buf(),
                llm_provider: None,
                no_install_plugins: true,
            };
            ucil_cli::commands::init::run(args).await.expect("init should succeed");
            let report: serde_json::Value = serde_json::from_str(
                &std::fs::read_to_string(dir.path().join(".ucil/init_report.json"))
                    .expect("init_report.json"),
            )
            .expect("valid JSON");
            for entry in report["plugin_health"].as_array().expect("array") {
                assert_eq!(
                    entry["status"], "skipped",
                    "all statuses must be 'skipped' with --no-install-plugins"
                );
            }
        }

        // ── F06 — init_report.json ────────────────────────────────────────────

        #[tokio::test]
        async fn test_init_report_json() {
            let dir = tmp();
            std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname=\"test\"\n").unwrap();
            let args = InitArgs {
                dir: dir.path().to_path_buf(),
                llm_provider: Some(LlmProvider::Claude),
                no_install_plugins: true,
            };
            ucil_cli::commands::init::run(args).await.expect("init should succeed");

            let report_path = dir.path().join(".ucil/init_report.json");
            assert!(report_path.exists(), "init_report.json must be created");
            let content = std::fs::read_to_string(&report_path).expect("read init_report.json");
            let report: serde_json::Value =
                serde_json::from_str(&content).expect("init_report.json must be valid JSON");

            assert_eq!(report["llm_provider"], "claude", "llm_provider mismatch");
            assert_eq!(report["schema_version"], "1.0.0", "schema_version mismatch");
            assert!(report["languages"].is_array(), "languages must be an array");
            assert!(report["plugin_health"].is_array(), "plugin_health must be an array");
            for entry in report["plugin_health"].as_array().expect("array") {
                assert_eq!(entry["status"], "skipped", "status must be skipped");
            }
            let langs = report["languages"].as_array().expect("array");
            assert!(
                langs.iter().any(|l| l == "rust"),
                "rust should be detected from Cargo.toml"
            );
        }
    }
}
```

**Access note**: `skipped_plugin_health()` is private. The integration test for
F05 tests the same behavior through `run()` with `no_install_plugins: true` and
inspects `init_report.json` — functionally equivalent verification without
requiring API exposure of a private helper.

### Fix 3: Remove F04/F05/F06 test bodies from `init.rs`

**Who**: executor  
**File**: `crates/ucil-cli/src/commands/init.rs`  
**Change**: Delete lines 382–509 (the three `async fn test_*` functions inside
`mod tests {}`). Keep lines 343–380 (the five synchronous language-detection
tests: `detects_rust_from_cargo_toml`, `detects_python_from_pyproject`,
`detects_typescript_from_package_json`, `detects_go_from_go_mod`,
`empty_dir_detects_nothing`) — these are pre-existing and their nextest paths
do not conflict with any frozen selector.

After this change, the `mod tests {}` block in `init.rs` retains only the
language-detection tests.

### Fix 4: Add `Feature: P0-W1-F06` commit trailer

**Who**: executor  
**Action**: After completing Fixes 1–3, add a chore commit:

```
chore(cli): tag Feature: P0-W1-F06 — init_report.json implementation

F06 (init_report.json serialisation) was implemented in the same commit
as F04 (a5a5470). Adding the trailer so reality-check.sh can locate the
source commit for the F06 mutation check.

Phase: 0
Feature: P0-W1-F06
Work-order: WO-0004
```

This is a content-only commit (no source changes); `reality-check.sh` will
find the commit and then walk the source files for F06's introducing commit.
Note: the script finds source files from ALL commits tagged `Feature: P0-W1-F06`
as a union — adding this chore commit (which touches no `.rs` files) is
harmless; the script will union with `a5a5470` (which does touch `init.rs`).

### Required dev-dependencies

- `tempfile` — already in `[dev-dependencies]` in `Cargo.toml` ✓
- `tokio` — already in `[dependencies]` (available to integration tests) ✓
- `serde_json` — already in `[dependencies]` ✓

No `Cargo.toml` changes required.

---

## Acceptance after fixes

```bash
# Selector now resolves to the integration test binary:
cargo nextest list -p ucil-cli | grep "test_llm_provider_selection"
# → ucil-cli::tests/init commands::init::test_llm_provider_selection

# Mutation check for F04:
#   reality-check.sh rolls back init.rs to a5a5470^ 
#   → LlmProvider, run(), InitArgs not in pre-F04 init.rs → compile error
#   → cargo nextest exits non-zero → reality-check reports OK (failure with stash) ✓

# F06 tag:
bash scripts/reality-check.sh P0-W1-F06
# → finds commit a5a5470 (F04 intro) + new chore commit → files: init.rs → proceeds ✓

# Full acceptance suite:
cargo nextest run -p ucil-cli --test-threads 1  # 8 unit + 3 integration tests
```

---

## Risk

| Risk | Severity | Mitigation |
|------|----------|-----------|
| `pub mod commands;` in lib.rs exposes internal modules | Low | Intentional for testing; lib.rs is a published library target already |
| Integration test for F05 calls real binaries | Low | `verify_plugin_health()` is already timeout-guarded and gracefully degrades to `Degraded` |
| Removing tests from `init.rs` breaks language-detection test paths | None | Language-detection tests remain in `mod tests {}` — paths unchanged |

---

## If hypothesis is wrong

If after Fix 1+2 nextest still shows a path other than
`commands::init::test_llm_provider_selection`, run:
```bash
cargo nextest list -p ucil-cli | grep "test_llm_provider"
```
Adjust the outer module nesting in `tests/init.rs` to match the required path.

If the mutation check for F04 trivially passes (0 tests) even after Fix 2,
verify that `ucil-cli` is actually building the integration test binary by
running `cargo nextest list -p ucil-cli --list-type all`.

---

*Generated by root-cause-finder on 2026-04-15. Supersedes rca-WO-0004-20260415.*
