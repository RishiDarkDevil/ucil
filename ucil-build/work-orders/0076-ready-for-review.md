# Ready-for-review: WO-0076 — ESLint + Semgrep G7 quality plugin manifests

**Final commit sha**: `2ed40062400949de4483ccb16b2f36c3817341b6`
**Branch**: `feat/WO-0076-eslint-and-semgrep-quality-plugin-manifests`
**Phase / Week / Features**: Phase 3, Week 11, P3-W11-F02 + P3-W11-F04
(P3-W11-F03 Ruff explicitly deferred per DEC-0020)

## What I verified locally

### Acceptance criteria 1–31 (all green)

- **AC1–4 (manifests present + structurally valid)**:
  - `plugins/quality/eslint/plugin.toml` parses; `plugin.name=="eslint"`,
    `category=="quality"`, `transport.type=="stdio"`, capabilities under
    `quality.*` namespace.
  - `plugins/quality/semgrep/plugin.toml` parses; same shape, semgrep name.
- **AC5 (no moving tags)**: `! grep -qE '"(main|latest|...)"'` on both
  manifests passes.
- **AC6–9 (install scripts present + executable + run)**:
  - `scripts/devtools/install-eslint-mcp.sh` is `chmod +x` and exits 0.
  - `scripts/devtools/install-semgrep-mcp.sh` is `chmod +x` and exits 0.
- **AC10–13 (integration-test file)**:
  - `crates/ucil-daemon/tests/g7_plugin_manifests.rs` exists.
  - `mod g7_plugin_manifests { ... }` block at file root (DEC-0007).
  - 2× `#[tokio::test]` annotations.
  - **No `std::process::Command` references** (verified via
    `! grep -qE 'std::process::Command'`) — fixture-init helpers all use
    the tokio process variant per WO-0075 W1 lesson.
- **AC14 (eslint health-check)**: `cargo test -p ucil-daemon --test
  g7_plugin_manifests g7_plugin_manifests::eslint_manifest_health_check`
  prints `1 passed; 0 failed`.
- **AC15 (semgrep health-check)**: same selector for `semgrep_manifest_
  health_check` prints `1 passed; 0 failed` (with a working semgrep CLI
  on PATH).
- **AC16 (F02 verify script)**: `bash scripts/verify/P3-W11-F02.sh`
  emits `[OK] P3-W11-F02` and exits 0; tool-level smoke produces 3
  structured findings with `filePath/ruleId/severity/line` keys.
- **AC17 (F04 verify script)**: `bash scripts/verify/P3-W11-F04.sh`
  emits `[OK] P3-W11-F04` and exits 0; tool-level smoke produces 1
  OWASP A03 Injection finding (`subprocess-shell-true`) under
  `p/owasp-top-ten`.
- **AC18 (word-ban grep)**: `! grep -qiE 'mock|fake|stub'` against all
  6 production-side files (2× plugin.toml + 2× install scripts + 2×
  verify scripts) returns empty.
- **AC19 (TODO stubs replaced)**: F02 + F04 verify scripts no longer
  contain `'TODO: implement acceptance test for P3-W11-F0[24]'`.
- **AC20 (clippy)**: `cargo clippy -p ucil-daemon --tests --all-targets
  -- -D warnings` exits 0 cleanly.
- **AC21 (fmt)**: `cargo fmt --all -- --check` exits 0 cleanly.
- **AC22 (full ucil-daemon test suite)**: `cargo test -p ucil-daemon
  --no-fail-fast` exits 0 with all tests + doctests passing.
- **AC23–26 (forbidden_paths git diff audit)**: all empty — 0 LOC
  diff vs main on:
  - `crates/ucil-daemon/src/`
  - `crates/ucil-core/src/`
  - `tests/fixtures/`
  - `ucil-build/feature-list.json` + schema + master plan
  - `crates/ucil-daemon/tests/g{3,4,5,6}_plugin_manifests.rs` +
    `plugin_manifests.rs`
