# WO-0077 — Ready for Review

**Work-order**: `ucil-build/work-orders/0077-mcp-pytest-runner-plugin-manifest.json`
**Feature**: `P3-W11-F08` (mcp-pytest-runner G8 testing manifest)
**Branch**: `feat/WO-0077-mcp-pytest-runner-plugin-manifest`
**Final commit**: `8342633facc6eb7fd9807c1ff6958b3def4f455d`
**Author session**: executor (Opus 4.7)

## Commit log (4 atomic commits, 0 merges; pushed to origin)

```
8342633 test(verify): replace P3-W11-F08 TODO stub with real smoke
71b8158 test(daemon): add g8_plugin_manifests health-check for mcp-pytest-runner
b4682f8 feat(devtools): add install-mcp-pytest-runner.sh warm-up script
ba2f5d2 feat(plugins-testing): add mcp-pytest-runner@0.2.1 plugin manifest
```

All commits Conventional-Commit-formatted with `Phase: 3` + `Feature: P3-W11-F08` + `Work-order: WO-0077` trailers.

## What I verified locally

### Source-side acceptance

- `test -f plugins/testing/mcp-pytest-runner/plugin.toml` — ✓
- `python3 -c 'import tomllib; ...'` manifest sanity — ✓ (`plugin.name == "mcp-pytest-runner"`, `plugin.category == "testing"`, `transport.type == "stdio"`, `capabilities.provides` = `["testing.pytest.discover", "testing.pytest.run", "testing.pytest.run_node_id"]`)
- `! grep -qE '"(main|latest|master|head|develop|dev|nightly)"' plugins/testing/mcp-pytest-runner/plugin.toml` — ✓ (immutable `0.2.1` pin; no moving tags)
- `test -x scripts/devtools/install-mcp-pytest-runner.sh` — ✓
- `bash scripts/devtools/install-mcp-pytest-runner.sh; test $? -eq 0` — ✓ (idempotent; warms uvx cache; exits 0 on success)
- `test -f crates/ucil-daemon/tests/g8_plugin_manifests.rs` — ✓
- `grep -Eq '^[[:space:]]*(pub )?mod g8_plugin_manifests[[:space:]]*\{' …rs` — ✓ (module-root placement per DEC-0007)
- `grep -c '#\[tokio::test\]' …rs` — `1` (≥ 1) ✓
- `! grep -qE 'std::process::Command' crates/ucil-daemon/tests/g8_plugin_manifests.rs` — ✓ (the rustdoc was rephrased to avoid the literal string per WO-0075/0076 W1 lesson; tokio variant idiomatic in async paths)
- `! grep -qiE 'mock|fake|stub' plugins/testing/mcp-pytest-runner/plugin.toml scripts/devtools/install-mcp-pytest-runner.sh scripts/verify/P3-W11-F08.sh` — ✓
- `! grep -qE 'TODO: implement acceptance test for P3-W11-F08' scripts/verify/P3-W11-F08.sh` — ✓ (TODO stub replaced)

### Cargo / clippy / fmt / coverage

- `cargo build -p ucil-daemon` — ✓ (clean; 1m32s cold, 1.7s warm)
- `cargo clippy -p ucil-daemon --tests --all-targets -- -D warnings` — ✓ (0 warnings)
- `cargo fmt --all -- --check` — ✓
- `cargo test -p ucil-daemon --no-fail-fast` — ✓ (14 test suites green; selected groups: 164 unit + 27 doctest + 9 main + 1 e2e_mcp_stdio + 1 e2e_mcp_with_kg + 2 g3 + 1 g4 + 2 g5 + 3 g6 + 2 g7 + 1 g8 + 3 plugin_manager + 3 plugin_manifests = 219 total)
- `cargo test -p ucil-daemon --test g8_plugin_manifests g8_plugin_manifests::mcp_pytest_runner_manifest_health_check` — `test result: ok. 1 passed; 0 failed` ✓
- `bash scripts/verify/P3-W11-F08.sh` — `[OK] P3-W11-F08` (cargo + tools/list + discover_tests + execute_tests selective re-run all green)

### AC23 substantive coverage

