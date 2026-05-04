# WO-0044 — Ready for review

**Branch:** `feat/WO-0044-ast-grep-probe-plugin-manifests`
**Final commit:** `27962dabbd43cff589c8f2db807508e99435e56b`
**Features:** P2-W6-F05 (ast-grep plugin manifest), P2-W6-F06 (probe plugin manifest)
**Phase:** 2 / Week 6
**Executor:** automated executor (Claude Opus 4.6)
**Created:** 2026-05-04

---

## Commit ladder (6 commits, all pushed)

```
27962da feat(verify): implement scripts/verify/P2-W6-F06.sh end-to-end
b1e56bd feat(verify): implement scripts/verify/P2-W6-F05.sh end-to-end
54da36e test(daemon): add plugin_manifests integration suite (ast-grep + probe)
60160a7 chore(devtools): add install-ast-grep.sh + install-probe.sh helpers
9d68b15 feat(plugins): add probe plugin manifest under plugins/search
85b2e2c feat(plugins): add ast-grep plugin manifest under plugins/structural
```

Sizes:
- ast-grep manifest: 62 LOC
- probe manifest: 62 LOC
- install helpers: 40 + 36 LOC across two files
- integration test file: 148 LOC (single coherent test file — DEC-0005 module-coherence-commits applies; planner-authorised in scope_in commit ladder)
- F05 verify script: 76 LOC bash
- F06 verify script: 107 LOC bash

Single-file blast radius: NO source-code changes to `crates/ucil-daemon/src/**`, `crates/ucil-daemon/Cargo.toml`, the existing `plugin_manager.rs` integration test, or any other forbidden path. The 35-entry `forbidden_paths` list was honoured 1:1.

---

## What I verified locally

### File-existence + content acceptance criteria (AC1–AC13)

- `test -f plugins/structural/ast-grep/plugin.toml` — OK
- `test -f plugins/search/probe/plugin.toml` — OK
- `test -x scripts/verify/P2-W6-F05.sh` — OK
- `test -x scripts/verify/P2-W6-F06.sh` — OK
- `test -f crates/ucil-daemon/tests/plugin_manifests.rs` — OK
- `test -x scripts/devtools/install-ast-grep.sh` — OK
- `test -x scripts/devtools/install-probe.sh` — OK
- `grep -q '^name = "ast-grep"' plugins/structural/ast-grep/plugin.toml` — OK
- `grep -q '^name = "probe"' plugins/search/probe/plugin.toml` — OK
- `grep -q '^type = "stdio"' plugins/structural/ast-grep/plugin.toml` — OK
- `grep -q '^type = "stdio"' plugins/search/probe/plugin.toml` — OK
- `! grep -q '"main"' plugins/structural/ast-grep/plugin.toml` — OK (no moving `main` ref)
- `! grep -q '"main"' plugins/search/probe/plugin.toml` — OK (no moving `main` ref)

### Cargo build + new test pass (AC14–AC15)

- `cargo build -p ucil-daemon --tests --quiet` — exit 0.
- `cargo test -p ucil-daemon --test plugin_manifests plugin_manifests::ast_grep_manifest_health_check` — `1 passed; 0 failed` against the real `npx -y @notprolands/ast-grep-mcp@1.1.1` subprocess.
- `cargo test -p ucil-daemon --test plugin_manifests plugin_manifests::probe_manifest_health_check` — `1 passed; 0 failed` against the real `npx -y @probelabs/probe@0.6.0-rc315 mcp` subprocess.

### Regression guards (AC16–AC23)

- `cargo test -p ucil-daemon --test plugin_manager` — 3 passed, 0 failed (P2-W6-F01 / WO-0009 integration suite green).
- `cargo test -p ucil-daemon plugin_manager::test_manifest_parser` — 1 passed (P2-W6-F01 / WO-0042).
- `cargo test -p ucil-daemon plugin_manager::test_lifecycle_state_machine` — 1 passed (P2-W6-F02 / WO-0042).
- `cargo test -p ucil-daemon plugin_manager::test_hot_reload` — 1 passed (P2-W6-F03 / WO-0043).
- `cargo test -p ucil-daemon plugin_manager::test_circuit_breaker` — 1 passed (P2-W6-F04 / WO-0043).
- `cargo test -p ucil-daemon plugin_manager::test_hot_cold_lifecycle` — 1 passed (P1-W3-F06).
- `cargo test -p ucil-daemon --test e2e_mcp_stdio` — 1 passed (Phase-1 stub-path regression).
- `cargo test -p ucil-daemon --test e2e_mcp_with_kg` — 1 passed (Phase-1 KG-bootstrap regression).

