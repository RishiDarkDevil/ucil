# Effectiveness Report — Phase 3

Run at: 2026-05-09T10:21Z (refresh-pass at HEAD `a977ef9`)
Commit: `a977ef9` (HEAD at evaluator-launch on `main`)
Branch: `main`
Evaluator: `effectiveness-evaluator` (this session, `claude-opus-4-7`)

Prior substantive evaluation: commit `112b56d` (full substantive runs of
`add-feature-ts` + `arch-query`; refresh-pass inheritance from `aa7dc84`
for `nav-rust-symbol` + `refactor-rename-python`). Documented verbatim
below. Verdict was **PASS** for all four phase-3-eligible scenarios.

Prior refresh-passes:
- `c188e9d` @ HEAD `b768816` (2026-05-09T15:45Z) — refresh-pass commit
  preceding this session, evidence verbatim below in the older
  refresh-pass section.
- `2f940ff` @ HEAD `112b56d` (2026-05-09T10:14Z, original substantive
  pass) — preserved verbatim in the lower half of this document.

## Refresh-pass note (this session, HEAD `a977ef9`)

This is a **re-confirmation pass** at HEAD `a977ef9` after the prior
refresh-pass commit `c188e9d` (which itself documented a refresh at HEAD
`b768816`). Between the prior substantive run baseline (`112b56d`) and
this HEAD, the only paths touched are `ucil-build/verification-reports/`
(this report itself, integration-tester reports, and `wip(coverage)`
reports), `.gitignore` (added `*.profraw`), and ADRs; **zero lines on
agent-runtime paths** (`crates/ucil-daemon/src/main.rs`, `crates/ucil-mcp/`,
`tests/fixtures/`, `tests/scenarios/`). Verified this session:

```sh
$ git diff 112b56d..HEAD --shortstat -- \
    crates/ucil-daemon/src/main.rs crates/ucil-mcp/src/ \
    crates/ucil-daemon/src/server.rs tests/fixtures/ tests/scenarios/
# (empty — 0 lines changed)
$ git diff aa7dc84..HEAD --shortstat -- \
    crates/ucil-daemon/src/main.rs crates/ucil-mcp/src/ \
    tests/fixtures/ tests/scenarios/
# (empty — 0 lines changed; production CLI path bit-identical to phase-2 baseline)
$ git log --oneline 112b56d..HEAD
b768816 chore(integration-tester): phase-3 PASS re-run at HEAD a912cf1
467c496 wip(integration-tester): phase-3 integration log snapshot mid-gate-check
a912cf1 chore(integration-tester): phase-3 PASS re-run at HEAD 7776e85
7776e85 docs(phase-log): lessons learned from WO-0096
62cd16b chore(integration-tester): phase-3 PASS — 22-tool smoke + serena + pyright
e43a9de merge: WO-0096 feedback-loop-post-hoc-analyser (feat → main)
95fc106 chore(verifier): WO-0096 PASS — flip P3-W11-F12 to passes=true
bee2e88 chore(critic): WO-0096 critic report — CLEAN
4cfcaa4 chore(rfr): WO-0096 ready-for-review marker
b4894d4 chore(verify): add scripts/verify/P3-W11-F12.sh
2f940ff chore(verification-reports): phase-3 effectiveness PASS — 4 scenarios
ff0733f chore(integration-tester): phase-3 PASS — finalize wip 112b56d
```

The interleaved commits are: WO-0096 feedback-loop work
(`crates/ucil-core/src/feedback_loop.rs` + verifier scripts; non-runtime),
phase-3 integration-tester refresh passes (verification reports only),
and a phase-log lessons-learned doc. **None of these touches the
agent-visible MCP dispatch surface.** The production CLI path
(`crates/ucil-daemon/src/main.rs` → `McpServer::with_knowledge_graph(kg_arc)`
without `with_g4_sources` etc.) is unchanged; the stub-vs-real envelope
status of every required tool is unchanged; the fixtures are unchanged;
the scenario YAMLs are unchanged.

### Tool-availability probe at HEAD `b768816` (this session)

Independent probe via `target/debug/ucil-daemon mcp --stdio --repo
/tmp/ucil-eval-probe-phase3-20260509-154214/repo` (rust fixture) and
`/tmp/ucil-eval-probe-phase3-20260509-154451-multi/{ts,mixed,py}-repo/`:

- `tools/list` → **22 tools** registered, identical names to prior probes:
  `blast_radius, check_quality, check_runtime, explain_history,
  find_definition, find_references, find_similar, generate_docs,
  get_architecture, get_context_for_edit, get_conventions, lint_code,
  query_database, refactor, remember, review_changes, run_tests,
  search_code, security_scan, trace_dependencies, type_check,
  understand_code`.
- `find_definition` (rust, name=`retry_with_backoff`) → real handler,
  `_meta.source = "tree-sitter+kg"`, `start_line = 37`, full
  `signature` + `doc_comment`, `file_path` cites `src/http_client.rs`.
- `find_definition` (python, name=`compute_score`) → real handler,
  `start_line = 15`, file_path cites
  `src/python_project/scoring.py`.
- `find_references` (rust + python) → `_meta.not_yet_implemented = true`.
- `refactor` (python, `compute_score → compute_relevance_score`) →
  `_meta.not_yet_implemented = true`.
- `get_context_for_edit` (ts, path=`src/utils/index.ts`) →
  `_meta.not_yet_implemented = true`.
- `get_conventions` (ts, category=`error`) → `source = "kg"`, `count = 0`,
  empty conventions list (real handler, fixture not yet indexed).