- `env -u RUSTC_WRAPPER cargo llvm-cov --package ucil-daemon --tests --summary-only --json | jq '.data[0].totals.lines.percent'` — `89.65267727930537` (≥ 85.0 floor per master plan §15.4)

### git-log discipline

- `git log feat/WO-0077-… ^main --merges | wc -l` — `0` ✓
- `git log feat/WO-0077-… ^main --pretty=format:%B | grep -Eq 'Work-order: WO-0077'` — ✓
- `git log feat/WO-0077-… ^main --pretty=format:%B | grep -Eq 'Feature: P3-W11-F08'` — ✓

### Forbidden-paths audits (all 24 paths empty diff)

- `crates/ucil-daemon/src/` — clean (no daemon-source changes; this WO is manifests-only)
- `crates/ucil-core/src/` — clean
- `tests/fixtures/` — clean (read-only fixture; tmpdir copy is in-test/in-script only)
- `ucil-build/feature-list.json`, `ucil-build/feature-list.schema.json`, `ucil-master-plan-v2.1-final.md` — clean
- `ucil-build/decisions/` — clean (DEC-0021 was authored by the planner alongside this WO; no executor-side ADRs)
- `crates/ucil-daemon/tests/g{3,4,5,6,7}_plugin_manifests.rs`, `tests/plugin_manifests.rs` — clean
- `plugins/{architecture,context,knowledge,platform,quality,search,structural}/` — clean
- `scripts/verify/P3-W11-F07.sh` — clean (F07 is deferred per DEC-0021; TODO stub remains)
- `.githooks/`, `scripts/gate/`, `scripts/flip-feature.sh` — clean

## Live tool name capture

Captured 2026-05-08 directly from `uvx mcp-pytest-runner@0.2.1` via the live `initialize` → `notifications/initialized` → `tools/list` JSON-RPC handshake.

**serverInfo** — `{"name": "pytest-mcp", "version": "0.2.1"}` (note: the PyPI package name is `mcp-pytest-runner`, the upstream's serverInfo.name is `pytest-mcp`; both documented in the manifest's top-of-file rustdoc).

**Advertised tools (2 total, snake_case as emitted by upstream)**:

1. **`execute_tests`** — `"Execute pytest tests with filtering and output options"`
   - `inputSchema`: `{node_ids?: string[], markers?: string, keywords?: string, verbosity?: int (-2..2), failfast?: bool, maxfail?: int, show_capture?: bool, timeout?: int}`
2. **`discover_tests`** — `"Discover available tests in the project"`
   - `inputSchema`: `{path?: string, pattern?: string}`