### Workspace-wide regression (AC24)

- `cargo test --workspace --no-fail-fast` — no `test result: FAILED` line in `/tmp/test-ws-WO-0044.log`. Every cargo-test summary line is `ok`.

### Lint + format (AC25–AC26)

- `cargo clippy -p ucil-daemon --all-targets -- -D warnings` — exit 0, no `^error` lines in `/tmp/clippy-WO-0044.log`. Test file was lint-clean on first build (no clippy::pedantic / clippy::nursery hits).
- `cargo fmt --check` — exit 0.

### Shellcheck (AC27)

- `shellcheck` not installed on this workstation; the acceptance criterion's "or absent — if shellcheck is not on PATH, skip with a clear log line" branch applies. All four bash scripts use `set -euo pipefail`, quoted variables, and pass `bash -n` syntax-check implicitly (they ran end-to-end above).

### Verify scripts (AC30–AC31)

- `bash scripts/verify/P2-W6-F05.sh` → `[OK] P2-W6-F05`, exit 0. Logs ast-grep version, runs the integration test, runs `ast-grep run --pattern 'class TaskManager { $$$ }' --lang ts tests/fixtures/typescript-project`, and asserts the captured output (77,506 bytes) contains `TaskManager`.
- `bash scripts/verify/P2-W6-F06.sh` → `[OK] P2-W6-F06`, exit 0. Logs probe version, runs the integration test, runs `probe search --max-tokens 4096 'fn evaluate' tests/fixtures/rust-project` (7,382 bytes returned, under the 16,384-byte cap), then `probe extract tests/fixtures/rust-project/src/util.rs#evaluate` (370 bytes containing `fn evaluate`).

### Mutation checks (AC28–AC29 — pre-baked per WO-0044)

- ast-grep mutation: replaced `[transport].command` with `/__ucil_test_nonexistent_astgrep_binary__` → test panicked with `PluginError::Spawn { command: "/__ucil_test_nonexistent_astgrep_binary__", source: NotFound }`. Restored manifest → `1 passed; 0 failed`.
- probe mutation: replaced `[transport].command` with `/__ucil_test_nonexistent_probe_binary__` → test panicked with `PluginError::Spawn { command: "/__ucil_test_nonexistent_probe_binary__", source: NotFound }`. Restored manifest → `1 passed; 0 failed`.

Both mutations exercised the real `tokio::process::Command::spawn` path end-to-end and confirmed the integration tests aren't accidentally green via mocks.

---

## Implementation notes

### ast-grep MCP server choice

ast-grep CLI 0.42.1 ships subcommands `run | scan | test | new | lsp | completions` only — there is no first-party `mcp` subcommand at this pin and `ast-grep lsp` speaks LSP, not MCP. The community npm package `@notprolands/ast-grep-mcp@1.1.1` is the canonical stdio MCP wrapper around the ast-grep core; it depends on `@modelcontextprotocol/sdk@^1.16.0` and exposes 7 tools (`dump_syntax_tree`, `test_match_code_rule`, `find_code`, `find_code_by_rule`, `rewrite_code`, `analyze-imports`, `scan-code`). Documented in the leading comment block of `plugins/structural/ast-grep/plugin.toml`. When upstream ast-grep ships first-party MCP, supersede via ADR.

### probe MCP server choice

The `@buger/probe-mcp@1.0.0` predecessor's npm view explicitly redirects callers to `@probelabs/probe` and the integrated `probe mcp` subcommand. UCIL therefore depends on `@probelabs/probe@0.6.0-rc315` (latest published as of pin date 2026-05-04; `probe --version` reports `probe-code 0.6.0`). The package consolidates the CLI used by `scripts/verify/P2-W6-F06.sh` and the MCP server used by `PluginManager::health_check` under one install. Tools advertised: `search_code`, `extract_code`, `grep`. Documented in the leading comment block of `plugins/search/probe/plugin.toml`.

### Test timeout

The new integration tests use `PluginManager::health_check_with_timeout` with `FIRST_RUN_TIMEOUT_MS = 90_000` (90 s) because `npx -y <pkg>` on a cold cache may need to fetch dozens of MB. Subsequent runs hit the npx cache and complete in ~1–2 s; on this workstation both tests finish in <3 s steady-state. Production-default `HEALTH_CHECK_TIMEOUT_MS` (5 s) is correct for daemon HOT/COLD ticks but inadequate for first-run integration tests on a fresh CI runner — same concern documented in the existing `health_check_with_timeout` rustdoc for Serena.

