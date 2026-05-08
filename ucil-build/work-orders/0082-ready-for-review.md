---
work_order: WO-0082
slug: test-runner-mcp-revival
phase: 3
week: 11
features: ["P3-W11-F07"]
final_commit_sha: 93132a637f32cc039939504ec9cbaf799e71fdd9
branch: feat/WO-0082-test-runner-mcp-revival
ready_for_review: true
authored_by: executor
---

# WO-0082 — test-runner (G8 Testing+CI, P0) plugin manifest revival — Ready for Review

## Summary

Lands `plugins/testing/test-runner/plugin.toml` pinned to
`@iflow-mcp/mcp-test-runner@0.2.1` (the third-party scoped npm
mirror — the only published surface; SHA pinned defensively to
`d6ccbd99f3c9b599216e3d9f655b6cb22e33867f` per master-plan §13 +
DEC-0025 §Rationale). Re-emits the archived WO-0081 with all
DEC-0024 source-data errors corrected per DEC-0025.

| Path | Purpose |
|------|---------|
| `plugins/testing/test-runner/plugin.toml` | G8 Testing plugin manifest, advertises `testing.run` capability — single-tool dispatcher (DEC-0025 §Decision point 3) |
| `scripts/devtools/install-test-runner-mcp.sh` | Idempotent dev-helper that reports state + warms npx cache |
| `scripts/verify/P3-W11-F07.sh` | Replaces TODO-stub with end-to-end smoke (cargo test + INDEPENDENT JSON-RPC tools/list capture inspecting inputSchema framework enum) |
| `crates/ucil-daemon/tests/g8_plugin_manifests.rs` | Adds the new `test_runner_manifest_health_check` test inside the existing `mod g8_plugin_manifests { ... }` block (additive at the test-body level — file-level rustdoc amended ONLY per WO-0082 scope_in #8) |

## DEC-0021 → DEC-0024 → DEC-0025 chain-of-corrections narrative

This WO discharges the deferral chain (`DEC-0019 → DEC-0020 →
DEC-0021`; graphiti / ruff / test-runner-mcp) by completing the
revival chain (`DEC-0022 → DEC-0023 → DEC-0024+DEC-0025`) for
the third (and final) deferred manifest. WO-0082 is the **first
verifier-eligible attempt** for F07 — the WO-0081 executor halt
with empirical-validation escalation does NOT increment the
attempts counter per DEC-0025 §Consequences (`attempts=0`).

### Chain summary

- **DEC-0021** (2026-05-04, accepted): Defer `test-runner-mcp`
  from Phase 3 to Phase 4/7 hardening. Pre-flight upstream-
  availability sweep (PyPI-404 + npm-search + GitHub-search
  triad) found no canonical MCP server published under the bare
  `test-runner-mcp` name; the planner's revisit trigger demanded
  a canonical upstream emerge before revival.

- **DEC-0024** (2026-05-08, accepted, AMENDED): Revive
  test-runner-mcp via npm. Research report claimed
  `test-runner-mcp` was published on npm by `privsim` and
  exposed a 6-tool surface — both claims contained material
  errors. The WO-0081 executor performed the planner-prescribed
  pre-flight live capture (scope_in #5) and discovered:

    1. **Bare `test-runner-mcp` is NOT on npm** (404). The only
       related publication is the third-party scoped mirror
       `@iflow-mcp/mcp-test-runner@0.2.1` published 2026-01-08
       by `chatflowdev`. The upstream `privsim/mcp-test-runner`
       GitHub repo has never published to npm under their own
       name. DEC-0024's research report conflated the GitHub
       repo path with an npm package name.

    2. **Upstream advertises ONE `run_tests` tool with a
       framework enum, NOT 6 separate tools.** Live `tools/list`
       capture (this WO's `/tmp/wo-0082-capture.out` — see
       transcript below) confirms `tools[]` has exactly 1 entry,
       `tools[0].name == "run_tests"`, and
       `inputSchema.framework.enum` carries 7 values
       `["bats", "pytest", "flutter", "jest", "go", "rust",
       "generic"]`. DEC-0024 §Decision point 3 mistook the
       framework enum (7 values) for a 6-tool surface.

  The WO-0081 executor halted with an empirical-validation
  escalation (`ucil-build/escalations/20260508T1435Z-wo-0081-
  dec-0024-tool-surface-mismatch.md`) per the scope_in #5/#2
  boundary instead of silently deviating from the WO contract.

- **DEC-0025** (2026-05-08, accepted, AMENDS DEC-0024): Adopts
  Option A from the executor's authored proposed-DEC-0025
  (accepted as-is). Manifest target =
  `@iflow-mcp/mcp-test-runner@0.2.1`; tool surface =
  single-dispatcher pattern; `[plugin] name = "test-runner"`;
  acceptance criterion #4 amended to assert `health.tools.len()
  == 1` AND `health.tools[0] == "run_tests"` AND framework enum
  ≥6 of canonical set (verify-script independent capture).

### Lessons-learned for future planner regressions

The executor's empirical-validation pattern (run `npx` +
`tools/list` and capture the live surface BEFORE sealing the
manifest) caught both DEC-0024 errors at zero cost. Web-search
alone is insufficient — search snippets can conflate GitHub
repo paths with npm package names AND can misread JSON-Schema
enum values for tool counts. The same live-capture protocol
applies to all manifest-revival WOs (recorded as DEC-0025
§Lessons-learned for planner). This WO carries the protocol
forward verbatim.

## Live `tools/list` capture transcript

Captured via `/tmp/wo-0082-capture.py` against
`npx -y @iflow-mcp/mcp-test-runner@0.2.1` on 2026-05-08; saved
to `/tmp/wo-0082-capture.out`:

```
serverInfo:
  name    = "test-runner"
  version = "0.1.0"

tools (1 total — single-tool dispatcher per DEC-0025):
  run_tests  - "Run tests and capture output"
               inputSchema:
                 type: object
                 required: [command, workingDir, framework]
                 properties:
                   command:        string
                   workingDir:     string
                   framework:      enum ∈ {bats, pytest, flutter,
                                          jest, go, rust, generic}
                                   — 7 values
                   outputDir?:     string
                   timeout?:       number (default 300000ms)
                   env?:           object<string, string>
                   securityOptions?: object {allowSudo?, allowSu?,
                                            allowShellExpansion?,
                                            allowPipeToFile?}
```

Capture matches DEC-0025 §Context expectations exactly:
- `serverInfo.name == "test-runner"` ✓
- `serverInfo.version == "0.1.0"` ✓
- `tools[]` length = 1 ✓
- `tools[0].name == "run_tests"` ✓
- `framework.enum` has 7 values; all from the canonical set
  `{bats, pytest, flutter, jest, go, rust, cargo, generic, vitest}` ✓

## npm-package-name vs serverInfo.name asymmetry (per WO-0077 §planner lesson)

Three distinct names tracked verbatim in the manifest top-of-file
rustdoc to prevent future planner regressions:

| Surface | Name |
|---------|------|
| GitHub upstream (canonical) | `privsim/mcp-test-runner` |
| npm package (only published) | `@iflow-mcp/mcp-test-runner` |
| Live `serverInfo.name` | `test-runner` |
| Manifest `[plugin] name` field | `test-runner` (matches serverInfo.name) |

The integration test asserts on `health.name == "test-runner"`
(the live serverInfo.name carried by `PluginManager::
health_check_with_timeout`); the install string + transport.args
reference the npm package; the manifest rustdoc cites the upstream
GitHub repo for provenance.

## Mutation contract

### M1 — transport.command poison

| | |
|---|---|
| File | `plugins/testing/test-runner/plugin.toml` |
| Patch | `s\|^command = "npx"$\|command = "/__ucil_test_nonexistent_M1__"\|` |
| Targeted selector | `cargo test -p ucil-daemon --test g8_plugin_manifests g8_plugin_manifests::test_runner_manifest_health_check` |
| Expected panic | `health-check test-runner MCP server: Spawn { command: "/__ucil_test_nonexistent_M1__", source: Os { code: 2, kind: NotFound, message: "No such file or directory" } }` |
| Restore | `git checkout -- plugins/testing/test-runner/plugin.toml` |
| Pre-mutation md5 | `/tmp/wo-0082-test-runner-orig.md5` (`ea1e6ab983757e9e66458f350a4e8c34`) |
| Post-restore md5 verify | `md5sum -c /tmp/wo-0082-test-runner-orig.md5` → `OK` ✓ |

Verified locally — test fails with the documented `PluginError::
Spawn { source: Os { code: 2, kind: NotFound } }`; restore is
md5-clean.

### M2 — expected-tool-name regression

| | |
|---|---|
| File | `crates/ucil-daemon/tests/g8_plugin_manifests.rs` |
| Patch | `s\|t == "run_tests"\|t == "NON_EXISTENT_TOOL_M2"\|` (one occurrence — the SA2 assertion line in `test_runner_manifest_health_check`) |
| Targeted selector | `cargo test -p ucil-daemon --test g8_plugin_manifests g8_plugin_manifests::test_runner_manifest_health_check` |
| Expected panic | `(SA2) expected `run_tests` tool in advertised set; got: ["run_tests"]; want: "run_tests"` |
| Restore | `git checkout -- crates/ucil-daemon/tests/g8_plugin_manifests.rs` |
| Pre-mutation md5 | `/tmp/wo-0082-g8tests-orig.md5` (`bcf6fe3a70eefc8a76b9e02246e61a26`) |
| Post-restore md5 verify | `md5sum -c /tmp/wo-0082-g8tests-orig.md5` → `OK` ✓ |

Verified locally — test fails with the SA2-tagged structured panic
that carries the live `tools/list` array verbatim
(`["run_tests"]`). The panic body's diagnostic dump of the live
tool surface IS the load-bearing detection signal for upstream
drift; restore is md5-clean.

## Acceptance criteria sweep (all green locally)

| AC | Check | Result |
|----|-------|--------|
| 1  | `test -f plugins/testing/test-runner/plugin.toml` | OK |
| 2  | `grep -q '^name = "test-runner"$'` in plugin.toml | OK |
| 3  | `grep -qE '^command = "npx"$'` | OK |
| 4  | `grep -qE '@iflow-mcp/mcp-test-runner@0\.2\.1'` | OK |
| 5  | `test -x scripts/devtools/install-test-runner-mcp.sh` | OK |
| 6  | `test -x scripts/verify/P3-W11-F07.sh` | OK |
| 7  | TODO-stub absent in verify script | OK |
| 8  | `! grep -qE 'std::process::Command'` in g8 tests | OK (tokio::process only) |
| 9  | `! grep -qiE 'mock\|fake\|stub'` on production-side files | OK (manifest + install + verify clean) |
| 10 | `! grep -qE '"(main\|latest\|...)"'` in plugin.toml | OK (frozen semver `0.2.1`) |
| 11 | `! grep -qE 'unsafe[[:space:]]*\{[^}]*set_var'` in g8 tests | OK (no NEW unsafe block) |
| 12 | `! grep -qE '"test-runner-mcp"'` in plugin.toml | OK (bare name absent — DEC-0025 §Context Error 1) |
| 13 | `grep -qE '#\[tokio::test\][[:space:]]*async fn test_runner_manifest_health_check'` | resolved via cargo selector substring match (line 270-271) — matches WO-0080 carry-pattern (rustfmt enforces newline between attribute and fn signature) |
| 14 | cargo test selector returns `1 passed` | OK (final run: 1 passed; 0 failed) |
| 15 | `cargo clippy -p ucil-daemon -- -D warnings` | OK |
| 16 | `cargo fmt --check -p ucil-daemon` | OK |
| 17 | `scripts/verify/P3-W11-F07.sh` | OK (cargo + tools/list dispatcher inspection all green) |
| 18 | RFR file exists | OK (this file) |
| 19 | `git log feat ^main --merges` empty | OK (zero merge commits) |

## What I verified locally

- `node --version` → `v22.22.2`; `npx --version` → `10.9.7`; `python3 --version` → `3.13.7`.
- npm registry sweep via `npm view @iflow-mcp/mcp-test-runner versions` → `['0.2.0', '0.2.1']`; latest = `0.2.1`. `npm view test-runner-mcp version` → 404 (confirms DEC-0025 §Context Error 1).
- Live `tools/list` capture against `npx -y @iflow-mcp/mcp-test-runner@0.2.1` succeeded first-attempt — 1 tool `run_tests` with framework enum `["bats", "pytest", "flutter", "jest", "go", "rust", "generic"]` exactly matching DEC-0025 §Context expectations.
- `cargo build -p ucil-daemon --tests` → green.
- `cargo test -p ucil-daemon --test g8_plugin_manifests g8_plugin_manifests::test_runner_manifest_health_check` → `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; finished in 1.60s`.
- `cargo clippy -p ucil-daemon --tests -- -D warnings` → clean.
- `cargo clippy -p ucil-daemon -- -D warnings` → clean.
- `cargo fmt --check -p ucil-daemon` → clean.
- `bash scripts/verify/P3-W11-F07.sh` → all-green, terminating in `[OK] P3-W11-F07` after exercising the cargo test path AND the tools/list dispatcher inspection (`tools/list ok: 1 tool 'run_tests'; framework enum (7 values): ['bats', 'pytest', 'flutter', 'jest', 'go', 'rust', 'generic']`).
- `bash scripts/devtools/install-test-runner-mcp.sh` → exits 0 with `[OK] npx is on PATH`.
- M1 mutation contract — applied, ran, observed expected `Spawn { source: Os { code: 2, kind: NotFound } }`, restored, md5-verified clean.
- M2 mutation contract — applied, ran, observed expected SA2 structured panic with full live-tool-list dump (`got: ["run_tests"]`), restored, md5-verified clean.
- Test-body zero-deletions check on `crates/ucil-daemon/tests/g8_plugin_manifests.rs` (scope_in #8 invariant): the existing `mcp_pytest_runner_manifest_health_check` test body is untouched. File-level rustdoc deletions (~21 lines) are confined to the documented overview amendments per scope_in #8 paragraph 2 (file overview update + DEC-0024+DEC-0025 revival narrative + dual-test selector citation + dual-package-manager FIRST_RUN_TIMEOUT_MS doc).
- Zero-merge-commits check (scope_in #22 / WO-0070 §planner #4): `git log feat/WO-0082-test-runner-mcp-revival ^main --merges` → empty.

## Standing carry-forward standing scope_outs (per WO-0082 scope_in #23/#24)

- AC23 standing: `coverage-gate.sh sccache RUSTC_WRAPPER` workaround applies — verifier should use `env -u RUSTC_WRAPPER cargo llvm-cov ...` for measurement, not the raw gate-script output. Out of scope for this WO per scope_out #16.
- AC24 standing: AC30/AC31 phase-1/phase-2 effectiveness-gate flake escalations remain open (3 escalations: `20260507T0357Z`, `20260507T1629Z`, `20260507T1930Z`); pre-existing standing scope_out, awaiting dedicated harness-improvement WO.

## Verifier notes

- The verifier MUST NOT set `UCIL_SKIP_EXTERNAL_PLUGIN_TESTS` OR `UCIL_SKIP_TESTING_PLUGIN_E2E` — per WO-0076 scope_in #12 + WO-0079 scope_in #25 + WO-0080 scope_in #25 + WO-0082 scope_in #25 carry-forward.
- The integration test relies on `npx` AND `node` being on PATH (per WO-0082 scope_in #27 — `which npx` + `which node` pre-flight). Missing `npx` produces a false-positive `PluginError::Spawn { kind: NotFound }` that masks real M1 detection. The verifier should run `which npx node` before re-running the test from a clean session.
- Cold-cache npx fetch budget: `FIRST_RUN_TIMEOUT_MS = 120_000` (already set in `tests/g8_plugin_manifests.rs:124`) — generous enough for the cold npx fetch of `@iflow-mcp/mcp-test-runner` + `@modelcontextprotocol/sdk` + transitive deps (~30s on cold cache; <1s on warm).
- Tier-2 (vendored TS wrapper at `plugin/test-runner-mcp/`) was NOT triggered. The Tier-1 install path (`npx -y @iflow-mcp/mcp-test-runner@0.2.1`) succeeded first-attempt against the live capture; the Tier-2 path is explicitly REJECTED per DEC-0025 §Decision point 4. The `plugin/test-runner-mcp/` directory does NOT exist on this branch.
- WO-0081 executor halted at the live-capture validation step with an empirical-validation escalation, NOT a verifier rejection. Per DEC-0025 §Consequences, F07 `attempts=0` — this WO is the first verifier-eligible attempt.
- The verify script's optional secondary `tools/call run_tests` smoke is gated on `cargo --version` and is INFORMATIONAL-only — the load-bearing assertions are (a) cargo test green, (b) tools/list independent capture asserting 1 tool + name + framework enum coverage. Per DEC-0025 §Decision point 2 + WO-0082 scope_in #6.

## Files changed (all four commits in `feat/WO-0082-test-runner-mcp-revival`)

```
f5c9e77 feat(plugins/testing): add test-runner G8 manifest pinned to npx -y @iflow-mcp/mcp-test-runner@0.2.1
fe6c11a feat(scripts/devtools): add install-test-runner-mcp.sh helper
4718120 test(daemon):           add test_runner_manifest_health_check inside mod g8_plugin_manifests
93132a6 feat(scripts-verify):   replace P3-W11-F07 TODO-stub with end-to-end test-runner smoke
```

Final commit SHA: `93132a637f32cc039939504ec9cbaf799e71fdd9`
