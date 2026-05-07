# Effectiveness Report — Phase 2

Run at: 2026-05-07T16:37Z
Commit: `d3c3c03707a46b73b2fcb43c6d85ba4fb608ec55`
Branch: `main`
Evaluator: `effectiveness-evaluator` (fresh session, `claude-opus-4-7`)

## Summary

| metric | value |
|---|---|
| Scenarios discovered for phase 2 | 2 |
| Scenarios run | 1 |
| Scenarios skipped (`tool_not_ready`) | 0 |
| Scenarios skipped (`scenario_defect`) | 1 |
| Scenarios PASS | 0 |
| Scenarios WIN | 0 |
| Scenarios FAIL | 1 |

**Gate verdict: FAIL (per strict contract reading).** Of the two
phase-2-eligible scenarios, `refactor-rename-python` was skipped with
`skipped_scenario_defect` (its task asserts the existence of a
`compute_score` function that the `python-project` fixture does not
contain — see escalation
`20260507T1629Z-effectiveness-refactor-rename-python-fixture-missing-symbol.md`).
The remaining scenario `nav-rust-symbol` was executed end-to-end. Both
UCIL and baseline produced substantively-correct answers ("no qualifying
HTTP-retry-with-exponential-backoff functions exist", matching ground
truth). LLM judges scored UCIL **5 / 5 / 5 / 4** and baseline **5 / 5 / 5
/ 5** across `correctness / caller_completeness / precision / formatting`.
Weighted means: UCIL **4.9231**, baseline **5.0000**, **Δ weighted =
−0.0769** (UCIL ≈ baseline within judge noise).