**Chosen M2 assertion target** — `discover_tests` (per WO-0077 scope_in #11 / planner's hint: "pytest discovery is the canonical pytest entry point per F08 spec").

The verify script's tool-level smoke additionally requires both `discover_tests` AND `execute_tests` in the `tools/list` reply (load-bearing for the F08 spec line "pytest hierarchical test discovery and selective re-run by node ID").

**Discovered-test entry shape** (load-bearing for the F08 spec):

```json
{
  "node_id": "tests/test_evaluator.py::test_evaluate_arithmetic",
  "module":  "tests.test_evaluator",
  "class_":  null,
  "function":"test_evaluate_arithmetic",
  "file":    "tests/test_evaluator.py",
  "line":    null
}
```

The `node_id` field is the canonical pytest selector — verified for all 159 discovered tests in the python-project fixture (after fabricating `conftest.py` to put `src/` on `sys.path`).

**Execute response shape** (selective re-run smoke; subset of 3 from `test_evaluator.py`):

```
exit_code=0, summary={total:3, passed:3, failed:0, skipped:0, errors:0}
```

## M1 mutation contract — transport.command poison

**Pre-mutation md5**: `9d23c419983f5f675aeda44e5e9a87ca  plugins/testing/mcp-pytest-runner/plugin.toml`

**Mutation**: edited `[transport]` section, replaced `command = "uvx"` with `command = "/__ucil_test_nonexistent_M1__"`.

**Run**: `cargo test -p ucil-daemon --test g8_plugin_manifests g8_plugin_manifests::mcp_pytest_runner_manifest_health_check`.

**Observed panic** (verbatim, line 162):

```
health-check mcp-pytest-runner MCP server: Spawn { command: "/__ucil_test_nonexistent_M1__", source: Os { code: 2, kind: NotFound, message: "No such file or directory" } }
```

— matches the WO-0072/0076 §10 verbatim shape (`PluginError::Spawn { source: Os { code: 2, kind: NotFound } }` chain).

**Restore**: `git checkout -- plugins/testing/mcp-pytest-runner/plugin.toml`

**Post-restore md5 confirm**: `md5sum -c /tmp/wo-0077-mcp-pytest-runner-orig.md5` → `OK`

**Post-restore re-run**: `cargo test … health_check` → `test result: ok. 1 passed; 0 failed`.

## M2 mutation contract — expected-tool-name regression

**Pre-mutation md5**: `001e36487b35bd72c486ea624c559ee4  crates/ucil-daemon/tests/g8_plugin_manifests.rs`

**Mutation**: edited the test file, replaced the `discover_tests` literal in `health.tools.iter().any(|t| t == "discover_tests")` with `health.tools.iter().any(|t| t == "NON_EXISTENT_TOOL_M2")` and updated the panic body's `want:` slot accordingly.

**Run**: `cargo test -p ucil-daemon --test g8_plugin_manifests g8_plugin_manifests::mcp_pytest_runner_manifest_health_check`.

**Observed panic** (verbatim, line 193, SA-numbered shape):

```
(SA1) expected `discover_tests` tool in advertised set; got: ["execute_tests", "discover_tests"]; want: "NON_EXISTENT_TOOL_M2"
```

— matches the WO-0072/0076 §11 verbatim shape with full live tool-list captured in the panic body for mutation-diagnosis trivia per DEC-0007.

**Restore**: `git checkout -- crates/ucil-daemon/tests/g8_plugin_manifests.rs`

**Post-restore md5 confirm**: `md5sum -c /tmp/wo-0077-g8tests-orig.md5` → `OK`

**Post-restore re-run**: `cargo test … health_check` → `test result: ok. 1 passed; 0 failed`.

## Lessons applied

| Lesson source | Lesson | Application in WO-0077 |
|---|---|---|
| WO-0076 §planner | G7 → G8 paired-manifest template port; SOLO-FEATURE shape per WO-0072 with deferred peer | Direct shape parent; F07 deferred per DEC-0021 mirrors F10 deferred per DEC-0019 in WO-0072 |
| WO-0076 §planner / DEC-0020 | Preemptive-deferral pattern (PyPI/npm/GitHub upstream-availability triad) | Applied via DEC-0021 (test-runner-mcp); chain DEC-0019 → DEC-0020 → DEC-0021 establishes the convention |
| WO-0076 scope_in #11 + WO-0077 scope_in #11 | Live tools/list capture at the pinned version BEFORE sealing manifest `[capabilities] provides` and the M2 assertion literal | Captured 2026-05-08; both `execute_tests` + `discover_tests` documented verbatim in plugin.toml top-of-file rustdoc; M2 target pinned on `discover_tests` |
| WO-0074 §executor #1 | Don't translate live tools/list names to snake_case when they ship snake_case (or kebab-case when kebab) | Upstream emits snake_case `execute_tests` / `discover_tests`; pinned literally |
| WO-0074 §executor #2 | Don't use `--mcp` as a devtools warm-up flag — use `--help` | Install script uses `uvx … --help` warm-up |
| WO-0074 §executor #3 | UCIL_SKIP_<GROUP>_PLUGIN_E2E env-var per-group opt-out, default RUN | `UCIL_SKIP_TESTING_PLUGIN_E2E` documented in test rustdoc + verify script |
| WO-0074/0076 §executor #4 | python polling with deadline > bash sleep for tools/call wall-time-sensitive smokes | Verify script uses python with 90 s / 120 s deadlines |
| WO-0074/0075/0076 §executor #5 | Copy `tests/fixtures/<project>` into mktemp -d tmpdir BEFORE invoking | Verify script copies python-project file-by-file with __pycache__ / .pytest_cache / .ruff_cache prune |
| WO-0075/0076 §executor W1 | Pre-emptive `! grep -qE 'std::process::Command'` AC; tokio process variant in async paths | AC enforced; rustdoc rephrased to avoid the literal string; idiomatic tokio surface used (the test does no direct spawning — `health_check_with_timeout` handles the subprocess internally) |
| WO-0075 §planner | Keep separate `g<N>_plugin_manifests.rs` peer file (not consolidated) | New `tests/g8_plugin_manifests.rs` peer of g3/g4/g5/g6/g7 |
| WO-0067/0068/0069/0070/0072/0073/0074/0075/0076 | Pre-bake M1/M2 mutation contracts in scope_in + AC + verifier-friendly restore-md5 protocol | M1 (transport.command) + M2 (expected-tool-name) both verified above |
| WO-0067/0068/0069/0070 / DEC-0007 | SA-numbered panic-body format (e.g. `(SA1) <semantic>; got: <full live list>; want: <literal>`) | `(SA1)` panic in the assertion body |
| WO-0067..WO-0076 | Substantive AC23 coverage standing protocol (`env -u RUSTC_WRAPPER cargo llvm-cov`) | 89.65% — above 85% floor |
| WO-0067..WO-0076 (scope_out) | AC30/AC31 phase-1/phase-2 effectiveness-gate flake carry-over with 3 standing escalations | Carried as scope_out #14 |
| WO-0070..WO-0076 | AC25 `git log feat ^main --merges = 0` workflow-timing tolerant | 0 merges on this branch ✓ |
| **DEC-0021 NEW** | P3-W11-F07 test-runner-mcp deferred to Phase 4/7 pending upstream — pre-flight upstream-availability sweep authored alongside this WO | Lineage cited in plugin.toml + install script + RFR; chain DEC-0019 → DEC-0020 → DEC-0021 |
| DEC-0005 (module-coherence carve-out) | >50-LOC plugin.toml + integration-test commits acceptable when load-bearing | Cited in commit bodies for plugin.toml (~150 LOC) + g8 test (~180 LOC) |

## Notes for the verifier

1. The verifier MUST NOT export `UCIL_SKIP_EXTERNAL_PLUGIN_TESTS` or `UCIL_SKIP_TESTING_PLUGIN_E2E` — both env vars must be unset when running `cargo test g8_plugin_manifests::*` and `bash scripts/verify/P3-W11-F08.sh` (per WO-0076 scope_in #12 carry-over).
2. The fixture `tests/fixtures/python-project` is read-only. The verify script and integration test BOTH copy it into a `mktemp -d` tmpdir before invoking the upstream binary; the fixture stays pristine across runs.
3. The verify script fabricates a `conftest.py` in the tmpdir copy that prepends `src/` to `sys.path` so pytest can import the `python_project` package without an editable install. This is in the tmpdir copy ONLY; the read-only fixture is untouched.
4. Cold-cache uvx fetches may take up to ~30 s for the very first invocation (uv resolves pytest, pluggy, anyio, etc.); subsequent runs are < 1 s. Both the integration test (`FIRST_RUN_TIMEOUT_MS = 120_000`) and verify script (90 s / 120 s deadlines) budget for this.
5. DEC-0021 was pre-authored by the planner alongside this WO; the executor authored no ADRs. The DEC-0019 → DEC-0020 → DEC-0021 chain documents the preemptive-deferral convention.
6. Standing escalation set (3 flakes carried since WO-0067):
   - `20260507T0357Z-effectiveness-nav-rust-symbol-rs-line-flake.md`
   - `20260507T1629Z-effectiveness-refactor-rename-python-fixture-missing-symbol.md`
   - `20260507T1930Z-effectiveness-nav-rust-symbol-doctest-caller-flake.md`

   All three are pre-existing scope_out entries; no new escalations introduced by this WO.

## Branch state

- Branch `feat/WO-0077-mcp-pytest-runner-plugin-manifest` exists locally + on origin.
- 4 commits ahead of `main`; 0 merges; tree clean; pushed.
- HEAD: `8342633facc6eb7fd9807c1ff6958b3def4f455d`.