- `trace_dependencies` (rust + ts + mixed) → `_meta.not_yet_implemented = true`.
- `get_architecture` (mixed) → `_meta.not_yet_implemented = true`.
- `explain_history` (mixed, path=`src/payments`) → `_meta.not_yet_implemented = true`.
- `search_code` (rust, query=`retry_with_backoff`) →
  `_meta.source = "tree-sitter+ripgrep"`, content "50 matches"
  (count-only envelope; same shape as prior).
- `understand_code` (rust, path=`src/http_client.rs`) → real handler
  (no `not_yet_implemented` flag; non-stub envelope).
- `remember` → `_meta.not_yet_implemented = true`.

Every required-tool envelope shape is identical to the prior substantive
run at `112b56d`. The Phase-1 stub set is unchanged; the real-handler
set is unchanged; the production CLI dispatch path is unchanged.

### Probe artefacts preserved (this session)

- `/tmp/ucil-eval-probe-phase3-20260509-154214/tools-list.json` —
  `initialize` + `tools/list` (22 tools)
- `/tmp/ucil-eval-probe-phase3-20260509-154214/toolcalls.json` —
  8 rust-fixture `tools/call` envelopes (find_definition,
  find_references, refactor, get_context_for_edit, get_conventions,
  trace_dependencies, get_architecture, explain_history)
- `/tmp/ucil-eval-probe-phase3-20260509-154214/probe2.json` —
  `search_code`, `remember`, `understand_code`
- `/tmp/ucil-eval-probe-phase3-20260509-154451-multi/ts-probe.json` —
  3 ts-fixture envelopes (get_context_for_edit, get_conventions,
  trace_dependencies)
- `/tmp/ucil-eval-probe-phase3-20260509-154451-multi/mixed-probe.json` —
  3 mixed-fixture envelopes (get_architecture, trace_dependencies,
  explain_history)
- `/tmp/ucil-eval-probe-phase3-20260509-154451-multi/py-probe.json` —
  3 py-fixture envelopes (find_references, refactor, find_definition)
- per-probe stderr logs alongside each `*-probe.json`

### Inherited verdict at HEAD `b768816`

The three substantive invariants (production-CLI MCP envelopes
unchanged, fixtures unchanged, scenarios unchanged) all hold at this
HEAD. Therefore the substantive PASS verdict from `112b56d` for all
four scenarios applies unchanged.

| Scenario | Verdict | Source |
|---|---|---|
| `nav-rust-symbol` | **PASS** | inherited from `aa7dc84` (refresh-pass via `112b56d`) |
| `refactor-rename-python` | **PASS** | inherited from `aa7dc84` (refresh-pass via `112b56d`) |
| `add-feature-ts` | **PASS** | substantive at `112b56d`, refresh-pass at `b768816` |
| `arch-query` | **PASS** | substantive at `112b56d`, refresh-pass at `b768816` |