- **AC27 (substantive coverage gate)**: `env -u RUSTC_WRAPPER cargo
  llvm-cov --package ucil-daemon --tests --summary-only --json | jq
  '.data[0].totals.lines.percent'` reports **89.66%** (≥85% floor per
  master-plan §15.4).
- **AC28 (zero merges)**: `git log feat ^main --merges | wc -l` == 0.
- **AC29–31 (commit-trailer audits)**: every WO-0076 commit's body
  carries `Work-order: WO-0076` + the appropriate `Feature: P3-W11-F02`
  / `Feature: P3-W11-F04` trailer.

### Live tool name capture (per WO-0076 scope_in #14)

#### ESLint MCP — `npx -y @eslint/mcp@0.3.5`

```json
serverInfo: {"name": "ESLint", "version": "0.3.5"}
tools/list: 1 tool
  - name: "lint-files" (kebab-case)
    inputSchema: {filePaths: array<string>, required, absolute paths}
    description: "Lint files using ESLint. You must provide a list of
                  absolute file paths..."
```

**Chosen M2 assertion target**: `"lint-files"` (kebab-case literal as
emitted by upstream `tools/list` — preferred over snake-case
translation per WO-0074 §executor #1 lesson).

#### Semgrep MCP — `uvx semgrep-mcp@0.9.0` (DISCLOSED DEVIATION → 0.8.1)

```json
# v0.9.0 (WO-0076 scope_in #2 prescription, DEPRECATED upstream):
serverInfo: {"name": "Semgrep", "version": "1.12.2"}
tools/list: 1 tool
  - name: "deprecation_notice"
    description: "...You should invoke this tool whenever you would
                  use any of the pre-existing Semgrep MCP tools!
                  This includes: semgrep_rule_schema,
                  get_supported_languages, semgrep_findings,
                  semgrep_scan_with_custom_rule, semgrep_scan,
                  semgrep_scan_remote, get_abstract_syntax_tree"

# v0.8.1 (PIVOT — last release with canonical scan tools):
serverInfo: {"name": "Semgrep", "version": "1.12.2"}
tools/list: 8 tools
  - semgrep_rule_schema
  - get_supported_languages
  - semgrep_findings
  - semgrep_scan_with_custom_rule
  - semgrep_scan        ← chosen M2 assertion target
  - semgrep_scan_local
  - security_check
  - get_abstract_syntax_tree
```

**Chosen M2 assertion target**: `"semgrep_scan"` (snake_case literal as
emitted by upstream `tools/list` — preferred over kebab-case
translation per WO-0074 §executor #1 lesson). Maps verbatim to our
declared `quality.semgrep.scan` capability.

`semgrep_scan` inputSchema:
```json
{
  "code_files": "array<{filename: str, content: str}> (required)",
  "config": "optional<str> (e.g. 'p/owasp-top-ten', 'auto')"
}
```

## Disclosed Deviations from WO-0076 scope_in

### DEVIATION 1 — Semgrep MCP version: scope_in #2 prescribed `0.9.0`; pivoted to `0.8.1`

**Rationale**: Live `tools/list` capture against `uvx semgrep-mcp@0.9.0`
confirms upstream removed the canonical 8-tool scan surface in v0.9.0
and replaced it with a single `deprecation_notice` tool that routes
users to either (a) the hosted `mcp.semgrep.ai` Streamable-HTTP MCP
surface, or (b) the bundled `semgrep mcp -t stdio` subcommand on the
Semgrep CLI itself. The historical scan tools (`semgrep_scan`,
`security_check`, `semgrep_findings`, `semgrep_scan_with_custom_rule`,
`semgrep_scan_local`, `semgrep_rule_schema`, `get_supported_languages`,
`get_abstract_syntax_tree` — 8 tools total) are no longer reachable
via the PyPI `uvx` launch path in v0.9.0.

v0.8.1 (released 2025-09-09; immediate predecessor of v0.9.0) is the
last PyPI release that exposes the canonical scan-tool surface.
Pinning to v0.8.1 preserves the WO-0076 F04 acceptance criterion
("≥1 security finding using the OWASP rule set") without changing the
upstream package, transport, or auth model. All other criteria from
WO-0076 scope_in #2 are satisfied verbatim:

- Same package (`semgrep-mcp` on PyPI) and same vendor (Semgrep Inc.)
- Same transport (`stdio`)
- Same launch shape (`uvx <pkg>@<version>`)
- Same operator-state requirement (Semgrep CLI on PATH or
  `SEMGREP_PATH` env var)
- Same MIT license
- Same documented OWASP-class default-ruleset support
  (`p/owasp-top-ten` available as `config` arg to `semgrep_scan`)

The pivot is documented verbatim in
`plugins/quality/semgrep/plugin.toml`'s top-of-file rustdoc + this
RFR §"Live tool name capture".

### DEVIATION 2 — Semgrep CLI dependency surfaced (scope_in #2 implicit assumption)

**Rationale**: Both `semgrep-mcp@0.8.1` and `@0.9.0` require the
Semgrep CLI binary on PATH (or located via `SEMGREP_PATH`). The
upstream lifespan handler `semgrep_mcp.semgrep.mk_context` calls
`semgrep --pro --version` BEFORE the MCP server reaches its
initialize handshake; if the CLI is missing the server raises
`McpError` and the integration test / verify script see a
`BrokenPipe` before tools/list can complete.

The WO-0076 `lessons_applied_summary` line about WO-0069 §planner
("API-key-presence short-circuit gate — N/A as load-bearing here
since neither upstream requires auth for the basic-scan smoke") is
incomplete: while no API token is required, an external CLI binary
IS required. The integration test and F04 verify script both apply
the WO-0069 short-circuit pattern but for a CLI binary rather than
an env var: `resolve_working_semgrep()` filters out the
broken-on-import uvx-bundled semgrep (the venv that uvx prepends to
PATH ships an opentelemetry-instrumentation-requests-conflicting
semgrep) and gates on a successful `semgrep --version`. When no
working CLI is found, the smoke skips with `[SKIP]` and exit 0
(operator-state, NOT failure).

The integration test additionally exports `SEMGREP_PATH` to the
spawned uvx subprocess so the upstream's `find_semgrep_info`
resolution finds the working binary instead of the broken venv-bundled
one. This is a load-bearing detail that didn't surface in any prior
WO since no other G-group plugin has an external-CLI dep.

This deviation is informational/discovery-only — no scope change. The
manifest top-of-file rustdoc + the `install-semgrep-mcp.sh`
operator-guidance script document the CLI dependency in full.

## Live integration verification

```
$ cargo test -p ucil-daemon --test g7_plugin_manifests -- --test-threads=1
running 2 tests
test g7_plugin_manifests::eslint_manifest_health_check ... ok
test g7_plugin_manifests::semgrep_manifest_health_check ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured;
finished in 2.47s

$ bash scripts/verify/P3-W11-F02.sh
[OK] P3-W11-F02
tools/call lint-files ok: 3 structured findings;
  first: rule='no-unused-vars' severity=2 line=1 file='.../bad.js'

$ bash scripts/verify/P3-W11-F04.sh
[OK] P3-W11-F04
tools/call semgrep_scan ok: 1 OWASP findings;
  first: rule='python.lang.security.audit.subprocess-shell-true...'
         severity='ERROR' file='bad_security.py' line=24

$ env -u RUSTC_WRAPPER cargo llvm-cov --package ucil-daemon --tests \
  --summary-only --json | jq '.data[0].totals.lines.percent'
89.66
```

## Mutation contract (M1 + M2 — for verifier reference)

### M1: `transport.command` poison (one mutation per feature)

**Pre-mutation snapshot**:
```
md5sum plugins/quality/eslint/plugin.toml > /tmp/wo-0076-eslint-orig.md5
md5sum plugins/quality/semgrep/plugin.toml > /tmp/wo-0076-semgrep-orig.md5
```

**ESLint mutation**: edit `plugins/quality/eslint/plugin.toml`
line ~115 — replace `command = "npx"` with
`command = "/__ucil_test_nonexistent_M1__"`. Run
`cargo test -p ucil-daemon --test g7_plugin_manifests
g7_plugin_manifests::eslint_manifest_health_check`. Expected: panics
with `PluginError::Spawn { source: ENOENT }`-class chain.
**Restore**: `git checkout -- plugins/quality/eslint/plugin.toml`;
verify `md5sum -c /tmp/wo-0076-eslint-orig.md5` returns OK.

**Semgrep mutation**: edit `plugins/quality/semgrep/plugin.toml`
line ~189 — replace `command = "uvx"` with
`command = "/__ucil_test_nonexistent_M1__"`. Same selector ↔ same
panic shape ↔ same restore protocol.

### M2: expected-tool-name regression (one mutation per feature)

**Pre-mutation snapshot**:
```
md5sum crates/ucil-daemon/tests/g7_plugin_manifests.rs > /tmp/wo-0076-g7tests-orig.md5
```

**ESLint mutation**: edit `crates/ucil-daemon/tests/g7_plugin_manifests.rs`
— replace the `"lint-files"` literal in
`eslint_manifest_health_check`'s `t == "lint-files"` assertion with
`"NON_EXISTENT_TOOL_M2"`. Run the same selector. Expected: panics
with `(SA1) expected `lint-files` tool in advertised set; got: [...]`.
**Restore**: `git checkout -- crates/ucil-daemon/tests/g7_plugin_manifests.rs`;
verify md5.

**Semgrep mutation**: same file — replace the `"semgrep_scan"` literal
in `semgrep_manifest_health_check`'s `t == "semgrep_scan"` assertion
with `"NON_EXISTENT_TOOL_M2"`. Same selector ↔ panics with
`(SA2) expected `semgrep_scan` tool in advertised set; got: [...]`.
Same restore protocol.

## Lessons applied (per WO-0076 scope_in #18)

- **WO-0075 §planner: G6 → G7 paired-manifest template port** — direct
  shape parent, scaled-down from 3 to 2 features (Ruff deferred per
  DEC-0020).
- **WO-0075 §executor W1: tokio process variant in async test
  helpers** — PRE-EMPTIVELY APPLIED in `g7_plugin_manifests.rs`'s
  `copy_fixture_to_tmpdir`, `version_check`, `resolve_working_semgrep`
  helpers. AC13 (`! grep -qE 'std::process::Command'
  crates/ucil-daemon/tests/g7_plugin_manifests.rs`) confirms this
  invariant holds throughout the file (rustdoc comments included). No
  WO-0075 W1 critic warning surfaced.
- **WO-0075 §planner: G6+ keep separate g<N>_plugin_manifests.rs peer
  file** — applied to `g7_plugin_manifests.rs` as peer of
  `g3/g4/g5/g6_plugin_manifests.rs`. AC25 forbidden-paths audit
  confirms peer files were not modified.
- **WO-0074 §executor #1: DON'T translate live tools/list names to
  snake_case when they ship kebab-case** — captured `lint-files`
  (kebab-case) verbatim from upstream + `semgrep_scan` (snake_case)
  verbatim from upstream. Both literals embedded as M2 assertion
  targets in test panic-body strings.
- **WO-0074 §executor #2: do NOT use `--mcp` as warm-up flag** — both
  install scripts use `--help` for the warm-up step.
- **WO-0074 §executor #3: document `UCIL_SKIP_QUALITY_PLUGIN_E2E`** —
  the new env var is documented in the test file's `//!` rustdoc + the
  F02/F04 verify scripts' shell comments. The verifier MUST NOT set
  this env var per WO-0076 scope_in #12.
- **WO-0074 §executor #4: prefer python deadline polling over bash
  sleep** — both verify scripts drive their tool-level smoke from
  python with 60-180s deadlines. Semgrep scans take 10-30s on cold
  rule-cache; python deadlines accommodate the wall-time variance.
- **WO-0074 §executor #5: copy fixtures into mktemp -d tmpdir BEFORE
  invoking** — applied to both F02 (typescript-project) and F04
  (mixed-project) in the integration test + verify scripts. AC25
  fixture-immutability audit confirms 0 LOC diff.
- **WO-0072/0074/0075 M1 + M2 mutation contract pre-baked** — see
  §"Mutation contract" above.
- **WO-0067/0068/0069/0070 DEC-0007 SA-numbered panic-body format** —
  test file uses `(SA0)`, `(SA1)`, `(SA2)` per DEC-0007.
- **WO-0067..WO-0075 substantive AC23 coverage standing protocol** —
  measured **89.66%** via `env -u RUSTC_WRAPPER cargo llvm-cov`
  (≥85% floor preserved).
- **DEC-0020 NEW Ruff deferral lineage** — P3-W11-F03 explicitly
  excluded from this WO. The DEC-0020 §Revisit-trigger curl/npm/gh
  triad documented for re-examination at next planner pass that
  suspects an upstream Ruff MCP server has emerged. The
  upstream-availability sweep pattern (PyPI-404 + npm-search +
  GitHub-search triad) is now a reusable preemptive-deferral pattern
  for future planner passes.
- **WO-0067..WO-0075 AC30/AC31 effectiveness-gate flake carry-over**
  with 3 standing escalations (`20260507T0357Z-effectiveness-nav-rust
  -symbol-rs-line-flake.md`, `20260507T1629Z-effectiveness-refactor
  -rename-python-fixture-missing-symbol.md`, `20260507T1930Z
  -effectiveness-nav-rust-symbol-doctest-caller-flake.md`) — pre-
  existing standing scope_out per WO-0076 scope_out #15. Not in
  this WO's scope.
- **WO-0070..WO-0075 AC25 wording: `git log feat ^main --merges = 0`**
  — applied verbatim. AC28 confirms 0 merges.

## Commit lineage

```
2ed4006 test(verify): replace P3-W11-F04 TODO stub with Semgrep MCP acceptance
dae60ee test(verify): replace P3-W11-F02 TODO stub with ESLint MCP acceptance
5e1b09c test(daemon): add g7_plugin_manifests integration test for ESLint + Semgrep
6e06dd5 feat(devtools): add install-eslint-mcp.sh and install-semgrep-mcp.sh
415f184 feat(plugins): add Semgrep G7 quality plugin manifest pinned to semgrep-mcp@0.8.1
7f8a3d8 feat(plugins): add ESLint G7 quality plugin manifest pinned to @eslint/mcp@0.3.5
569eccc chore(planner): WO-0076 G7 quality plugin manifests + DEC-0020 Ruff deferral
```

6 commits authored by the executor (the 7th, `569eccc`, is the
planner's WO-0076 + DEC-0020 emit and is the branch base). Each
executor commit is Conventional-Commit-formatted with `Phase: 3` +
appropriate `Feature: P3-W11-F02` / `Feature: P3-W11-F04` /
both / `Work-order: WO-0076` trailers. AC29–31 git-trailer audits
confirmed.

The integration-test commit (`5e1b09c`) covers both `#[tokio::test]`
async fns under DEC-0005 module-coherence carve-out (cited in commit
body); splitting would leave one of the two test bodies un-buildable
in isolation.

## Pre-flight checklist (WO-0076 scope_in #13)

- [x] `cargo build -p ucil-daemon` — clean compile
- [x] `cargo test -p ucil-daemon --test g7_plugin_manifests` — both
      tests green
- [x] `cargo clippy -p ucil-daemon --all-targets --tests -- -D warnings`
      — zero warnings
- [x] `cargo fmt -p ucil-daemon --check` — formatting clean
- [x] Word-ban grep on production-side files — empty
- [x] `bash scripts/verify/P3-W11-F02.sh` + `bash scripts/verify/
      P3-W11-F04.sh` — both green
- [x] `command -v npx uvx python3` — all three on PATH

Ready for verifier in fresh session.