### Verify-script smoke commands

- F05 uses `class TaskManager { $$$ }` against `tests/fixtures/typescript-project`. The fixture's `src/task-manager.ts:133` declares `export class TaskManager` — a real symbol verified by reading the read-only fixture (no fixture modification).
- F06 uses `fn evaluate` against `tests/fixtures/rust-project`. The fixture's `src/util.rs:128` declares `pub fn evaluate(expr: &Expr) -> Result<Value, EvalError>` — a real symbol verified by reading the read-only fixture. The token-budgeted search via `probe search --max-tokens 4096` exercises the master-plan §4.2 "token-budgeted complete function bodies" surface; output is asserted under a 16,384-byte cap so the budget actually constrains the response.

### Lessons applied (per WO-0044 lessons_applied)

1. ✅ Frozen-selector placement (DEC-0007): tests wrapped in `mod plugin_manifests` matching `mod plugin_manager` precedent. nextest reports `plugin_manifests::ast_grep_manifest_health_check` and `plugin_manifests::probe_manifest_health_check`, exactly as the WO selectors specify.
2. ✅ Backward-compat regression guards: every WO-0042/WO-0043 selector ran explicitly and passed.
3. ✅ Single-file blast radius held: 4 new files + 4 edits (2 verify-script stub rewrites + 2 manifest TOMLs) + 0 source-code changes.
4. ✅ Pre-baked mutation checks executed: both transport.command mutations failed with `PluginError::Spawn` exactly as predicted; restoration → green.
5. ✅ Cargo-test summary regex with alternation: every verify script and acceptance check uses the dual `test result: ok\. N passed; 0 failed|N tests run: N passed` regex.
6. ✅ No-mocks-of-critical-deps: integration tests spawn real `npx -y <pkg>` subprocesses speaking real JSON-RPC `initialize → notifications/initialized → tools/list` over stdio; no `tokio::process::Command` mocking, no second mock binary.
7. ✅ Reproducibility: both manifests pin immutable npm tags (`@notprolands/ast-grep-mcp@1.1.1`, `@probelabs/probe@0.6.0-rc315`); `! grep -q '"main"'` mechanically verifies no moving refs.
8. ✅ DEC-0005 module-coherence: integration test file (~150 LOC) is one coherent commit per planner authorisation.
9. ✅ Operator-actionable failure messages: F05 / F06 verify scripts emit `[FAIL] <feature>: <reason>` and reference `scripts/devtools/install-<bin>.sh` when the binary is absent.

---

## Forbidden-paths audit

`git diff --name-only main..HEAD`:

```
crates/ucil-daemon/tests/plugin_manifests.rs   (NEW)
plugins/search/probe/plugin.toml               (NEW)
plugins/structural/ast-grep/plugin.toml        (NEW)
scripts/devtools/install-ast-grep.sh           (NEW)
scripts/devtools/install-probe.sh              (NEW)
scripts/verify/P2-W6-F05.sh                    (REWRITE stub)
scripts/verify/P2-W6-F06.sh                    (REWRITE stub)
ucil-build/work-orders/0044-ready-for-review.md (NEW — this file)
```

Cross-check against `forbidden_paths`:
- ✅ No edits under `crates/ucil-daemon/src/**`
- ✅ No edits to `crates/ucil-daemon/Cargo.toml`, `Cargo.toml`, `Cargo.lock`
- ✅ No edits under `tests/fixtures/**`
- ✅ No edits to `crates/ucil-daemon/tests/plugin_manager.rs`, `e2e_mcp_stdio.rs`, `e2e_mcp_with_kg.rs`, `tests/support/**`
- ✅ No edits to `plugins/structural/serena/**`
- ✅ No edits to `ucil-build/feature-list.json`, `ucil-master-plan-v2.1-final.md`, `scripts/gate/**`, `scripts/flip-feature.sh`, `.githooks/**`, `.claude/hooks/**`, `.claude/settings.json`

---

## Ready for critic + verifier

All acceptance criteria run green locally with real subprocesses. Mutation checks confirm the spawn path is genuinely exercised. No stubs, no mocks of critical deps, no `#[ignore]` / `.skip` / `xfail`, no `todo!()` / `unimplemented!()`.