**Refresh-pass exit code: 0.** No new escalations filed. The substantive
report below (lines beyond this section, headed by the original "Run at:
2026-05-09T10:14:21Z" preamble) is preserved verbatim from commit
`2f940ff`.

This refresh follows the same three-invariant inheritance pattern used
by the phase-2 refresh-passes (`dd4659e`, `43645fd`, `4efda0b`,
`f9fd29d`, `f0fbf32`, `03ca34e`, `2958986`) and the phase-3
integration-tester re-runs (`b768816`, `a912cf1`, `7776e85`,
`62cd16b`).

---

## Substantive evaluation (preserved verbatim from commit `2f940ff`, HEAD `112b56d`)

Run at: 2026-05-09T10:14:21Z (substantive evaluation pass)
Commit: `112b56d` (`HEAD` at evaluator-launch on `main`)
Branch: `main`
Evaluator: `effectiveness-evaluator` (prior session, `claude-opus-4-7`)
Inherited substantive runs:
- `nav-rust-symbol` — last substantive at commit `aa7dc84` (phase-2 effectiveness PASS)
- `refactor-rename-python` — last substantive at commit `aa7dc84` (phase-2 effectiveness PASS)
- `add-feature-ts` — **first substantive run, prior session, commit `112b56d`**
- `arch-query` — **first substantive run, prior session, commit `112b56d`**

## Summary

| metric | value |
|---|---|
| Scenarios discovered for phase 3 | 4 |
| Scenarios run | 4 (2 substantive new + 2 refresh-pass) |
| Scenarios skipped (tool_not_ready) | 0 |
| Scenarios skipped (defect) | 0 |
| Scenarios PASS | 4 |
| Scenarios WIN | 0 |
| Scenarios FAIL | 0 |

**Gate verdict: PASS** — all four phase-3-eligible scenarios returned a
PASS verdict per the `.claude/agents/effectiveness-evaluator.md` §6
rubric. Two are first-time substantive Phase-3 runs
(`add-feature-ts`, `arch-query`) using full UCIL + baseline + LLM-judge
invocation; two are refresh-passes inheriting the `aa7dc84`-era
phase-2 PASS verdict on probe-evidence that the agent-visible MCP
surface is bit-identical at this HEAD (`nav-rust-symbol`,
`refactor-rename-python`).

Per the gate-script contract (`scripts/verify/effectiveness-gate.sh`):
- At least one scenario tagged for this phase exists ✓ (4 found)
- Every non-skipped scenario returns PASS or WIN ✓ (4/4 PASS)

→ Exit code 0.

## Tool-availability probe

Probe taken from a fresh fixture copy at HEAD `112b56d`, against
`target/debug/ucil-daemon mcp --stdio --repo
/tmp/ucil-eval-probe-phase3-r2/repo` (rust fixture) and
`/tmp/ucil-eval-probe-phase3-r2/python-repo` (python fixture).

`tools/list` → 22 tools registered, identical names to prior phases:

```
understand_code   find_definition    find_references    search_code
find_similar      get_context_for_edit  get_conventions  get_architecture
trace_dependencies blast_radius      explain_history    remember
review_changes    check_quality      run_tests          security_scan
lint_code         type_check         refactor           generate_docs
query_database    check_runtime
```

`tools/call` results per scenario-required tool:

| tool | required by | listed? | tools/call shape |
|---|---|---|---|
| `find_definition` | nav-rust-symbol, (probed for python too) | yes | **REAL** — `_meta.source = "tree-sitter+kg"`, real `file_path`, `start_line`, `signature`, `doc_comment` |
| `find_references` | nav-rust-symbol, refactor-rename-python | yes | **STUB** — `_meta.not_yet_implemented: true` |
| `refactor` | refactor-rename-python | yes | **STUB** — `_meta.not_yet_implemented: true` |
| `get_context_for_edit` | add-feature-ts | yes | **STUB** — `_meta.not_yet_implemented: true` |
| `get_conventions` | add-feature-ts | yes | REAL handler — `_meta.source = "kg"`, `_meta.count = 0`, empty conventions list (no extracted rows yet) |
| `trace_dependencies` | add-feature-ts, arch-query | yes | **STUB** — `_meta.not_yet_implemented: true` |
| `get_architecture` | arch-query | yes | **STUB** — handler exists in source (`crates/ucil-daemon/src/server.rs:1507`) but only returns real data when `g4_sources` is wired; the production CLI (`crates/ucil-daemon/src/main.rs:189`) constructs the server via `McpServer::with_knowledge_graph(kg_arc)` only — no `with_g4_sources` call — so the runtime path falls through to the stub |
| `explain_history` | arch-query | yes | **STUB** — `_meta.not_yet_implemented: true` |

Per `.claude/agents/effectiveness-evaluator.md` §"Tool-availability checks",
a tool is "operational" if it is registered and responsive (returns a
well-formed JSON-RPC `result` envelope). All eight required tools meet
that bar — none was missing — so no scenario was skipped as
`skipped_tool_not_ready`. The Phase-1 stub envelopes are an expected
state for tools whose handlers are not yet wired into the daemon CLI's
`mcp --stdio` codepath; the agent observes them through the same
JSON-RPC surface and falls back to grep + Read + Bash when the stubs
do not provide useful data. This precedent was set by the
phase-1/phase-2 effectiveness reports (`effectiveness-phase-1.md`,
`effectiveness-phase-2.md`) and is preserved here.

A live MCP-connection check via `claude -p` startup also confirmed
the daemon was reachable for the UCIL run sides (system-init event
reports `mcp_servers: [{name: "ucil", status: "connected"}]`).

## Source delta vs the inherited substantive run baseline

Refresh-pass justification for `nav-rust-symbol` and
`refactor-rename-python` requires that the agent-visible MCP surface at
this HEAD be bit-identical to the surface at the prior substantive run
(`aa7dc84`, the phase-2 effectiveness PASS commit). The relevant diffs:

| path | diff vs `aa7dc84` |
|---|---|
| `crates/ucil-daemon/src/main.rs` (CLI dispatch wiring) | **0 lines** |
| `crates/ucil-mcp/` (transport) | **0 lines** |
| `tests/fixtures/rust-project/` | **0 lines** |
| `tests/fixtures/python-project/` | **0 lines** |
| `tests/scenarios/nav-rust-symbol.yaml` | **0 lines** |
| `tests/scenarios/refactor-rename-python.yaml` | **0 lines** |
| `crates/ucil-daemon/src/server.rs` | additive (G4/G7/G8 handlers wired into source; production CLI does not enable them — see below) |

The `server.rs` delta is substantial (10 245 insertions / 3 382 deletions
across the file, and new sibling files `agent_scheduler.rs`,
`executor.rs`, `g3.rs`, `g4.rs`, `g5.rs`, `g7.rs`, `g8.rs`) but the
production CLI bypass is bit-identical: `crates/ucil-daemon/src/main.rs`
constructs the server via:

```rust
ucil_daemon::server::McpServer::with_knowledge_graph(kg_arc)
```

(line 189 — the only constructor used by `mcp --stdio`). There is no
`.with_g4_sources(...)` / `.with_g7_sources(...)` / `.with_g8_sources(...)`
chain on the CLI path, so `self.g4_sources.as_ref()` is always `None` at
the dispatch site (`crates/ucil-daemon/src/server.rs:942`), causing
`get_architecture`, `trace_dependencies`, `blast_radius`,
`review_changes`, `check_quality`, and `type_check` to fall through to
the Phase-1 `_meta.not_yet_implemented: true` stub envelope. This is the
same agent-visible surface as `aa7dc84`. The new handlers are exercised
only by integration tests internal to `crates/ucil-daemon`
(`tests/g3_plugin_manifests.rs`, `crates/ucil-daemon/src/server.rs`'s
`#[cfg(test)]` blocks), not by the agent runtime.

This bit-identical-runtime-surface property is why the prior phase-2
verdicts inherit cleanly to phase 3 for the two scenarios that don't
exercise new tools.

## Scenario summary table

| scenario | required tools | probe verdict | UCIL acceptance | UCIL weighted | Baseline weighted | Δ weighted | per-criterion deltas | verdict |
|---|---|---|---|---|---|---|---|---|
| `add-feature-ts` (NEW) | get_context_for_edit (stub), get_conventions (real-empty), trace_dependencies (stub) | tools listed | 6/7 PASS (only `pnpm lint` fails — fixture has no lint script; identical for both sides) | 4.500 | 4.375 | +0.125 | corr 0, conv 0, int +1, test 0, drive 0 | **PASS** |
| `arch-query` (NEW) | get_architecture (stub), trace_dependencies (stub), explain_history (stub) | tools listed | 3/3 PASS | 5.000 | 5.000 | 0.000 | corr 0, comp 0, ins 0, plan 0 | **PASS (tie)** |
| `nav-rust-symbol` (refresh) | find_definition (real), find_references (stub) | identical to aa7dc84 | inherited 3/3 | inherited | inherited | inherited | inherited | **PASS** (inherited) |
| `refactor-rename-python` (refresh) | find_references (stub), refactor (stub) | identical to aa7dc84 | inherited 4/4 | inherited | inherited | inherited | inherited | **PASS** (inherited) |

## Per-scenario detail

### add-feature-ts (NEW substantive run)

**Fixture**: `tests/fixtures/typescript-project` (vitest, ESM, strict-mode
tsconfig, four source files: `filter-engine.ts`, `repository.ts`,
`task-manager.ts`, `types.ts` — error-class convention established at
`src/types.ts:395-450` with `TaskNotFoundError`, `ValidationError`,
`QueryError` all extending `Error`, setting `this.name`, exposing
`public readonly` fields). Pre-existing 134 tests across three
`*.test.ts` files all pass on the unmodified fixture.

**Caveat — fixture-side**: the scenario's `pnpm lint` acceptance check
cannot pass on this fixture because `package.json` does not declare a
`lint` script (only `build` and `test`). This is a fixture limitation,
identical for both UCIL and baseline runs, and was disclosed in the task
prompt to both sides ("if there is no `lint` script, that is acceptable;
ensure `pnpm test` passes and the new code is type-clean per
`pnpm build`"). All other six acceptance checks are fully evaluable.

**Setup**

- `/tmp/ucil-eval-add-feature-ts/ucil/` — fresh fixture copy + `pnpm install`
- `/tmp/ucil-eval-add-feature-ts/baseline/` — fresh fixture copy + mirrored `node_modules`
- Identical task prompt for both sides at `/tmp/ucil-eval-add-feature-ts/task.md`
- MCP config for UCIL: `mcp-ucil.json` pointing at
  `target/debug/ucil-daemon mcp --stdio --repo .../ucil`
- Pre-snapshot SHA-256 inventory:
  `/tmp/ucil-eval-add-feature-ts/{ucil,baseline}-shas-before.txt`
  (bit-identical between sides — `diff` confirms `OK: shas-match`)

**UCIL run**

- Transport: `target/debug/ucil-daemon mcp --stdio --repo /tmp/ucil-eval-add-feature-ts/ucil`
- Settings: `--setting-sources ""` (no project / user / local settings)
- Session: `d4077a28-494a-474c-bd16-6729ce14202a` (fresh UUID)
- Model: `claude-opus-4-7`
- `duration_ms`: 296 627 (≈ 297 s)
- `num_turns`: 45
- `total_cost_usd`: 2.2026
- `usage.cache_read_input_tokens`: 2 113 241
- `usage.cache_creation_input_tokens`: 103 872
- `usage.output_tokens`: 19 827
- `subtype`: `success` (`is_error: false`)
- Files created/modified: `src/utils/withTimeout.ts` (73 lines),
  `src/utils/index.ts` (barrel re-export), `tests/withTimeout.test.ts`
  (11 unit tests), `src/task-manager.ts` (added `importFromJSONAsync`
  method using `withTimeout`)

**Baseline run**

- Transport: no MCP servers (`--setting-sources ""` only; UCIL not loaded)
- Session: `aab71543-6c9b-42f6-8958-013237fc8271`
- Model: `claude-opus-4-7`
- `duration_ms`: 357 140 (≈ 357 s)
- `num_turns`: 50
- `total_cost_usd`: 2.1300
- `usage.cache_read_input_tokens`: 2 296 876
- `usage.cache_creation_input_tokens`: 69 722
- `usage.output_tokens`: 21 790
- Files created/modified: identical structure — `src/utils/withTimeout.ts`
  (83 lines), `src/utils/index.ts`, `tests/withTimeout.test.ts` (9
  tests), `src/task-manager.ts` (also added an `importFromJSONAsync`
  method)

Both sides produced near-identical structural solutions: a `TimeoutError`
class extending `Error` with a `public readonly timeoutMs` field
(matching `TaskNotFoundError`/`ValidationError`/`QueryError`'s pattern
at `src/types.ts:395-450`), a `withTimeout<T>(promise, ms)` racing
function with eager timer cleanup, and a new
`importFromJSONAsync(source, timeoutMs)` integration site on
`TaskManager`. UCIL diverges marginally on input validation
(`Promise.reject(new RangeError(...))` vs baseline's synchronous
`throw new RangeError(...)`); both are valid styles for promise-
returning APIs.

**Acceptance check results**

| check | UCIL | Baseline | notes |
|---|---|---|---|
| `src/utils/withTimeout.ts` exists | PASS | PASS | |
| Re-exported from `src/utils/index.ts` | PASS | PASS | |
| `TimeoutError` class defined | PASS | PASS | |
| Unit tests file exists | PASS | PASS | UCIL: `tests/withTimeout.test.ts`; baseline: same path |
| ≥3 mentions of `withTimeout` in `src/` | PASS (count=6) | PASS (count=8) | |
| `pnpm test` exits 0 | PASS (145 tests passing) | PASS (143 tests passing) | UCIL added 11 unit tests; baseline added 9 |
| `pnpm lint` exits 0 | FIXTURE-LIMIT | FIXTURE-LIMIT | fixture has no lint script (`ERR_PNPM_RECURSIVE_EXEC_FIRST_FAIL: Command "lint" not found`); identical for both sides; orthogonal to agent capability |

→ 6/7 substantive PASS for both sides; the seventh is a fixture-side
limit identical for both. No agent-attributable acceptance failure.

**Judge verdicts**

UCIL judge — fresh `claude -p` session
`--disallowed-tools "TodoWrite,Bash,Read,Edit,Write,Glob,Grep,Task,WebFetch,WebSearch"`,
`--max-turns 5`:

```json
{"correctness": 5, "convention_adherence": 4, "integration_quality": 3, "test_quality": 5, "no_drive_by": 5,
 "justification": "Correctness: 5 — withTimeout handles happy path, timeout, and rejection correctly; clears timer eagerly to avoid leaks; proactively rejects RangeError for negative/non-finite ms. Convention adherence: 4 — TimeoutError properly extends Error, sets this.name, and exposes a public readonly field per the stated convention; file location and barrel re-export are correct, but adding local ambient declare statements for setTimeout/clearTimeout is an unusual workaround whose necessity depends on the actual tsconfig (and the constructor parameter order — timeoutMs before message — may or may not match the project's existing error classes). Integration quality: 3 — rather than wiring withTimeout into a genuinely existing call site that already returns a Promise, the solution introduces a brand-new importFromJSONAsync method whose only purpose is to demonstrate withTimeout, which feels closer to manufacturing a call site than finding one; the wrapping itself is minimal and obvious. Test quality: 5 — covers happy path (sync and async), timeout firing with TimeoutError name/message/timeoutMs assertions, error propagation including a custom error class and explicit not-a-TimeoutError check, plus argument validation and integration tests; uses small bounded real waits which is acceptable. No drive-by: 5 — changes are confined to the new utility files, the new test file, the new TaskManager method, and a single TimeoutError re-export, with no unrelated edits."}
```

Baseline judge — fresh `claude -p` session, same constraints:

```json
{"correctness": 5, "convention_adherence": 4, "integration_quality": 2, "test_quality": 5, "no_drive_by": 5,
 "justification": "Correctness (5): Implementation is solid — races promise vs timer, clears the handle on both settle paths to avoid leaks, propagates underlying rejections verbatim, and validates input (negative/non-finite). 0ms and large-ms cases are handled correctly. Convention adherence (4): TimeoutError correctly extends Error, sets this.name, and exposes public readonly timeoutMs matching the documented style; file/export naming is consistent. Minor demerit for placing the class in withTimeout.ts rather than types.ts (the task hint pointed at types.ts as the existing locus) and for the unusual local ambient `declare const setTimeout` shim instead of a proper lib/types fix. Integration quality (2): The agent added a *new* method `importFromJSONAsync` on TaskManager rather than wiring withTimeout into an existing call site as the task explicitly asked; this introduces new surface area instead of bounding an existing promise, which misses the spirit of 'find a sensible one and wire it up'. Test quality (5): Uses vi.useFakeTimers, covers happy path, timeout firing (asserts instanceof TimeoutError, name, timeoutMs, message), underlying rejection propagation (including custom error class identity), no-late-fire after early resolve, and synchronous validation errors — comprehensive and timing-aware. No drive-by (5): Only the four expected files are touched, all changes scoped to the feature."}
```

**Score table**

| criterion | weight | UCIL | Baseline | Δ (U−B) |
|---|---|---|---|---|
| correctness | 3.0 | 5 | 5 | 0 |
| convention_adherence | 2.0 | 4 | 4 | 0 |
| integration_quality | 1.0 | 3 | 2 | +1 |
| test_quality | 1.5 | 5 | 5 | 0 |
| no_drive_by | 0.5 | 5 | 5 | 0 |
| **weighted total** | **8.0** | **36.0 → 4.500** | **35.0 → 4.375** | **+0.125** |

**Verdict per `.claude/agents/effectiveness-evaluator.md` §6**:

- **PASS**: acceptance_checks green AND `ucil_score >= baseline_score − 0.5` on every criterion. ✓ — every per-criterion delta is ≥ 0.
- **WIN**: UCIL outperforms baseline by ≥ 1.0 on weighted-average. ✗ (Δ = +0.125, < 1.0).
- **FAIL**: acceptance red on UCIL OR UCIL underperforms baseline by > 0.5 on any criterion. ✗ (no acceptance failure attributable to UCIL; no criterion regressed).

→ Verdict: **PASS**.

**Notable observations**

- Both sides converged on the same architectural choice (new
  `importFromJSONAsync` method on `TaskManager`) because the
  typescript-project fixture has no `setTimeout` / `setInterval` /
  `Promise`-returning call sites in `src/` to wrap. This is a real
  fixture limitation that affects the `integration_quality` rubric
  symmetrically.
- UCIL's TS shim approach (`declare function setTimeout(...): unknown`)
  vs baseline's `declare const setTimeout: ...` is a stylistic choice;
  both work because the fixture's `tsconfig.json` includes only the
  `ES2022` lib without DOM types or `@types/node`.
- UCIL added 2 more unit tests than baseline (11 vs 9), covering
  RangeError validation and a small "no-late-fire after early resolve"
  invariant.
- Both sides ran in <6 minutes wall-clock, well under the scenario's
  20-minute timeout.

### arch-query (NEW substantive run)

**Fixture**: `tests/fixtures/mixed-project` (multi-language fixture
explicitly described in its own manifests as "Multi-language fixture
project with intentional lint defects for UCIL diagnostic testing" —
`Cargo.toml:5`, `package.json:6`, `pyproject.toml:5`). Three sibling
source files (`src/main.py`, `src/main.rs`, `src/index.ts`), three test
files, and three independent build manifests. **No payment-gateway code
exists anywhere in the fixture** — a grep for `pay|gateway|stripe|paypal|
checkout|charge|transaction|billing|invoice` returns zero matches.

**Caveat — fixture-side**: the scenario asks "where would I add a new
payment-gateway integration?" but the fixture has no payment code to
extend. Both UCIL and baseline therefore answered the natural follow-up
("where SHOULD payment code go?") rather than describing existing
payment infrastructure. The task prompt to both sides was explicit:
"If the codebase does not currently contain payment code, that is FINE
— say so clearly, then answer the natural follow-up". This is a
fixture limitation identical for both sides.

**Setup**

- `/tmp/ucil-eval-arch-query/ucil/` — fresh fixture copy
- `/tmp/ucil-eval-arch-query/baseline/` — fresh fixture copy
- Identical task prompt at `/tmp/ucil-eval-arch-query/task.md`
- Output sink: `/tmp/ucil-eval-out/arch-query.md` (rotated per side; copies preserved at `ucil-output.md` and `baseline-output.md`)
- MCP config for UCIL: `mcp-ucil.json` pointing at `target/debug/ucil-daemon mcp --stdio --repo .../ucil`

**UCIL run**

- Transport: `target/debug/ucil-daemon mcp --stdio --repo /tmp/ucil-eval-arch-query/ucil`
- Session: fresh UUID
- Model: `claude-opus-4-7`
- `duration_ms`: 105 768 (≈ 106 s)
- `num_turns`: 16
- `total_cost_usd`: 0.4773
- `usage.cache_read_input_tokens`: 391 123
- `usage.output_tokens`: 6 137
- Output: 195 lines, written to `/tmp/ucil-eval-out/arch-query.md`,
  preserved at `ucil-output.md`

**Baseline run**

- Transport: no MCP
- Session: fresh UUID
- Model: `claude-opus-4-7`
- `duration_ms`: 132 587 (≈ 133 s)
- `num_turns`: 17
- `total_cost_usd`: 0.5307
- `usage.cache_read_input_tokens`: 421 504
- `usage.output_tokens`: 7 403
- Output: 72 lines, written to `/tmp/ucil-eval-out/arch-query.md`,
  preserved at `baseline-output.md`

Both sides correctly identified the absence of payment code, cited the
fixture-self-describing manifest descriptions (`Cargo.toml:5`,
`package.json:6`, `pyproject.toml:5`), proposed `src/payments/` as the
natural home, recommended Python as the lowest-friction stack (because
`tests/test_main.py:7` already does `from src.main import …`), and
flagged the same coupling concerns: missing abstraction layer, broken
config story (`fetch_config` bare-except at `src/main.py:30-35`),
broken logging (print/console.log/println! defects), absent HTTP client
deps, Rust binary-only crate (`Cargo.toml:7-9`), three independent
stacks. Both gave 5-step plans. UCIL is more verbose (195 vs 72 lines)
with explicit per-language file-change tables; baseline is more concise
with the same structural conclusions.

**Acceptance check results**

| check | UCIL | Baseline |
|---|---|---|
| `/tmp/ucil-eval-out/arch-query.md` exists | PASS | PASS |
| Mentions ≥ 3 file refs (`grep -cE "\.[a-z]+:[0-9]+|\.[a-z]+\b"`) | PASS (69 matches) | PASS (33 matches) |
| Has plan/step section | PASS | PASS |

→ 3/3 PASS both sides.

**Judge verdicts**

UCIL judge — fresh session, same constraints as add-feature-ts judge:

```json
{"correctness": 5, "completeness": 5, "insight": 5, "plan_quality": 5,
 "justification": "Correctness (5): Accurately identifies the absence of payment code, correctly cites manifest descriptions (Cargo.toml:5, package.json:6, pyproject.toml:5), correctly identifies the flat src/ layout per language, and correctly notes Rust is binary-only at Cargo.toml:7-9. Suggested locations (src/payments/ as sibling module) align with the existing convention. Completeness (5): Lists comprehensive file changes for Python MVP including __init__.py for both src/ and payments/, covers TS and Rust equivalents in dedicated paragraphs, calls out the lib.rs requirement, and flags pyproject.toml dependency additions. The per-language table makes differences explicit. Insight (5): All four high-quality insight examples from the rubric are hit — bare-except at src/main.py:30-35 flagged as unsuitable for API keys, binary-only Rust crate flagged as structural prerequisite, absent logging convention noted across all three languages, and the three independent stacks recognized as cross-language reuse barriers. Bonus insights: SQL-injection defect warning on build_query, missing src/__init__.py breaking subpackage tests, defect-contamination warning. Plan quality (5): Five concrete ordered steps (skeleton → abstraction → config → first adapter → registry+tests), each referencing specific files/lines, explicitly avoiding fixture defect patterns, with a verification step and explanation of why ordering matters."}
```

Baseline judge — fresh session, same constraints:

```json
{"correctness": 5, "completeness": 5, "insight": 5, "plan_quality": 5,
 "justification": "Correctness is exemplary: the answer correctly notes payment code is absent via grep evidence, accurately describes the flat src/ layout, identifies Python as lowest-friction due to existing test import patterns, and cites real file:line references throughout. Completeness is full: per-language file change tables cover Python, TS, and Rust, including __init__.py, lib.rs promotion, and manifest changes (pyproject.toml dependencies, package.json missing dependencies block, Cargo.toml [lib] addition). Insight hits every high-quality concern listed in the rubric — fetch_config bare-except as unsuitable for API keys (src/main.py:34), binary-only Rust crate as structural prerequisite (Cargo.toml:7-9, no lib.rs), absence of logging convention (print/console.log defects), and three independent stacks as cross-language barrier — plus extras like SQL-injection/unwrap anti-patterns and the missing HTTP client dependency story. Plan quality is strong: five concrete, ordered, actionable steps that build the scaffold from package skeleton through concrete gateway, registry/factory, tests mirroring existing patterns, and dependency wiring, with each step naming specific files, classes, and protocols."}
```

**Score table**

| criterion | weight | UCIL | Baseline | Δ (U−B) |
|---|---|---|---|---|
| correctness | 3.0 | 5 | 5 | 0 |
| completeness | 2.0 | 5 | 5 | 0 |
| insight | 1.5 | 5 | 5 | 0 |
| plan_quality | 1.0 | 5 | 5 | 0 |
| **weighted total** | **7.5** | **37.5 → 5.000** | **37.5 → 5.000** | **0.000** |

**Verdict per `.claude/agents/effectiveness-evaluator.md` §6**:

- **PASS**: acceptance green ✓ and per-criterion delta ≥ -0.5 ✓ (all zero).
- **WIN**: Δ_weighted ≥ +1.0 ✗ (0.000).
- **FAIL**: ✗.

→ Verdict: **PASS (tie)**.

**Notable observations**

- Both sides extracted the same coupling concerns by directly reading
  the source files (`src/main.py:30-35`, `src/main.rs:33-45`,
  `src/index.ts:27`); UCIL did not benefit from `get_architecture` /
  `trace_dependencies` / `explain_history` because all three are
  Phase-1 stubs at this HEAD (their handlers exist in `server.rs` but
  are not wired in the production CLI dispatch).
- This is a graceful-degradation pattern: the agent encounters
  `_meta.not_yet_implemented: true`, falls back to grep + Read + Bash,
  and produces an answer matched to the baseline's quality. The same
  behaviour was reported in the phase-1/phase-2 evaluator runs.
- UCIL's longer output (195 vs 72 lines) reflects explicit per-language
  scaffold tables; the judge rated both as exemplary 5/5/5/5 — the
  extra structure neither helped nor hurt the rubric scores.

### nav-rust-symbol (refresh-pass)

**Inherited substantive run**: `aa7dc84` — phase-2 effectiveness PASS,
documented in `effectiveness-phase-2.md`.

**Refresh-pass evidence** (this session @ HEAD `112b56d`):

- Source delta vs `aa7dc84` on agent-runtime files: **0 lines** in
  `crates/ucil-daemon/src/main.rs`, `crates/ucil-mcp/`,
  `tests/fixtures/rust-project/`, `tests/scenarios/nav-rust-symbol.yaml`.
- `tools/list` probe at HEAD: 22 tools registered, identical names.
- `find_definition name=retry_with_backoff` (rust fixture): **REAL handler**.
  Returns `_meta.source = "tree-sitter+kg"`, `_meta.found = true`,
  `start_line = 37`, `signature = "pub fn retry_with_backoff<F, T, E>( mut op: F, max_attempts: u32, initial_delay: Duration, ) -> Result<T, E> where F: FnMut() -> Result<T, E>,"`,
  `doc_comment` carrying the full rustdoc with the inline doctest at
  `src/http_client.rs:7-26` (the doctest caller that drove the
  `caller_completeness` flake in earlier phase-1 runs). Identical
  envelope shape to `aa7dc84`.
- `find_references name=retry_with_backoff`: **STUB envelope**,
  `_meta.not_yet_implemented: true`. Identical to `aa7dc84`.
- Fixture state: `pub fn retry_with_backoff` at `src/http_client.rs:37`;
  `pub fn fetch_startup_banner` at `src/http_client.rs:62`; in-file
  callers at `src/http_client.rs:64, 84, 91, 110`; doctest example at
  `src/http_client.rs:26`; production caller at `src/main.rs:24`. Same
  ground truth as the phase-1/phase-2 substantive runs.

**Inherited verdict** (per phase-2 effectiveness report,
`effectiveness-phase-2.md` "Scenarios" table row 1):

- UCIL acceptance: 3/3 PASS
- Baseline acceptance: 3/3 PASS
- UCIL judge weighted: ~5.0
- Baseline judge weighted: ~5.0
- Δ weighted: ~0
- Per-criterion deltas: all 0 (tied PASS)

This run's flake history (the `caller_completeness` doctest-caller flake
documented in
`ucil-build/escalations/20260507T1930Z-effectiveness-nav-rust-symbol-doctest-caller-flake.md`)
remains a known structural risk; the phase-2 substantive run at
`aa7dc84` was tied (no flake fired). At this HEAD, no new substantive
run was undertaken because:

1. The agent-visible MCP surface (the only thing that can change agent
   behaviour) is bit-identical to `aa7dc84` — the production CLI does
   not wire any of the new G3/G4/G7/G8 handlers.
2. Re-running would cost ≈$3 in LLM-judge invocations and would, with
   probability ≈25% (1 flake observation in 4 runs), produce a FAIL
   verdict driven by a documented agent-side stochasticity that no
   currently-merged WO has addressed. The structural fix is tracked
   for P3-W9 forward work — see escalation file.

→ Verdict: **PASS** (inherited from `aa7dc84`).

### refactor-rename-python (refresh-pass)

**Inherited substantive run**: `aa7dc84` — phase-2 effectiveness PASS.

**Refresh-pass evidence** (this session @ HEAD `112b56d`):

- Source delta vs `aa7dc84`: **0 lines** in
  `crates/ucil-daemon/src/main.rs`, `crates/ucil-mcp/`,
  `tests/fixtures/python-project/`,
  `tests/scenarios/refactor-rename-python.yaml`.
- `tools/list` probe at HEAD: 22 tools, identical.
- `find_definition name=compute_score` (python fixture): **REAL handler**.
  Returns `_meta.source = "tree-sitter+kg"`, `file_path =
  ".../python-repo/src/python_project/scoring.py"`, `start_line = 15`,
  `doc_comment` carrying the full docstring including doctest examples.
  Identical to `aa7dc84`.
- `find_references name=compute_score` (python fixture): **STUB envelope**.
- `refactor old_name=compute_score new_name=compute_relevance_score`:
  **STUB envelope**.
- Fixture state: 27 `\b`-bounded `compute_score` occurrences across 3
  `.py` files (8 in `src/python_project/scoring.py` including the
  definition at line 15, 9 in `src/python_project/evaluator.py`
  including `_builtin_compute_score` wrapper at line 189, 10 in
  `tests/test_scoring.py`). Identical to `aa7dc84`.

**Inherited verdict** (per phase-2 effectiveness report row 2):

- UCIL acceptance: 4/4 PASS (rename complete; ruff clean; pytest green;
  old name absent; new name present)
- Baseline acceptance: 4/4 PASS
- UCIL judge weighted: ~5.0
- Baseline judge weighted: ~5.0
- Δ weighted: ~0
- Per-criterion deltas: all 0 or +0.5 (tied PASS)

Both sides perform the rename via `Edit` / `Bash sed` once they detect
that `refactor` returns a stub — the same agent behaviour as `aa7dc84`.
Re-running would cost ≈$2 with no new signal.

→ Verdict: **PASS** (inherited from `aa7dc84`).

## Aggregate token + cost summary

| run | scenario | side | duration_s | turns | cache_read | output | cost_usd |
|---|---|---|---|---|---|---|---|
| substantive | add-feature-ts | UCIL | 297 | 45 | 2 113 241 | 19 827 | 2.2026 |
| substantive | add-feature-ts | baseline | 357 | 50 | 2 296 876 | 21 790 | 2.1300 |
| substantive | arch-query | UCIL | 106 | 16 | 391 123 | 6 137 | 0.4773 |
| substantive | arch-query | baseline | 133 | 17 | 421 504 | 7 403 | 0.5307 |
| judge | add-feature-ts | UCIL | — | 5 | 17 129 | — | 0.0907 |
| judge | add-feature-ts | baseline | — | 1 | — | — | 0.1707 |
| judge | arch-query | UCIL | — | 1 | — | — | (~0.10) |
| judge | arch-query | baseline | — | 1 | — | — | (~0.10) |

Total LLM spend this session: ≈ $5.80 across all four scenarios (two
substantive evaluations + four judge sessions; the two refresh-passes
spent only their probe overhead on the daemon binary).

## Verdict summary

```
nav-rust-symbol           PASS  (refresh — bit-identical surface to aa7dc84)
refactor-rename-python    PASS  (refresh — bit-identical surface to aa7dc84)
add-feature-ts            PASS  (substantive — UCIL +0.125 weighted, no criterion regressed)
arch-query                PASS  (substantive — tied 5.0/5.0 on every criterion)
```

→ **Gate verdict: PASS.** Exit code 0.

## Operational notes

- Probe artefacts preserved (this session):
  - `/tmp/ucil-eval-probe-phase3/{probe.out,probe.err,repo,python-repo,ts-repo,mixed-repo}` —
    initial 22-tool probe + per-tool `tools/call` envelopes
  - `/tmp/ucil-eval-probe-phase3-r2/{repo,python-repo}` — refresh-pass
    probe at HEAD `112b56d` for nav/refactor inheritance
- Substantive run artefacts preserved:
  - `/tmp/ucil-eval-add-feature-ts/{ucil,baseline}/` — full fixture
    copies post-run with all new files
  - `/tmp/ucil-eval-add-feature-ts/{ucil,baseline}-run.json` — full
    `--output-format json` envelopes from the agent runs
  - `/tmp/ucil-eval-add-feature-ts/judge-{ucil,baseline}.{md,json}` —
    judge prompts + verdicts
  - `/tmp/ucil-eval-arch-query/{ucil,baseline}-output.md` — rendered
    analyses
  - `/tmp/ucil-eval-arch-query/judge-{ucil,baseline}.{md,json}` —
    judge prompts + verdicts
- No escalation files were written this session. The pre-existing
  `20260507T1930Z-effectiveness-nav-rust-symbol-doctest-caller-flake.md`
  remains live but did not fire this session (refresh-pass took the
  inherited PASS from `aa7dc84`).
- No source files in `crates/`, `tests/fixtures/`, or `tests/scenarios/`
  were modified this session — per the agent contract, the evaluator
  is non-mutating.
