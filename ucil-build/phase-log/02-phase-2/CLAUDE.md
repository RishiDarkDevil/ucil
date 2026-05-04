# Phase 2 — Plugins + G1/G2 + embeddings (Weeks 6–8)

## Goal (master-plan §18)
Plugin system operational. G1 (Structural) and G2 (Search) fully working. Embedding pipeline operational.

## Features in scope (25)
- **Week 6 — Plugin system**: `P2-W6-F01..F08`
  Plugin manifest parser (capabilities + activation rules), full lifecycle state machine
  (DISCOVERED→REGISTERED→LOADING→ACTIVE→IDLE→STOPPED→ERROR), hot-reload, circuit breakers,
  ast-grep + Probe manifests, `ucil plugin` CLI subcommands, lifecycle integration suite.
- **Week 7 — G1 + G2 fusion**: `P2-W7-F01..F09`
  G1 parallel execution (tree-sitter + Serena + ast-grep + diagnostics), G1 result fusion,
  G2 intra-group RRF (Probe×2.0, ripgrep×1.5, LanceDB×1.5), session deduplication,
  `find_references` / `search_code` MCP tools, ripgrep plugin smoke, SCIP P1, LanceDB per-branch.
- **Week 8 — Embedding pipeline**: `P2-W8-F01..F08`
  `ucil-embeddings` crate with ONNX Runtime, CodeRankEmbed (137M, CPU) default + Qwen3 (8B GPU)
  upgrade, LanceDB integration, background indexing, throughput / latency / recall benchmarks.

## Gate criteria (`scripts/gate/phase-2.sh`)
- All 25 phase-2 features `passes=true` and `last_verified_by` starts with `verifier-`.
- No `#[ignore]`/`xfail`/`skip` on phase-2 tests.
- Plugin lifecycle integration suite (`tests/integration/test_plugin_lifecycle.rs`) green.
- Vector search benchmark recorded in `docs/benchmarks.md`.