**Why the surviving scenario FAILs:**
1. Acceptance check #3 (`cites at least one .rs:LINE`) was RED on the
   **UCIL** side (PASS on baseline this run). Per
   `.claude/agents/effectiveness-evaluator.md` §6 ("FAIL: acceptance_checks
   red on UCIL run"), this is a per-scenario FAIL.
2. The UCIL judge gave `formatting = 4` while baseline got `formatting =
   5` (Δ = −1.0, exceeds the 0.5 tolerance window). Per the same §6
   ("OR UCIL underperforms baseline by > 0.5 on any criterion"), this is
   a second independent FAIL trigger.

Both fail-triggers are **driven by a single root cause**: the truthful
answer to this task on this fixture ("no qualifying functions") naturally
contains no `.rs:LINE` tokens. UCIL chose to enumerate negative-search
keywords without citing fixture file:line tokens; baseline incidentally
emitted `src/parser.rs:617,621,738` and `src/eval_ctx.rs:49`,
`src/util.rs:10` tokens as "incidental substring hits inside unrelated
identifiers" (the word "surface"). Whether either side emits a `.rs:LINE`
token in evidence prose is **stochastic LLM narrative style**, not a
substantive UCIL regression.

This is the **fourth recorded instance** of the
scenario-fixture-alignment flake described in escalation
`20260507T0357Z-effectiveness-nav-rust-symbol-rs-line-flake.md`. The
historical pattern (one PASS/PASS, one FAIL/FAIL, two FAIL/PASS) confirms
the flake is bidirectional. The substantive UCIL behaviour is
indistinguishable from baseline on this null-ground-truth scenario.

## Tool-availability probe

`tools/list` against `ucil-daemon mcp --stdio --repo /tmp/ucil-mcp-probe/repo`
reported all 22 §3.2 tools registered. The two tools required by
`nav-rust-symbol` (`find_definition`, `find_references`) are both
registered.

| tool | listed? | tools/call returns real data? |
|---|---|---|
| `find_definition` | yes | yes — `_meta.source = "tree-sitter+kg"`, `found=true`, real `file_path`/`start_line`/`qualified_name` |
| `find_references` | yes | **no** — handler returns `_meta.not_yet_implemented: true` |
| `refactor` | yes | **no** — handler returns `_meta.not_yet_implemented: true` |
| `search_code` | yes | yes — `_meta.count`, `results[]` populated |

Note on `find_references`: `feature-list.json` records `P2-W7-F05` as
`passes=true` (verified by `verifier-d249db74-...`), but the MCP-layer
routing in `crates/ucil-daemon/src/server.rs:700-758` still falls through
to the Phase-1 stub for `find_references` and `refactor`. The fusion-layer
implementation exists (per `executor.rs:497`, `:792`, `:965`, `:1177`,
`:1222`, `:2109`, `:2383` references), but a follow-up WO is needed to
swap the stub dispatch in `dispatch_tools_call` for the real fusion path.
This does **not** change the eligibility for this scenario per
§"Tool-availability checks" ("operational" = registered + responsive),
matching the precedent from the phase-1 report.

## Scenarios

| scenario | requires_tools | tools_real | UCIL pass? | UCIL score | Baseline score | Δ weighted | verdict |
|---|---|---|---|---|---|---|---|
| `nav-rust-symbol` | `find_definition`, `find_references` | `find_definition` (real); `find_references` (stub — `_meta.not_yet_implemented: true`) | acceptance partial: 2 of 3 checks PASS, `cites file:line` FAIL on UCIL (PASS on baseline) | 4.9231 | 5.0000 | −0.0769 | **FAIL** |
| `refactor-rename-python` | `find_references`, `refactor` | n/a — scenario skipped before tool-call | n/a | n/a | n/a | n/a | **`skipped_scenario_defect`** |

## Per-scenario detail

### nav-rust-symbol

**Fixture**: `tests/fixtures/rust-project` — a self-contained expression
parser/evaluator. `Cargo.toml` declares zero dependencies; case-insensitive
grep across the fixture for any of `retry / backoff / exponential / http /
reqwest / hyper / tokio / async / Client / fetch / request / attempt /
max_retries / jitter / sleep` returns zero match in any source file.

**Ground truth**: "no HTTP-retry exponential-backoff functions exist".
Confirmed by independent grep + per-file SHA-256 inventory at
`/tmp/ucil-eval-nav-rust-symbol/fixture-checksum.txt` (9 files,
byte-for-byte identical to the fixture committed at HEAD).

**Setup**
- `/tmp/ucil-eval-nav-rust-symbol/ucil/` — fresh fixture copy for UCIL run
- `/tmp/ucil-eval-nav-rust-symbol/baseline/` — fresh fixture copy for baseline run
- Output sink: `/tmp/ucil-eval-out/nav-rust-symbol.md` (rotated per side)
- Identical task prompt for both sides modulo "Fixture root: …" line; both prompts captured verbatim at `ucil-prompt.md` / `baseline-prompt.md`

**UCIL run**
- Transport: `ucil-daemon mcp --stdio --repo /tmp/ucil-eval-nav-rust-symbol/ucil`
- MCP config: `--mcp-config $ROOT/mcp-ucil.json --strict-mcp-config`
- Settings: `--setting-sources ""` (no project / user / local settings)
- Session: `9e798e5d-5858-463c-8190-f0cbd1cb979d` (fresh, deterministic UUID)
- Model: `claude-opus-4-7`
- `duration_ms`: 152 966 (≈ 153 s)
- `num_turns`: 30
- `total_cost_usd`: 0.8548
- `usage.input_tokens`: 44
- `usage.cache_read_input_tokens`: 812 660
- `usage.cache_creation_input_tokens`: 37 865
- `usage.output_tokens`: 8 437
- `is_error`: false
- Output: 45 lines, written to `/tmp/ucil-eval-out/nav-rust-symbol.md`,
  preserved at `/tmp/ucil-eval-nav-rust-symbol/ucil-output.md`

**UCIL agent self-report** (excerpted from output, `ucil-output.md` lines 36–45):

> *"This run was intended to use the UCIL MCP tools, but the deferred-tool
> schemas surfaced for `mcp__ucil__search_code`, `mcp__ucil__find_definition`,
> and `mcp__ucil__understand_code` did not expose the required `query` /
> `name` / `target` arguments (despite `additionalProperties: true`), so
> each invocation was rejected with `InputValidationError`. The conclusion
> above was therefore verified with `Grep` + `Read` against the fixture;
> the result (no HTTP retry/backoff functions exist) is independent of
> which tool is used."*

This is a real UCIL surface defect orthogonal to the scenario-fixture
flake: the `inputSchema` returned by `tools/list` advertises only the
universal CEQP fields (`current_task`, `files_in_context`, `reason`,
`token_budget`) plus `additionalProperties: true`, but Claude Code's
deferred-tool dispatcher rejects calls whose arguments are not in the
schema's `properties` map. Filed for triage as a follow-up note in this
report; no separate escalation since the scenario's substantive answer is
correct on grep+Read alone for this null-ground-truth fixture, and a
positive-match scenario will eventually exercise the path under the
fixture-augmentation remediation tracked in
`20260507T0357Z-effectiveness-nav-rust-symbol-rs-line-flake.md`.

**Baseline run**
- Transport: no MCP servers (`mcp-empty.json` with empty `mcpServers`)
- MCP config: `--mcp-config $ROOT/mcp-empty.json --strict-mcp-config`
- Settings: `--setting-sources ""` (no project / user / local settings)
- Session: `f095d7b1-e07b-44df-a2c0-c9262f279cd7` (fresh, deterministic UUID)
- Model: `claude-opus-4-7`
- `duration_ms`: 77 955 (≈ 78 s)
- `num_turns`: 18
- `total_cost_usd`: 0.5460
- `usage.input_tokens`: 23
- `usage.cache_read_input_tokens`: 574 256
- `usage.cache_creation_input_tokens`: 23 129
- `usage.output_tokens`: 4 539
- `is_error`: false
- Output: 55 lines, written to `/tmp/ucil-eval-out/nav-rust-symbol.md`,
  preserved at `/tmp/ucil-eval-nav-rust-symbol/baseline-output.md`

**Acceptance checks**

| check | UCIL | Baseline |
|---|---|---|
| `test -f /tmp/ucil-eval-out/nav-rust-symbol.md` | PASS | PASS |
| `test $(wc -l < /tmp/ucil-eval-out/nav-rust-symbol.md) -ge 5` | PASS (45) | PASS (55) |
| `grep -qE "\.rs:[0-9]+" /tmp/ucil-eval-out/nav-rust-symbol.md` | **FAIL** | **PASS** |

The UCIL output reports its negative-search keywords inline but does not
quote any `<file>.rs:<line>` token; the closest reference is the prose
`mcp__ucil__search_code` / `mcp__ucil__find_definition` mentions which are
not `.rs:LINE` shape. The baseline output incidentally cites
`src/parser.rs:617,621,738`, `src/eval_ctx.rs:49`, and `src/util.rs:10`
under the disclaimer *"only `surf` substring matches were incidental hits
inside `surface` in documentation prose"*, satisfying the grep
acceptance check trivially. Neither side claimed any function exists; both
correctly concluded "no qualifying functions" — the acceptance check
diverges purely on whether the agent happened to emit the literal
`<file>.rs:<line>` token in passing.

**Judge scoring** (fresh `claude -p` session per side, run from
`cd /tmp` so the repo's project hooks/settings cannot interfere;
`--setting-sources ""`, `--strict-mcp-config` with empty server map; rubric
copied verbatim from the scenario yaml; ground truth disclosed to the
judge so it can score correctness against the truth, not against the
agent's own claims)

| criterion | weight | UCIL | Baseline | Δ |
|---|---|---|---|---|
| correctness         | 3.0 | 5 | 5 | 0 |
| caller_completeness | 2.0 | 5 | 5 | 0 |
| precision           | 1.0 | 5 | 5 | 0 |
| formatting          | 0.5 | **4** | **5** | **−1** |
| **weighted mean**   |     | **4.9231** | **5.0000** | **−0.0769** |

UCIL judge (`6fe5e217-36fe-4169-be29-503af047b6b1`):

> *"The solution correctly arrives at the ground-truth answer: no
> functions in the fixture perform HTTP retry with exponential backoff.
> The agent justifies the empty result with concrete evidence (crate
> description, module layout, Cargo.lock dependency audit, an explicit
> list of negative search terms covering HTTP clients, async runtimes,
> and backoff vocabulary), so correctness and precision are exemplary,
> and caller_completeness is vacuously satisfied. Formatting loses a
> point because the spec called for H2-per-function with bulleted callers
> and the mechanical `\.rs:[0-9]+` check failed; while an empty result
> set genuinely cannot produce per-function H2s, the document still
> deviates from the prescribed shape rather than, e.g., emitting an
> explicit 'none found' section that hugs the template."*

Baseline judge (`016648f0-7a42-431b-ae73-366bcc0c541e`):

> *"The solution correctly identifies that no qualifying functions exist,
> matching ground truth. It provides strong evidence: documents the
> project's purpose from Cargo.toml, verifies zero external dependencies
> via Cargo.lock, lists all source files with line counts, and reports
> an exhaustive keyword search across HTTP/retry/backoff/async/networking
> terms. It even acknowledges incidental substring hits (e.g. 'surface')
> and explains why they don't qualify. Caller completeness and precision
> are vacuously satisfied with an empty result set. Formatting is
> appropriate for a null result — the empty 'Functions found' section
> with '(none)' is a reasonable rendering of the H2-per-function
> structure when there are zero functions."*

Both judges returned clean JSON on first attempt.

**Verdict: FAIL** (per strict contract: UCIL acceptance_checks contain a
red AND UCIL formatting underperforms baseline by 1.0 — both same
narrative-style coin flip on `.rs:LINE` token emission).

### refactor-rename-python — `skipped_scenario_defect`

**Reason for skip**: the scenario's task asserts *"In the Python fixture
at the current working directory, there is a function named
`compute_score`"* — but `tests/fixtures/python-project/` is a
self-contained interpreter (lexer / parser / evaluator) with **zero
occurrences of `compute_score`** anywhere. The fixture's surface is
`Lexer / Parser / Evaluator / Environment / Token / ASTNode / Value` —
60+ functions, none related to scoring or relevance.

```
$ grep -rn "compute_score\|compute_relevance" tests/fixtures/python-project/
(no output — exit 1)
```

The scenario's acceptance check #2 (`grep -rn --include="*.py"
"\bcompute_relevance_score\b" .`) cannot be satisfied without the agent
fabricating a function under that name. A truthful agent (UCIL or
baseline) would correctly report "no `compute_score` function exists in
this fixture; nothing to rename", which is the right answer to the
actual fixture state but FAILs acceptance check #2 deterministically.

Per `.claude/agents/effectiveness-evaluator.md` §"Hard rules":

> If a scenario is bad (ambiguous task, impossible-to-score rubric), file
> an escalation describing the defect and skip it with
> `skipped_scenario_defect`.

This is the action taken. Escalation filed at
`ucil-build/escalations/20260507T1629Z-effectiveness-refactor-rename-python-fixture-missing-symbol.md`
with three remediation options for planner/ADR triage (augment fixture,
rewrite scenario to target an existing symbol, defer to Phase-8 audit).
No UCIL run, no baseline run, no judge call.

This is structurally analogous to the existing `nav-rust-symbol` /
`rust-project` escalation
(`20260507T0357Z-effectiveness-nav-rust-symbol-rs-line-flake.md`); both
are scenario-fixture-alignment defects. Recommend triaging together in a
Phase-8 dedicated audit work-order (per the prior escalation's resolution
note).

## Observations

- **Substantive parity, structural fail.** The UCIL surface produces the
  same correct null-ground-truth answer as baseline on `nav-rust-symbol`,
  with all weight ≥ 1.0 criteria scoring 5/5 on both sides. The only
  differentiator is the 0.5-weighted `formatting` criterion (UCIL 4,
  baseline 5) — judge noise tied to the same `.rs:LINE` coin flip that
  drives the acceptance check. Net Δ-weighted = −0.0769.

- **Cost-edge to baseline (informational).** UCIL was ~1.96× slower this
  run (153 s vs 78 s) and used ~1.41× more cache-read tokens (812 K vs
  574 K) and ~1.86× more output tokens (8 437 vs 4 539). UCIL spent a
  number of turns probing MCP tool schemas before falling through to
  `Glob` / `Grep` / `Read`. For a positive-match task with `find_references`
  wired through the MCP layer, the advantage shape would flip; that path
  is gated on the `find_references` MCP-router patch noted in the probe
  table.

- **Stochastic acceptance-check satisfaction (recurrent).** Across the
  five known runs at five commits:

  | commit | UCIL acceptance #3 | Baseline acceptance #3 |
  |---|---|---|
  | `70aa72e` | PASS | PASS |
  | `f4adc41` | FAIL | FAIL |
  | `762bd5d` | FAIL | PASS |
  | `1f20c3b` | FAIL | FAIL |
  | `d3c3c03` (this run) | **FAIL** | **PASS** |

  Same fixture, same scenario, same UCIL surface. **This is the fifth
  instance of the documented flake mode** (and the second showing
  baseline-PASS / UCIL-FAIL asymmetry).

- **`find_references` MCP routing is still a stub.** `feature-list.json`
  records `P2-W7-F05` as `passes=true`, but the dispatch logic in
  `crates/ucil-daemon/src/server.rs:700-758` still routes
  `find_references` to the Phase-1 stub envelope. The fusion-layer impl
  exists per `executor.rs` cross-references; a follow-up MCP-router WO
  is required to make the tool live to MCP clients. Did NOT bite this
  scenario (null ground truth — nothing to cross-reference) but will
  matter for any positive-match nav scenario.

- **Tool inputSchema gap (UCIL surface bug).** The UCIL agent's
  self-report flags that `tools/list` returns `inputSchema` containing
  only the four CEQP universal fields plus `additionalProperties: true`,
  with no per-tool `properties` (`query` for `search_code`, `name` for
  `find_definition`, `target` for refactoring tools, etc.). Claude
  Code's deferred-tool dispatcher rejects calls with
  `InputValidationError` when arguments aren't in the listed `properties`
  map, even with `additionalProperties: true`. This forces the agent to
  fall through to built-in `Grep` / `Read` rather than using UCIL tools.
  This is orthogonal to the scenario-fixture flake but is the more
  load-bearing finding from this run — it should be fixed by extending
  the static `tools_definitions()` table in
  `crates/ucil-daemon/src/server.rs:280-369` so each tool's
  `inputSchema.properties` includes the per-tool argument fields it
  actually consumes (e.g. `find_definition` takes `name` and optional
  `file_scope`; `search_code` takes `query` and optional `max_results`).

- **Reproducibility note.** Judge sessions ran from `cd /tmp` with
  explicit `--setting-sources ""` to avoid the repo Stop-hook hijacking
  documented in the prior reports. Both judges returned clean JSON on
  first attempt.

## Substantive judge-tie line

Per-criterion deltas (UCIL − Baseline):
- correctness: 0
- caller_completeness: 0
- precision: 0
- formatting: −1 (entirely attributable to the missing `.rs:LINE` token,
  which is the same coin flip as acceptance check #3)

Weighted-mean delta: **−0.0769** (UCIL 4.9231 vs baseline 5.0000).

UCIL did not regress on any substantive criterion. The single-point
formatting gap is a downstream symptom of the same narrative-style flake
that drives acceptance check #3. The substantive UCIL behaviour is
**indistinguishable from baseline** on this null-ground-truth scenario.

## Gate contract (why this run is FAIL)

Per `.claude/agents/effectiveness-evaluator.md` §6 "Per-scenario verdict":

> **PASS**: `acceptance_checks` green AND `ucil_score >= baseline_score - 0.5` on every criterion.
> **WIN**: UCIL outperforms baseline by at least 1.0 on the weighted-average score.
> **FAIL**: `acceptance_checks` red on UCIL run, OR UCIL underperforms baseline by > 0.5 on any criterion.

Two FAIL triggers fire on the surviving scenario:
1. UCIL acceptance_check `cites at least one file:line` is RED.
2. UCIL `formatting` = 4 vs baseline `formatting` = 5 (Δ = −1.0 > 0.5).

Both triggers are downstream of the same narrative-style coin flip
documented above and in the existing escalation. The substantive tie at
4.9231 / 5.0000 weighted-mean (Δ = −0.0769) is recorded but does not
override the strict-letter verdict.

Per `.claude/agents/effectiveness-evaluator.md` §"Exit code":

> 0 if gate passes, 1 if any scenario FAIL, 2 on evaluator-internal error.

One scenario FAIL recorded → **exit 1**.

## Reproducibility

All artefacts of this run are preserved under
`/tmp/ucil-eval-nav-rust-symbol/`:

- `task.md` — scenario task prompt (verbatim from yaml)
- `ucil-prompt.md`, `baseline-prompt.md` — full agent task prompts (with fixture root)
- `ucil-output.md`, `baseline-output.md` — raw agent outputs
- `ucil-run.json`, `baseline-run.json` — `claude -p` JSON envelopes
  (duration, tokens, session ids, stop reason)
- `ucil-run.stderr`, `baseline-run.stderr` — child-process stderr
- `mcp-ucil.json`, `mcp-empty.json` — MCP configs for UCIL and baseline
- `run-ucil.sh`, `run-baseline.sh` — exact `claude` invocations
- `run-judge-ucil.sh`, `run-judge-baseline.sh` — judge invocations
- `judge-ucil-prompt.md` (`/tmp/ucil-eval-judge-nav-rust-symbol-ucil.md`),
  `judge-baseline-prompt.md` (`/tmp/ucil-eval-judge-nav-rust-symbol-baseline.md`)
  — verbatim judge prompts
- `judge-ucil-raw.json`, `judge-baseline-raw.json` — judge `claude -p`
  JSON envelopes
- `judge-ucil.json`, `judge-baseline.json` — extracted scoring JSON
- `ucil-acceptance.txt`, `baseline-acceptance.txt` — acceptance check results
- `ucil-session-id`, `baseline-session-id`, `judge-ucil-session-id`,
  `judge-baseline-session-id` — session UUIDs
- `fixture-checksum.txt` — per-file SHA-256 of the fixture (9 files)

## Advisory — recurring scenario-fixture defects (Phase-8 audit recommended)

This phase-2 run records two distinct scenario-fixture-alignment defects:

1. **`nav-rust-symbol` × `rust-project`** — fifth instance of the
   `.rs:LINE` flake. Documented in
   `20260507T0357Z-effectiveness-nav-rust-symbol-rs-line-flake.md`.
   Resolution deferred to Phase-8 (planner/ADR-gated since `tests/fixtures`
   and `tests/scenarios` are protected).

2. **`refactor-rename-python` × `python-project`** — symbol referenced
   by scenario does not exist in fixture. Documented in
   `20260507T1629Z-effectiveness-refactor-rename-python-fixture-missing-symbol.md`
   (filed by this run).

Both defects share a structural shape: a scenario asserts the existence
of code that doesn't exist in the named fixture. A Phase-8
scenario-fixture-alignment WO should: (a) augment fixtures with the
referenced symbols, OR (b) rewrite scenarios to target existing symbols,
plus update `tests/scenarios/README.md` with a guard ("scenarios MUST
cite at least one identifier that `grep -q` finds in the named
fixture"). The current effectiveness gate is correctly identifying
substantive parity but is forced to FAIL by structural contract triggers
neither side can deterministically satisfy.

## Follow-up notes for the executor pool (no separate escalations filed)

1. **`find_references` MCP routing patch.** Add a real-handler dispatch
   to `crates/ucil-daemon/src/server.rs:700-758` that routes
   `find_references` to the existing fusion-layer code referenced from
   `executor.rs:497`/`:792`/`:965`/`:1177`/`:1222`/`:2109`/`:2383`,
   following the same KG-attached-router pattern used for
   `find_definition` / `get_conventions` / `search_code` /
   `understand_code` / `find_similar`. Same shape applies to `refactor`
   when its fusion-layer impl lands. Tracked under existing
   feature-list entry; not a regression.

2. **Tool inputSchema completeness.** Extend the static
   `tools_definitions()` table in `crates/ucil-daemon/src/server.rs:280-369`
   so each tool advertises its real per-tool argument fields under
   `inputSchema.properties`. Without this, Claude Code's deferred-tool
   dispatcher rejects MCP tool calls with `InputValidationError` and the
   agent falls through to built-in `Grep`/`Read`, neutralising the UCIL
   advantage. This is the most load-bearing finding from this run.