## Dependencies (external)
- ast-grep binary on PATH (Week 6).
- probe binary (https://github.com/buger/probe) on PATH.
- ripgrep on PATH (already a dev tool).
- ONNX Runtime via the `ort` crate (Week 8). CPU build is the default; GPU is opt-in behind a
  cargo feature.
- LanceDB via `lancedb` crate.

## Risks carried from Phase 1
- Plugin manifest parsing currently models only `[plugin]`, `[transport]`, `[lifecycle]`.
  `[capabilities]` (provides / languages / activation rules) and `[resources]` are NEW fields
  introduced in Week 6 — schema-compat with existing fixture manifests must be tested.
- Plugin lifecycle state machine in `crates/ucil-daemon/src/plugin_manager.rs` has only the
  HOT/COLD subset (`Active ↔ Idle`). Phase 2 must drive the remaining transitions
  (`Discovered → Registered → Loading → Active`, `* → Stopped`, `* → Error`) with logged
  transitions per master-plan §15.2 (`ucil.<layer>.<op>` span naming).
- Frozen test selectors per DEC-0007: `plugin_manager::test_manifest_parser` and
  `plugin_manager::test_lifecycle_state_machine` MUST land at the module root (NOT inside a
  `mod tests { }` wrapper) so nextest's path matches the feature-list selector.

## Standing rules for Phase 2 work-orders
1. **stdout pristine on stdio MCP path** — tracing always writes to stderr in `ucil-daemon mcp`
   (carried from Phase 1 lessons); plugin runtime logs MUST honour the same constraint.
2. **No mocks of Serena, LSP, ast-grep, Probe, ripgrep, LanceDB, ONNX Runtime** — integration
   tests use real subprocesses or the Docker fixtures.
3. **Frozen `#[test]` selectors** — every acceptance selector in `feature-list.json` must match
   exactly the path nextest reports. Wrap or unwrap `mod tests { }` accordingly. Cite DEC-0007
   when re-locating.
4. **No regressions on Phase 1 acceptance** — `cargo test --workspace --no-fail-fast` stays
   green at every WO boundary; verifier reruns the Phase 1 gate scripts.

# Lessons Learned Log

(Seeded by planner; appended by docs-writer after each WO merges.)

## Lessons Learned (WO-0042 — plugin-manifest-and-lifecycle-statemachine)

**Features**: P2-W6-F01 (manifest parser), P2-W6-F02 (lifecycle state machine)
**Rejections**: 0 (verifier-green on first attempt)
**Critic blockers**: none — two soft warnings on commit cadence (`7c272a4` +171 LOC, `c0de35f` +116 LOC), both ineligible for in-place fix post-push, both covered by DEC-0005 spirit (≈45% rustdoc inflation)
**ADRs raised**: none
**Coverage**: 89.68% line for `ucil-daemon` (+4.68 pp above 85% floor); `plugin_manager.rs` 87.71% line / 88.06% function

### What worked

- **Single-file-edit blast radius.** Whole WO touched exactly one source file (`crates/ucil-daemon/src/plugin_manager.rs`, +681/-5) plus the ready-for-review marker. The 35-entry `forbidden_paths` list made the executor's scope unambiguous and trivially auditable by the critic. No cross-module surprise.
- **Pre-baked mutation checks in `acceptance`.** Two function-body mutations named explicitly (`PluginRuntime::register` body → no-op; `PluginManifest::validate` body → `Ok(())`) so the verifier did not have to invent the mutation patch. Both stashes failed on the targeted assertions; both restores went green. This is the correct pattern when DEC-0007 has removed the per-WO cargo-mutants gate — pre-baked function-body mutations are the authoritative anti-laziness layer.
- **Module-root frozen-selector placement (DEC-0007).** Both new tests landed at the module root of `plugin_manager.rs` (NOT inside `mod tests {}`), exactly matching the feature-list selectors `plugin_manager::test_manifest_parser` and `plugin_manager::test_lifecycle_state_machine`. Same pattern as the existing `test_hot_cold_lifecycle`. Zero selector drift between feature-list and nextest output.
- **Backward-compat via `#[serde(default)]` on every new manifest section** (`CapabilitiesSection`, `ResourcesSection`, `LifecycleSection`). Existing minimal manifests in `tests/plugin_manager.rs` (only `[plugin]` + `[transport]`) parsed unchanged. Acceptance criterion AC06 ran the existing integration test as an explicit regression guard — `3 passed; 0 failed` confirms the invariant.
- **Centralised `log_transition` helper** for tracing emission. Every transition method routed through one private helper, so the `target = "ucil.plugin.lifecycle"` and `plugin / from / to` field set stayed consistent across `register / mark_loading / mark_active / stop / mark_error`. Master-plan §15.2 (`ucil.<layer>.<op>`) compliance through one call site, not five.
- **Cargo-test summary-line regex with alternation** (`grep -Eq 'test result: ok\. ... 0 failed|... tests run: ... passed'`) matched both `cargo test` and `cargo nextest` shapes — lesson carried from WO-0038/WO-0039 retries continues to hold.

### What to carry forward

**For planner**:
- WOs introducing a state machine SHOULD pre-bake mutation checks naming specific transition methods to stash. The planner already knows which lines are load-bearing; the verifier should not have to discover them. Use the WO-0042 `acceptance` block as the template (one mutation per feature, with the expected-failure assertion line cited).
- WOs extending a TOML schema with new sections MUST include an acceptance_criterion that runs the EXISTING integration test using the OLD schema (here: `cargo test -p ucil-daemon --test plugin_manager`). Forces backward-compat to be tested mechanically, not assumed.
- **P2-W6-F07 (`ucil plugin` CLI subcommand) MUST add explicit `pub use` lines** to `crates/ucil-daemon/src/lib.rs` for `CapabilitiesSection`, `ActivationSection`, `ResourcesSection`. WO-0042 deliberately deferred this (no consumer in scope), and the existing `lib.rs:105–109` re-export is a named list, not a glob. Plan that surface change explicitly into the F07 WO so it's not surprise scope.

**For executor**:
- When a feature commit exceeds the ~50 LOC soft target by mostly rustdoc (≈45% of diff), DEC-0005 spirit covers it — but FLAG it in the ready-for-review note so the critic doesn't have to discover it. Cleaner split for the next state-machine-style WO: (1) error variant + helper, (2) happy-path transitions, (3) catch-all error-state transition.
- Crate-level `#![deny(rustdoc::broken_intra_doc_links)]` (set by Phase 1) means rustdoc additions MUST use plain backticks or fully qualified intra-doc links only — shorthand `[Foo]` will fail `cargo doc`. Cumulative lesson from WO-0009 / WO-0024 / WO-0025 / WO-0027 / WO-0039 rejections.
- **When adding fields to an existing struct used by tests, every struct literal in `mod tests {}` MUST be updated with the new field initialiser.** WO-0042 added `error_message: Option<String>` to `PluginRuntime`; two test-body literals at `plugin_manager.rs:1697` and `:1754` needed the additive `capabilities: CapabilitiesSection::default(), resources: None` shape. Missing one breaks the test build invisibly until the verifier's clean-slate run catches it.

**For verifier**:
- `scripts/verify/coverage-gate.sh` REMAINS BROKEN (RUSTC_WRAPPER + corrupt-header profraw issue carried from WO-0039 retry-1). Documented workaround: `env -u RUSTC_WRAPPER cargo llvm-cov --package <crate> --summary-only --json` restores correct numbers (here: 5,686 instrumented lines vs. the script's 249). Keep applying until escalation `20260419-0152-monitor-phase1-gate-red-integration-gaps.md` is resolved.
- `scripts/reality-check.sh <FEATURE>` reports script-level "FAILURE" on multi-feature WOs where multiple feature-trailed commits touch the same file. The script picks the NEWEST commit and rolls the file to its parent, which still contains all the feature implementation. This is a script limitation, not a feature defect. Pre-baked function-body mutation checks (per the WO `acceptance` block) provide tighter, authoritative coverage. Same handling as WO-0040 / WO-0041.
- **State-machine acceptance test checklist** (template for siblings P2-W6-F03..F08): (a) every `transition() → state` assertion uses `assert_eq!(runtime.state, PluginState::X)` not `matches!()` so panics print the actual state; (b) at least one illegal-transition assertion using `expect_err` + `match` arm pinning `IllegalTransition { from, to }` to specific `(PluginState, PluginState)` pairs; (c) `error_message` field check via `.as_deref() == Some("...")`; (d) NO global tracing-subscriber install (avoids cross-test contamination — assert state directly).

### Technical debt incurred

- `lib.rs` re-exports for the three new manifest types (`CapabilitiesSection`, `ActivationSection`, `ResourcesSection`) deferred to the `ucil plugin` CLI WO (P2-W6-F07). Documented in WO-0042 ready-for-review note (lines 80–105).
- Local `Duration as Dur` alias inside `test_lifecycle_state_machine` (line 1480) shadows the module-scope `std::time::Duration` import. Cosmetic; no follow-up.

## Lessons Learned (WO-0043 — plugin-hot-reload-and-circuit-breakers)

**Features**: P2-W6-F03 (hot-reload), P2-W6-F04 (circuit breakers)
**Rejections**: 0 (verifier-green on first attempt)
**Critic blockers**: none — two soft warnings (5/11 commit subjects over 70-char soft cap; `PluginManager::add` silently degrades on `try_write` contention — explicitly authorised by `scope_in[3]`)
**ADRs raised**: none — existing DEC-0005 (module-root flat-test commits) and DEC-0007 (frozen-selector module-root placement) covered the entire WO
**Coverage**: 89.49% line for `ucil-daemon` (+4.49 pp above 85% floor); `plugin_manager.rs` at 87.18% line / 86.81% function (slightly down from WO-0042 because the new `add` contention branch + writer-guard scope are not exercised by the sequential acceptance tests)

### What worked

- **Builder-injected timing constant**. The new `PluginManager::with_circuit_breaker_base(Duration)` builder collapses the production `MAX_RESTARTS × {1s, 2s, 4s} = 7s` wall to `{5ms, 10ms, 20ms} ≈ 35ms` in `test_circuit_breaker` while production code keeps the 1 s `pub const`. Test asserts BOTH directions: `elapsed >= 35ms` (proves backoff occurred — behavior invariant) AND `elapsed < 2s` (fast-test ceiling). The dual-bound assertion is what makes this pattern reusable: the lower bound is feature-specific, the upper bound is the fast-test contract.
- **Real-ENOENT for failure-branch coverage** instead of a second mock binary. `test_circuit_breaker` uses `transport.command = "/__ucil_test_nonexistent_breaker_binary__"` so `tokio::process::Command::spawn` returns `PluginError::Spawn { source: ENOENT }` through the real codepath. No second `mock-mcp-plugin-fail` binary needed; forbidden-paths list shrinks; the failure branch is genuinely exercised end-to-end. Same dispatcher, same error variant, no `#[cfg(test)]`-only branches in production code.
- **Pre-baked function-body mutation entries (round 2)**. The WO `acceptance[14..15]` named exactly which two function bodies to stash (`PluginManager::reload` body → `Ok(())`; `PluginManager::restart_with_backoff` body → `Ok(())`); both stashes failed at the prescribed assertion line (1630 panic on `elapsed >= 100ms`; 1706 panic on the `CircuitBreakerOpen { .. }` match), both restores green. Confirms the WO-0042 pattern as the authoritative anti-laziness layer post-DEC-0007. Verifier never had to invent a mutation patch.
- **Single-file blast radius held a second time**. Whole WO touched `crates/ucil-daemon/src/plugin_manager.rs` + a 2-symbol re-export in `lib.rs` + the ready-for-review marker. 35-entry `forbidden_paths` list (same template as WO-0042 plus `crates/ucil-daemon/src/bin/**`) made the executor's scope unambiguous. Critic re-verified with `git diff --name-only` and accepted in one pass.
- **Six explicit Phase-1/Phase-2 regression sub-checks**. Acceptance criteria run `test_hot_cold_lifecycle`, `test_manifest_parser`, `test_lifecycle_state_machine`, `--test plugin_manager`, `--test e2e_mcp_stdio`, `--test e2e_mcp_with_kg` as named entries — not "the whole workspace passes" hand-wave. Mechanical regression discipline carried from WO-0042; verifier ran each individually from clean slate.
- **No deferred re-export debt this time**. WO-0042 deferred lib.rs surface for `CapabilitiesSection`/`ActivationSection`/`ResourcesSection`; WO-0043 included `MAX_RESTARTS` and `CIRCUIT_BREAKER_BASE_BACKOFF_MS` re-exports in the same WO. Cumulative-debt avoidance worked — P2-W6-F07 will not inherit two deferred export piles.

### What to carry forward

**For planner**:
- WOs introducing time-dependent behavior (sleep / timeout / backoff / retry-with-delay) MUST specify a `with_<config>_base(Duration)` builder on the orchestrator type so the acceptance test can compress production seconds to milliseconds. Use `with_circuit_breaker_base` as the template. Production code keeps the `pub const`; the builder is a `mut self` chain so the default constructor is unchanged.
- The WO-0042 lesson "additive struct fields require updating every literal in `mod tests {}`" needs nuance: it ONLY applies when the struct is constructed via `PluginRuntime { ... }` literals. WO-0043 added `in_flight` + `restart_attempts` to `PluginRuntime`, but the existing `test_hot_cold_lifecycle` constructs through `mgr.activate(...)` (factory path, not literal), so zero literal fix-ups were required. Planner pre-flight should grep for `<StructName> {` (literal sites) vs. construction via factory methods, and only require fix-ups when literals exist. Without this distinction, scope_in carries phantom work.
- `PluginManager::add` ships with a silent `try_write` contention fall-through (warn-log + drop the registration). Approved by WO-0043 `scope_in[3]` but the critic flagged it as soft concern (lines 1382-1393): production callers cannot programmatically detect the no-op. Future "register pre-built X" surfaces (likely lands in P2-W6-F07 `ucil plugin install <name>`) should return `Result<(), AlreadyRegistered | LockContention>` so the no-op case is observable. Plan that contract into F07 explicitly.
- For multi-feature WOs that share a single source file, expect `scripts/reality-check.sh` to return script-level "FAILURE" — same handling as WO-0040 / WO-0041 / WO-0042 / WO-0043 (now 4 WOs in a row). Pre-baked function-body mutations in `acceptance` are the authoritative replacement; do not waste planner cycles trying to fix the script per-feature.

**For executor**:
- Two clippy lints to expect when adding async lock-handling code, both flagged on this WO:
  1. `clippy::significant_drop_tightening` — DO NOT bind a guard with `let g = runtimes.read().await;`. Consume inline at the expression boundary so the guard drops at end-of-statement: `let snapshot = runtimes.read().await.iter().find(...).cloned();`. WO-0043 fixed this in `reload` and `restart_with_backoff`.
  2. `clippy::single_match_else` — single-arm `match` over a 2-variant `Result` should be `if .is_ok() { ... }; sleep(...);` not `match { Ok => return, Err => sleep }`. WO-0043 hit this in the `restart_with_backoff` retry loop.
- `clippy::doc_markdown` rejects bare identifiers in rustdoc paragraphs. Every constant / type / function / lint identifier MUST be in backticks (e.g. `` `MAX_RESTARTS` ``, NOT `MAX_RESTARTS`). WO-0043 commit `257a1e1` was a follow-up fix exactly because one bare `MAX_RESTARTS` slipped into a `# Examples` block. Pre-flight grep: `rg -nE '^\s*///.*\b[A-Z][A-Z_0-9]+\b' <file>` to find bare-uppercase identifiers in doc comments before pushing — easier than commit-by-commit chase.
- DEC-0005 module-root flat-test commits continue to clear critic with no warning even at 139 LOC (`528eb6a` `test_hot_reload`) and 91 LOC (`934243b` `test_circuit_breaker`). Continue using the pattern; do NOT pre-emptively split a single-test commit into "skeleton + assertions" unless it crosses the 200-line hard threshold.

**For verifier**:
- The `RUSTC_WRAPPER=sccache` + corrupt-header profraw coverage-gate workaround is now in its 6th consecutive WO (WO-0039 retry-1 → WO-0040 → WO-0041 → WO-0042 → WO-0043). Until escalation `20260419-0152-monitor-phase1-gate-red-integration-gaps.md` resolves, the standard workflow is: `env -u RUSTC_WRAPPER cargo llvm-cov --package <crate> --summary-only --json` plus a manual zero-byte + corrupt-header profraw prune between `cargo test` and `cargo llvm-cov report`. Treat as standing protocol, not per-WO discovery.
- **State-machine acceptance test checklist** (extends the WO-0042 template) now also requires:
  - Asymmetric time-bound assertions on tests that exercise sleep/backoff: BOTH a lower behavior bound (`elapsed >= X` proving the delay actually happened) AND an upper fast-test bound (`elapsed < 2s` proving production constants didn't leak). Reject if only one direction is asserted.
  - Failure-branch tests use real-ENOENT command paths (or equivalent real-error triggers) rather than mocked `Command::spawn`. Search the diff for `mock_command|MockCommand|spawn_mock` — if present without an existing `mock-mcp-plugin`-style real binary fixture, treat as a forbidden mock of `tokio::process::Command`.

### Technical debt incurred

- **`PluginManager::add` silent contention fall-through** (`plugin_manager.rs:1382-1393`). Returns `()` on `try_write` failure with only a `tracing::warn!` event. Critic-flagged as soft concern; explicitly approved by WO scope_in. Follow-up: tighten to `Result<(), PluginError::AlreadyRegistered | PluginError::LockContention>` when P2-W6-F07 (`ucil plugin install`) lands, so the CLI can surface the no-op to operators.
- **Coverage hot spots not exercised**: the writer-guard `Drop` ordering inside `reload` and the contended-`try_write` branch in `add` are not covered by sequential acceptance tests. `plugin_manager.rs` line coverage held at 87.18% (above floor) but specific branch holes will widen as we add more concurrent-call tests; consider a multi-task contention test in a future WO if the coverage floor moves above 90%.

## Lessons Learned (WO-0044 — ast-grep-probe-plugin-manifests)

**Features**: P2-W6-F05 (ast-grep manifest), P2-W6-F06 (probe manifest)
**Rejections**: 0 (verifier-green on first attempt)
**Critic blockers**: none — three soft warnings (probe pinned to `0.6.0-rc315` RC tag with documented justification, `health_check_with_timeout(90s)` instead of bare `health_check(5s)` for cold-cache `npx -y` fetches, two commits over the 50-LOC soft target both authorised by DEC-0005 module-coherence)
**ADRs raised**: none — existing DEC-0005 (module-coherence test/script commits) and DEC-0007 (frozen-selector module-root placement) covered the entire WO; first WO that ships ZERO Rust source-code changes (`crates/ucil-daemon/src/**` blanket-banned in `forbidden_paths`)
**Coverage**: 89.51% line for `ucil-daemon` (+4.51 pp above 85% floor); `plugin_manager.rs` held at 87.18% line — new tests exercise the spawn → JSON-RPC `tools/list` path against two new real subprocess targets without modifying the file under test

### What worked

- **Zero-source-change WO**. Whole WO touched two new manifest TOMLs, one new integration test file, two devtools install scripts, and two verify-script stub rewrites — and ZERO source files under `crates/ucil-daemon/src/**`. The 35-entry `forbidden_paths` list with `crates/ucil-daemon/src/**` as a blanket entry made this mechanically enforced; the executor literally could not touch the parser or lifecycle code. First time this shape has been used and it cleared critic in one pass.
- **Reproducibility comment block in the manifest itself**. Both new TOMLs lead with a structured comment block citing upstream URL, pinned npm tag, MCP-server version, advertised-tool list, and a forward-revisit clause ("supersede via ADR when upstream ships first-party MCP"). Same shape as `plugins/structural/serena/plugin.toml:1-14`. Critic re-verified `! grep -q '"main"'` mechanically — no moving refs slipped in.
- **Pre-baked `[transport].command` mutation checks (round 3)**. WO `acceptance[19..20]` named exactly which manifest field to mutate (`command = "npx"` → `"/__ucil_test_nonexistent_<bin>_binary__"`), the expected panic shape (`PluginError::Spawn { command, source: NotFound }`), and the panic line (`tests/plugin_manifests.rs:88` / `:128`). Verifier panic output matched character-for-character on both mutations. Confirms the WO-0042 / WO-0043 pattern as the authoritative anti-laziness layer — pre-baked manifest-field mutations are even cleaner than function-body mutations because the diff is one literal-string swap, no Rust syntax knowledge needed.
- **Real-binary spawn end-to-end**. Tests call `PluginManager::health_check_with_timeout(&manifest, 90_000)` which `tokio::process::Command::spawn`'s the real `npx -y @notprolands/ast-grep-mcp@1.1.1` / `npx -y @probelabs/probe@0.6.0-rc315 mcp` subprocess and exchanges real JSON-RPC `initialize → notifications/initialized → tools/list`. No `tokio::process::Command` shim, no fake `Child`, no second mock binary. The single `mock` token in the diff is a PROHIBITION in the docstring at `tests/plugin_manifests.rs:10`, not actual use.
- **Tool-name pinning as drift sentinel**. Each test asserts a specific upstream-advertised tool name (`find_code` for ast-grep, `search_code` for probe) — if either upstream renames its tool, the test fails loudly rather than silently green-on-empty-list. The empty-list assertion alone (`!health.tools.is_empty()`) would not catch a rename; the explicit `health.tools.iter().any(|t| t == "find_code")` does.
- **Token-budget assertion technique for `probe`**. F06 verify script asserts BOTH that `probe search --max-tokens 4096` returned bounded output (`wc -c < log` < 16384 chars) AND that an `extract` follow-up actually contains the function body (`grep -q 'fn evaluate'`). Two-direction assertion: budget honoured (upper bound) AND content present (lower bound on usefulness). Same dual-bound discipline as WO-0043's `elapsed >= 35ms && elapsed < 2s` — reusable pattern for any "constrained but non-empty" surface.
- **Operator-actionable failure paths**. Both verify scripts emit `[FAIL] P2-W6-F0X: <reason>` and reference `scripts/devtools/install-<bin>.sh` if the binary is absent — verifier sees "ast-grep not on PATH; run scripts/devtools/install-ast-grep.sh" rather than a cryptic `command not found`. Carries the convention forward from WO-0042 / WO-0043.

### What to carry forward

**For planner**:
- For ANY future WO that introduces an external-binary plugin manifest (npx/uvx/cargo-install + pinned upstream tool), require in `scope_in`: (a) a leading TOML comment block citing upstream URL + pinned tag + advertised-tool-name list + revisit clause, (b) `! grep -q '"main"'` in `acceptance_criteria` to mechanically verify no moving refs, (c) ONE pre-baked `[transport].command` mutation check per manifest with the panic shape `PluginError::Spawn { command, source: NotFound }` named exactly. WO-0044 `acceptance[19..20]` is the canonical template.
- WOs whose tests spawn `npx -y <pkg>` MUST specify `health_check_with_timeout(manifest, 90_000)` rather than bare `health_check(5s)` in `scope_in` — cold-cache npm fetches need 30-90 s, second runs hit the npx cache and complete in 1-3 s. Pre-specifying avoids an executor judgment call (which produced a warning on this WO even though the call was right).
- Fixture symbol anchors are now load-bearing across multiple verify scripts: `class TaskManager` (`tests/fixtures/typescript-project/src/task-manager.ts:133`) for ts-fixture probes, `pub fn evaluate` (`tests/fixtures/rust-project/src/util.rs:128`) for rs-fixture probes. Sibling P2-W6-F08 (lifecycle integration suite) and any future plugin-smoke WO should reuse these. If a future fixture-refresh WO renames either symbol, multiple verify scripts break in lockstep — flag the dependency in the rename WO's `scope_out`.
- Cumulative-debt avoidance now confirmed across THREE consecutive WOs (WO-0042 deferred; WO-0043 + WO-0044 cleared). Pattern: lib.rs re-exports for any new `pub` types must land in the SAME WO that introduces them, not "deferred to the consumer WO". P2-W6-F07 (`ucil plugin` CLI) thus inherits zero deferred surface.
- Zero-source-change WO is now a known, recommended shape for plugin-manifest landings — `forbidden_paths` blanket-bans `crates/<crate>/src/**` so the executor cannot accidentally touch the file under test. Reuse this shape for the structural plugin family (joern, scip when they land) and any subsequent search/quality plugin manifests.

**For executor**:
- When upstream first-party tool lacks an MCP subcommand, the canonical pattern is community-wrapper-via-pinned-npm-tag plus a leading TOML comment block documenting the choice. WO-0044 ast-grep used `@notprolands/ast-grep-mcp@1.1.1` (vs. nonexistent first-party `ast-grep mcp` at upstream 0.42.1); probe used `@probelabs/probe@0.6.0-rc315` (the `@buger/probe-mcp` predecessor explicitly redirects to this). Cite the upstream URL, the redirect chain (if any), the advertised-tool list, and a forward "supersede via ADR when upstream ships first-party MCP" clause. The next WO that lands a similar plugin should follow this template verbatim.
- Verify-script smoke commands MUST ground on a confirmed real fixture symbol — read the fixture source first, do NOT modify the fixture (`tests/fixtures/**` is in every plugin-manifest WO's `forbidden_paths`). Grep technique: `rg -nE 'pub fn|class |export class |export function' tests/fixtures/<lang>-project/src/` to enumerate candidate anchor symbols before writing the verify script.
- When `cargo clippy -p ucil-daemon --all-targets -- -D warnings` runs against a NEW integration test file with NO source-code changes, expect zero clippy hits (the file is fresh, no inherited debt). If clippy DOES fire, the lint is in the test file itself — most likely candidates remain `clippy::doc_markdown` (bare uppercase identifiers in doc comments) and `clippy::single_match_else`. WO-0044 had zero clippy hits, confirming the WO-0043 pre-flight grep `rg -nE '^\s*///.*\b[A-Z][A-Z_0-9]+\b'` is sufficient pre-emption when applied.
- Test-file docstring as no-mock prohibition: WO-0044 used `//! Mocking ... is forbidden — the WO-0044 contract is precisely that real MCP-server subprocesses speak real JSON-RPC over stdio` in `tests/plugin_manifests.rs:10`. Critic detected the `mock` token, recognised the prohibition shape, and cleared. Continue using this docstring pattern in any future no-mock-required test file — it's both reader-facing documentation AND a self-test against scope creep.

**For verifier**:
- `UCIL_SKIP_EXTERNAL_PLUGIN_TESTS` env-var opt-out for `plugin_manifests::*` and any future external-binary-plugin tests is a LEGITIMATE runtime skip that MUST stay UNSET in the verifier shell. Add to the standing pre-flight checklist next to the `RUSTC_WRAPPER`-unset-for-coverage workaround. Pre-flight: `env | grep UCIL_SKIP_EXTERNAL_PLUGIN_TESTS` should return nothing before any `cargo test`.
- For external-binary plugin tests, `npx -y` first-run can take 30-90 s on cold cache, dropping to 1-3 s steady-state. If the verifier sees a 5-30 s timeout failure on a fresh shell, suspect a partial npm cache rather than a real failure — pre-warm via `npx -y @<pkg>@<version> --version` (or `--help` if `--version` is unsupported) before re-running. Document any pre-warm in the verification report.
- Mutation check checklist for plugin-manifest WOs (extends the WO-0042 / WO-0043 templates): (a) every manifest with a `[transport].command` field MUST have a paired mutation entry replacing the field with `/__ucil_test_nonexistent_<bin>_binary__`; (b) the expected panic shape is `PluginError::Spawn { command: "<the path>", source: Os { code: 2, kind: NotFound, message: "No such file or directory" } }`; (c) restoration via `git checkout -- <manifest-path>` (NOT `git stash pop` — manifest mutations are committed-file edits, not staged changes); (d) `git status --short` must be clean between mutation steps so a stale mutation cannot bleed into a sibling test.
- The `RUSTC_WRAPPER=sccache` + corrupt-header profraw coverage-gate workaround is now in its 7th consecutive WO (WO-0039 retry-1 → WO-0040 → WO-0041 → WO-0042 → WO-0043 → WO-0044). Until escalation `20260419-0152-monitor-phase1-gate-red-integration-gaps.md` resolves, treat as standing protocol: `env -u RUSTC_WRAPPER cargo llvm-cov --package <crate> --summary-only --json` plus manual zero-byte + corrupt-header `.profraw` prune between `cargo test` and `cargo llvm-cov report`.

### Technical debt incurred

- **probe pinned to `0.6.0-rc315`** (release-candidate tag, not stable). The pin is to an immutable npm tag, not a moving `latest` / `main`, so reproducibility holds. Justified in `plugins/search/probe/plugin.toml:1-30` leading comment block: upstream has not yet cut a stable 0.6.0; `latest` resolves to `0.6.0-rc315`. **Follow-up trigger**: when upstream cuts stable `0.6.0` (or higher), supersede the manifest pin via a fix-WO with an ADR documenting the upgrade.
- **Fixture symbol anchors becoming load-bearing across verify scripts**. `class TaskManager` and `pub fn evaluate` are now referenced by both `scripts/verify/P2-W6-F05.sh` (ast-grep) and `scripts/verify/P2-W6-F06.sh` (probe), and will likely be referenced by sibling P2-W6-F08 (lifecycle integration suite) and any future plugin-smoke WO. **Follow-up trigger**: if a future fixture-refresh WO touches `tests/fixtures/typescript-project/src/task-manager.ts` or `tests/fixtures/rust-project/src/util.rs`, audit `scripts/verify/P2-W6-*.sh` and `tests/integration/**` for symbol-name dependencies before merge.
- **`@notprolands/ast-grep-mcp@1.1.1` is a community wrapper, not first-party ast-grep MCP**. Upstream ast-grep CLI 0.42.1 ships subcommands `run | scan | test | new | lsp | completions` only — no `mcp`. **Follow-up trigger**: when upstream ast-grep ships first-party `ast-grep mcp`, supersede via ADR + fix-WO swapping the manifest's `transport.command` from the npm wrapper to the first-party invocation. The leading TOML comment block already names this revisit.
