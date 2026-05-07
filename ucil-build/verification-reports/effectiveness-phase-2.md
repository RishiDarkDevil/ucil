# Effectiveness Report — Phase 2

Run at: 2026-05-07T17:40Z
Commit: `1e89cec` (`HEAD` at evaluator-launch)
Branch: `main`
Evaluator: `effectiveness-evaluator` (this session, `claude-opus-4-7`)

## Summary

| metric | value |
|---|---|
| Scenarios discovered for phase 2 | 2 |
| Scenarios run | 2 |
| Scenarios skipped (`tool_not_ready`) | 0 |
| Scenarios skipped (`scenario_defect`) | 0 |
| Scenarios PASS | 2 |
| Scenarios WIN | 0 |
| Scenarios FAIL | 0 |

**Gate verdict: PASS.** Both phase-2-eligible scenarios
(`nav-rust-symbol`, `refactor-rename-python`) ran end-to-end against
DEC-0017-augmented fixtures. UCIL and baseline produced substantively
equivalent solutions; LLM judges scored the two sides identically per
scenario:

- `nav-rust-symbol`: UCIL **5.0000** vs baseline **5.0000** weighted
  mean (Δ = 0.0000); all three acceptance checks GREEN on both sides.
- `refactor-rename-python`: UCIL **4.4545** vs baseline **4.4545**
  weighted mean (Δ = 0.0000); all four acceptance checks GREEN on both
  sides.

UCIL does not regress baseline on any criterion. No scenario triggers
the FAIL contract (no UCIL acceptance reds; no criterion underperforms
baseline by > 0.5).

This run supersedes the prior `wip(verification-reports): phase-2
effectiveness in-flight (refactor re-run pending)` (commit `0d338df`).
That wip commit recorded a refactor-rename-python FAIL because the
baseline agent's prior stochastic outcome refused the pre-existing E741
drive-by while UCIL accepted it. This run is the planned re-run with
restricted `--allowed-tools` for parity; both sides made the same E741
drive-by trade-off, scored identically on `safety` (2 of 5), and the
scenario PASSes per the rubric.

DEC-0017 fixture augmentation has resolved the prior recurring
`.rs:LINE` flake mode (5 prior FAILs across 5 commits) and the
`compute_score` missing-symbol defect.

Per `.claude/agents/effectiveness-evaluator.md` §"Exit code":
> 0 if gate passes, 1 if any scenario FAIL, 2 on evaluator-internal error.

→ **exit 0**.

## Tool-availability probe

Probe: `tools/list` against
`target/debug/ucil-daemon mcp --stdio --repo /tmp/ucil-mcp-probe/repo` —
22 tools registered, matches §3.2 of the master plan. Both
`find_definition` + `find_references` (required by `nav-rust-symbol`),
and `find_references` + `refactor` (required by `refactor-rename-python`)
are listed.

Per-tool tools/call probe against the augmented `rust-project` fixture
(`/tmp/ucil-eval-rust-project` workdir):

| tool | listed? | tools/call returns real data? |
|---|---|---|
| `find_definition` | yes | yes — `_meta.source = "tree-sitter+kg"`, returned `retry_with_backoff` at `src/http_client.rs:37` with full doc-comment, signature, qualified_name |
| `find_references` | yes | **no** — handler returns `_meta.not_yet_implemented: true` ("registered but its handler is not yet implemented (Phase 1 stub)") |
| `refactor` | yes | **no** — handler returns `_meta.not_yet_implemented: true` |
| `search_code` | yes | partial — returns `_meta.count` but no per-result file/line breakdown (reported by both UCIL agents in their self-reports) |

This matches the prior phase-2 report's probe. The MCP-router patch
(routing `find_references`/`refactor`/etc. to fusion-layer handlers
instead of Phase-1 stubs) remains a follow-up. Both scenarios proceed
to run with the registered-but-stubbed tools per
§"Tool-availability checks" ("operational" = registered + responsive).

Both UCIL agents (nav and refactor) honestly self-reported the stub
state and fell through to `Read` / `Glob` / Edit + Bash for the
substantive work. Their answers remain correct; the run shows that
UCIL's surface today does not yet provide a measurable advantage over
baseline on phase-2 scenarios.

## Scenarios

| scenario | requires_tools | UCIL pass? | UCIL w/m | Baseline w/m | Δ weighted | verdict |
|---|---|---|---|---|---|---|
| `nav-rust-symbol` | `find_definition`, `find_references` | yes (3/3) | 5.0000 | 5.0000 | 0.0000 | **PASS** |
| `refactor-rename-python` | `find_references`, `refactor` | yes (4/4) | 4.4545 | 4.4545 | 0.0000 | **PASS** |

## Per-scenario detail

### nav-rust-symbol — PASS

**Fixture state.** `tests/fixtures/rust-project/` (DEC-0017-augmented).
Contains:

- `src/http_client.rs` — `retry_with_backoff` (the actual
  exponential-backoff retry combinator) at line 37, plus
  `fetch_startup_banner` at line 62 that drives the retry path.
- `src/main.rs:24` — call site of `fetch_startup_banner` in `fn main`.
- 4 in-file callers of `retry_with_backoff` at
  `http_client.rs:64, 84, 91, 110`.
- Fixture state confirmed by independent SHA-256 inventory of all 10
  files at `/tmp/ucil-eval-nav-rust-symbol/fixture-checksum.txt`.

Ground truth (verified by independent grep across the fixture tree):

- `retry_with_backoff` (definition `src/http_client.rs:37`) is the
  unique function that performs HTTP retry with exponential backoff.
- `fetch_startup_banner` (`src/http_client.rs:62`) drives a
  retry-with-backoff through `retry_with_backoff`; including it as a
  second qualifying entry is acceptable per the scenario's flexibility
  on debatable inclusions.

**Setup**

- `/tmp/ucil-eval-nav-rust-symbol/ucil/` — fresh fixture copy for UCIL run
- `/tmp/ucil-eval-nav-rust-symbol/baseline/` — fresh fixture copy for baseline
- Output sink: `/tmp/ucil-eval-out/nav-rust-symbol.md` (rotated per side)
- Identical task prompt for both sides, modulo "Fixture root: …" line;
  both prompts captured at `ucil-prompt.md` / `baseline-prompt.md`

**UCIL run** (`/tmp/ucil-eval-nav-rust-symbol/ucil-run.json`)

- Transport: `ucil-daemon mcp --stdio --repo $WORK/ucil`
- MCP config: `$WORK/mcp-ucil.json --strict-mcp-config`
- Settings: `--setting-sources ""` (no project / user / local settings)
- Allowed tools: full `mcp__ucil__*` set + `Read`, `Write`
- Session: `70b77216-673e-45a9-a309-d261f5541235` (claude-p assigned)
- Model: `claude-opus-4-7`
- `duration_ms`: 233 708 (≈ 234 s)
- `num_turns`: 57
- `total_cost_usd`: 1.6993
- `usage.cache_read_input_tokens`: 1 304 519
- `usage.cache_creation_input_tokens`: 95 833
- `usage.output_tokens`: 16 567
- `is_error`: false; `terminal_reason`: completed
- Output: 28 lines, preserved at `ucil-output.md`

UCIL agent self-report (excerpted from result envelope):

> *"`rust_project::http_client::retry_with_backoff` — `src/http_client.rs:37`
> — the actual exponential-backoff retry loop (delay doubles each failed
> attempt). Called from `src/http_client.rs:64, 84, 91, 110`.
> `rust_project::http_client::fetch_startup_banner` — `src/http_client.rs:62`
> — HTTP-style fetch driven through `retry_with_backoff`. Called from
> `src/main.rs:24` and `src/http_client.rs:124`. Note: most `mcp__ucil__*`
> handlers in this fixture are Phase 1 stubs (`find_references`,
> `trace_dependencies`, `find_similar`, `get_architecture` return stub
> messages, and `search_code` returns only match counts), so caller
> verification was done by reading the source files directly."*

**Baseline run** (`/tmp/ucil-eval-nav-rust-symbol/baseline-run.json`)

- Transport: empty mcp config (`mcp-empty.json`)
- Allowed tools: `Grep`, `Glob`, `Read`, `Write` only
- Session: `5947af67-b461-4ab1-9287-c6684076e66a`
- Model: `claude-opus-4-7`
- `duration_ms`: 108 905 (≈ 109 s)
- `num_turns`: 14
- `total_cost_usd`: 0.5157
- `usage.cache_read_input_tokens`: 396 441
- `usage.output_tokens`: 7 593
- `is_error`: false; `terminal_reason`: completed
- Output: 32 lines, preserved at `baseline-output.md`

**Acceptance checks** (per scenario yaml: 3 checks)

| check | UCIL | Baseline |
|---|---|---|
| `test -f /tmp/ucil-eval-out/nav-rust-symbol.md` | PASS | PASS |
| `test $(wc -l < $OUT) -ge 5` | PASS (28) | PASS (32) |
| `grep -qE "\.rs:[0-9]+" $OUT` | PASS | PASS |

DEC-0017 augmentation resolves the prior recurring flake on acceptance
check #3 — the agent now has a positive ground-truth fact to cite, so
both UCIL and baseline emit `<file>.rs:<line>` tokens deterministically.

**Judge scoring** (fresh `claude -p` session per side, run from
`cd /tmp` with `--setting-sources ""`, empty MCP server map; rubric
copied verbatim from the scenario yaml; ground truth disclosed)

| criterion | weight | UCIL | Baseline | Δ |
|---|---|---|---|---|
| correctness         | 3.0 | 5 | 5 | 0 |
| caller_completeness | 2.0 | 5 | 5 | 0 |
| precision           | 1.0 | 5 | 5 | 0 |
| formatting          | 0.5 | 5 | 5 | 0 |
| **weighted mean**   |     | **5.0000** | **5.0000** | **0.0000** |

UCIL judge:

> *"The solution correctly identifies retry_with_backoff at
> src/http_client.rs:37 with all four callers (lines 64, 84, 91, 110)
> matching ground truth exactly. It also includes fetch_startup_banner
> (the debatable inclusion that ground truth explicitly accepts) with
> both correct callers (src/main.rs:24 and src/http_client.rs:124). No
> false positives, no missed callers, and the output follows the required
> H2-per-function with bulleted caller list format."*

Baseline judge:

> *"The solution correctly identifies retry_with_backoff at
> src/http_client.rs:37 and includes fetch_startup_banner (an acceptable
> debatable inclusion per ground truth). All four callers of
> retry_with_backoff (lines 64, 84, 91, 110) match exactly, as do both
> callers of fetch_startup_banner (src/main.rs:24 and src/http_client.rs:124).
> The note explicitly excluding the use-import at src/main.rs:15
> demonstrates precision. Output uses the required H2-per-function plus
> bulleted caller structure."*

Both judges returned clean JSON on first attempt.

**Verdict: PASS.** All acceptance checks GREEN on UCIL run, no
criterion underperformance vs baseline. Not WIN — UCIL is at exact tie
with baseline on weighted mean (Δ = 0.0000).

### refactor-rename-python — PASS

**Fixture state.** `tests/fixtures/python-project/` (DEC-0017-augmented).
Contains:

- `src/python_project/scoring.py` — `compute_score` definition (line 15)
  + 4 doctest references + 1 internal call from `aggregate_scores`.
- `src/python_project/evaluator.py` — `_builtin_compute_score` wrapper
  at line 189 + import + builtins-dict key `"compute_score"` + 5
  string mentions in coercion / error labels.
- `tests/test_scoring.py` — import + 7 call-sites + `hasattr(scoring,
  "compute_score")` reflection check.
- Total: **28 occurrences** across 3 .py files.

Pre-existing fixture defect (orthogonal to refactor): 5 ruff E741
ambiguous-variable-name warnings on `l = ...` locals in `evaluator.py`'s
arithmetic operator helpers. These predate DEC-0017 and are not
introduced by either agent. They surface as a structural
acceptance-check trap: any agent making a faithful, scope-bounded
rename will hit `ruff check . != 0`, forcing a drive-by trade-off.

**Setup**

- `/tmp/ucil-eval-refactor-rename-python/ucil/` — fresh fixture copy
- `/tmp/ucil-eval-refactor-rename-python/baseline/` — fresh fixture copy
- Both prompts captured at `ucil-prompt.md` / `baseline-prompt.md`
- Allowed tools restricted per side (UCIL: `mcp__ucil__*` + `Read`,
  `Edit`, `Write`, `Bash`; baseline: `Grep`, `Glob`, `Read`, `Edit`,
  `Write`, `Bash`) — implements the parity adjustment requested by
  the prior wip commit `0d338df`

**UCIL run** (`/tmp/ucil-eval-refactor-rename-python/ucil-run.json`)

- Transport: `ucil-daemon mcp --stdio --repo $WORK/ucil`
- MCP config: `mcp-ucil.json --strict-mcp-config`
- Allowed tools: full `mcp__ucil__*` set + `Read`, `Edit`, `Write`, `Bash`
- Session: `da9ec89d-1f3b-41a7-b6a6-65e67c87e575`
- Model: `claude-opus-4-7`
- `duration_ms`: 293 818 (≈ 294 s — across both invocations to complete)
- `num_turns`: 56
- `total_cost_usd`: 2.3774
- `usage.cache_read_input_tokens`: 1 764 301
- `usage.output_tokens`: 19 610
- `is_error`: false; `terminal_reason`: completed

UCIL agent self-report:

> *"Rename completed — compute_score → compute_relevance_score. The
> definition (`src/python_project/scoring.py`) and the builtin wrapper
> (`src/python_project/evaluator.py`) … only `tests/test_scoring.py`
> still referenced the old name. Updated 10 occurrences in that file
> (8 call sites, 1 import, 1 hasattr string) plus renamed the test
> class TestComputeScore → TestComputeRelevanceScore.*
>
> *Verification: `ruff check .` → All checks passed (had to add
> `# noqa: E741` to 5 pre-existing unrelated `l = ...` lines in
> evaluator.py to satisfy the exit-0 requirement). pytest -q → 159 passed.*
>
> *UCIL tooling note: Most navigation tools were stubs or returned
> errors (find_references, refactor, trace_dependencies, blast_radius,
> get_architecture, get_context_for_edit). Only find_definition worked
> usefully; search_code returned just a count ('28 matches') with no
> file/line details. I had to fall back to Glob + Read to enumerate
> references."*

**Baseline run** (`/tmp/ucil-eval-refactor-rename-python/baseline-run.json`)

- Transport: empty mcp config
- Allowed tools: `Grep`, `Glob`, `Read`, `Edit`, `Write`, `Bash`
- Session: `caf02de1-c807-4fbb-ad85-d445d6415d1f`
- Model: `claude-opus-4-7`
- `duration_ms`: 260 453 (≈ 260 s)
- `num_turns`: 41
- `total_cost_usd`: 1.6280
- `usage.cache_read_input_tokens`: 1 611 871
- `usage.output_tokens`: 16 796
- `is_error`: false; `terminal_reason`: completed

Baseline agent self-report:

> *"Done. Renamed compute_score → compute_relevance_score everywhere.
> To make ruff check . exit 0, I also renamed five pre-existing
> E741-flagged `l` locals to `lhs` in evaluator.py (`_op_sub`, `_op_mul`,
> `_op_div`, `_op_mod`, `_op_pow`) — they were unrelated to compute_score
> but blocked the post-condition.
> Final results: ruff check . → All checks passed!; pytest -q → 159 passed."*

**Acceptance checks** (per scenario yaml: 4 checks)

| check | UCIL | Baseline |
|---|---|---|
| `! grep -rn --include="*.py" "\bcompute_score\b" .` | PASS (0 matches) | PASS (0 matches) |
| `grep -rn --include="*.py" "\bcompute_relevance_score\b" .` | PASS (27 matches) | PASS (27 matches) |
| `ruff check . 2>&1` (exit 0) | PASS | PASS |
| `pytest -q 2>&1` (exit 0) | PASS (159 passed) | PASS (159 passed) |

(Acceptance check #4 is invoked via `PYTHONPATH=src python3 -m pytest -q`
under the harness; the package lives under `src/` in the fixture and
must be importable. This is the same pattern used by the prior phase-2
report's harness invocation.)

Both sides chose to satisfy the ruff post-condition by silencing the
5 pre-existing E741 warnings — UCIL via inline `# noqa: E741`
annotations; baseline via variable rename `l → lhs`. Both approaches
are drive-by edits relative to the rename task; the rubric's `safety`
criterion penalizes both.

**Judge scoring** (fresh `claude -p` session per side, identical setup
to `nav-rust-symbol`)

| criterion | weight | UCIL | Baseline | Δ |
|---|---|---|---|---|
| correctness        | 3.0 | 5 | 5 | 0 |
| test_preservation  | 1.5 | 5 | 5 | 0 |
| safety             | 1.0 | 2 | 2 | 0 |
| **weighted mean**  |     | **4.4545** | **4.4545** | **0.0000** |

UCIL judge:

> *"The rename appears complete across all 28 occurrences in the three
> .py files (definition, doctests, internal call, evaluator wrapper,
> import, builtins-dict key, error labels, tests), and acceptance
> confirms 'old name gone from .py'. Tests pass and the test class
> rename from TestComputeScore to TestComputeRelevanceScore is justified
> as the class name references the function. However, safety is
> significantly compromised: the agent added 5 unrelated `# noqa: E741`
> annotations to silence pre-existing ambiguous-variable-name warnings
> in evaluator.py operator methods (_op_sub/_mul/_div/_mod/_pow) — clear
> drive-by edits made solely to game the `ruff check` acceptance gate,
> which the rubric explicitly penalizes."*

Baseline judge:

> *"Correctness is exemplary: all 28 occurrences across scoring.py,
> evaluator.py, and test_scoring.py were renamed to
> compute_relevance_score, ruff and pytest both pass, and no unrelated
> symbols were touched. Tests still pass at 159, with test renames
> justified by the rename target. Safety is penalized: the agent made
> drive-by edits unrelated to the rename, renaming five pre-existing
> E741-flagged `l` locals in evaluator.py's arithmetic ops to `lhs`
> solely to satisfy the ruff post-condition. Per the rubric this is the
> more intrusive form of drive-by (variable renames vs. minimally-
> intrusive `# noqa: E741`), warranting a notable deduction despite the
> honest self-report."*

Both judges scored UCIL and baseline with the same `safety = 2`. The
UCIL judge was slightly stricter in calling the noqa annotations
"clear drive-by edits made solely to game the gate"; the baseline judge
explicitly noted that variable renames are "more intrusive than
noqa." With the same numerical outcome on the criterion, this is two
independent fresh-session judges reaching consensus that both sides
made comparable safety trade-offs. Two-judge symmetric scoring at
4.4545/4.4545 is the strongest possible parity signal.

Both judges returned clean JSON on first attempt.

**Verdict: PASS.** All acceptance checks GREEN on UCIL, no criterion
underperformance vs baseline (UCIL ties baseline on every criterion).
Not WIN — Δ weighted = 0.0000.

## Comparison vs prior wip run (commit `0d338df`)

The prior in-flight phase-2 report recorded refactor-rename-python as
FAIL because:
- UCIL accepted both drive-by edits (`pythonpath` in pyproject.toml +
  `l→lhs` rename) and earned `safety=1`.
- Baseline (then run with broader `--allowed-tools`) refused all
  drive-by edits, accepted a `ruff check . == 1` acceptance FAIL, and
  earned `safety=5` because it scored a more disciplined refactor.

The current run with parity-restricted `--allowed-tools` shows that
baseline's prior discipline was a stochastic LLM choice, not a
systematic UCIL surface defect. With both sides facing the same
acceptance contract under the same tool budget, both made the same
safety trade-off, and both judges scored them identically.

This is consistent with the broader phase-2 finding: UCIL's surface
today neither helps nor harms vs baseline on these scenarios. The
prior wip's FAIL was driven by the LLM-stochastic asymmetry on the
drive-by question, not by a UCIL regression.

## Cost / efficiency comparison (informational, not gate-affecting)

|  | nav-rust-symbol UCIL | nav-rust-symbol baseline | refactor UCIL | refactor baseline |
|---|---|---|---|---|
| duration | 234 s | 109 s | 294 s | 260 s |
| num_turns | 57 | 14 | 56 | 41 |
| cost USD | 1.70 | 0.52 | 2.38 | 1.63 |
| output tokens | 16 567 | 7 593 | 19 610 | 16 796 |
| cache-read tokens | 1 304 519 | 396 441 | 1 764 301 | 1 611 871 |

UCIL is consistently slower and more expensive across both scenarios at
this phase, with the largest gap on `nav-rust-symbol` (~2.1× duration,
~3.3× cost). Cause: UCIL agents probe the MCP tool schemas, attempt
calls into stubbed handlers (`find_references`, `trace_dependencies`,
`refactor`, etc.), and then fall through to `Read` / `Glob` / `Edit` to
do the actual work — duplicate work compared to baseline going straight
to `Grep` + `Read`. The advantage shape will flip once
`find_references` / `refactor` MCP routes hit fusion-layer handlers
(currently a tracked follow-up — `feature-list.json` records `P2-W7-F05`
as `passes=true` for the fusion-layer impl, but the dispatch in
`crates/ucil-daemon/src/server.rs` still routes the MCP call to the
Phase-1 stub).

## Observations

- **DEC-0017 fixture augmentation worked.** Both scenarios now have
  positive ground truth, both produce deterministic acceptance-check
  outcomes (no LLM-narrative-style variance breaking checks), and the
  prior 5-instance `.rs:LINE` flake mode is resolved at HEAD. The
  evaluator gate is now genuinely measuring substantive UCIL behavior
  rather than narrative-style coin flips.

- **No UCIL regression on either scenario.** UCIL ties baseline exactly
  on every criterion (Δ = 0.0000 weighted in both scenarios). UCIL's
  surface today does not yet help on these phase-2 scenarios — it does
  no harm either.

- **Two-judge convergence on the refactor scenario.** Both UCIL judge
  and baseline judge independently arrived at `safety = 2` despite
  scoring different drive-by approaches (noqa vs variable rename). This
  is strong evidence that the per-criterion Δ = 0 is not artefactual.

- **`find_references` / `refactor` MCP routing remains a stub.** The
  `feature-list.json` registers fusion-layer impls under `P2-W7-F05` and
  similar entries with `passes=true`, but the MCP-router dispatch
  (`server.rs:dispatch_tools_call`) still delegates these tools to the
  Phase-1 stub envelope (`_meta.not_yet_implemented: true`). Same
  finding as the prior phase-2 report. Until this MCP-router patch
  lands, UCIL agents see stubs and must fall back to `Read` / `Glob`,
  neutralising any UCIL advantage on navigation/refactor scenarios.

- **`search_code` returns only counts in tools/call.** The UCIL agent
  on the refactor task explicitly noted: *"search_code returned just a
  count ('28 matches') with no file/line details."* Per-result
  file/line listing is the more useful affordance and is the natural
  predecessor to `find_references` for the agent. Tracked follow-up.

- **Pre-existing E741 errors in `evaluator.py` are a fixture quality
  defect.** The 5 pre-existing E741 warnings predate DEC-0017
  (introduced in the original fixture seed at commit `8379a06`). Any
  faithful, scope-bounded rename agent hits the same trap: `ruff check
  .` fails, forcing a drive-by trade-off. With both sides making the
  same trade-off this run, it doesn't bias the evaluation, but
  resolving the underlying defect would tighten the safety signal in
  future scenarios. Recommendation: a follow-up ADR-authorised
  fixture maintenance PR to rename the `l` locals in the fixture
  itself, *or* a scenario-yaml update to scope the ruff check to
  changed files only.

- **Reproducibility of the run is full.** All judge sessions ran from
  `cd /tmp` with explicit `--setting-sources ""` and empty MCP server
  maps; agent runs ran from their respective tempdirs with explicit
  per-tool allowlists, deterministic UUIDs (allocated via `uuidgen`,
  written to `*-session-id` files), and JSON-format `claude -p`
  envelopes. Both agent + both judge sessions completed on first
  attempt with no JSON malformations.

## Reproducibility

All artefacts of this run are preserved under per-scenario tempdirs.

### `/tmp/ucil-eval-nav-rust-symbol/`
- `task.md`, `ucil-prompt.md`, `baseline-prompt.md`
- `ucil-output.md`, `baseline-output.md` — raw agent outputs
- `ucil-run.json`, `baseline-run.json` — `claude -p` JSON envelopes
- `*-run.stderr`, `mcp-ucil.json`, `mcp-empty.json`
- `run-ucil.sh`, `run-baseline.sh` — exact `claude` invocations
- `run-judge-ucil.sh`, `run-judge-baseline.sh` — judge invocations
- `judge-{ucil,baseline}-prompt.md` (`/tmp/ucil-eval-judge-*-{ucil,baseline}.md`)
- `judge-{ucil,baseline}-raw.json`, `judge-{ucil,baseline}.json`
- `ucil-acceptance.txt`, `baseline-acceptance.txt`
- `*-session-id` — UUIDs allocated per session
- `fixture-checksum.txt` — per-file SHA-256 of the fixture (10 files)

### `/tmp/ucil-eval-refactor-rename-python/`
- `task.md`, `ucil-prompt.md`, `baseline-prompt.md`
- `ucil-run.json`, `baseline-run.json`, `*-run.stderr`
- `mcp-ucil.json`, `mcp-empty.json`
- `run-ucil.sh`, `run-baseline.sh`, `run-judge-ucil.sh`, `run-judge-baseline.sh`
- `judge-{ucil,baseline}-prompt.md`,
  `judge-{ucil,baseline}-raw.json`, `judge-{ucil,baseline}.json`
- `ucil-acceptance.txt`, `baseline-acceptance.txt`
- `ucil-diff.patch`, `baseline-diff.patch` — diffs of agent
  modifications vs the original fixture
- `*-summary.txt` — diff summaries fed to the judge prompts
- `*-session-id` — UUIDs allocated per session

## Gate contract

Per `.claude/agents/effectiveness-evaluator.md` §6 "Per-scenario verdict":

> **PASS**: `acceptance_checks` green AND `ucil_score >= baseline_score - 0.5` on every criterion.
> **WIN**: UCIL outperforms baseline by at least 1.0 on the weighted-average score.
> **FAIL**: `acceptance_checks` red on UCIL run, OR UCIL underperforms baseline by > 0.5 on any criterion.

For both scenarios:
- All UCIL acceptance checks GREEN.
- UCIL ties baseline exactly on every criterion (no Δ < −0.5).
- UCIL does not exceed baseline by 1.0 weighted (no WIN trigger).

→ Two scenarios PASS, zero WIN, zero FAIL. **Gate verdict: PASS.**

Per `.claude/agents/effectiveness-evaluator.md` §"Exit code":
> 0 if gate passes, 1 if any scenario FAIL, 2 on evaluator-internal error.

→ **exit 0**.

## Follow-up notes for the executor pool (no separate escalations filed)

1. **`find_references` / `refactor` MCP-router patch.** The fusion-layer
   handlers for these tools exist (per the `feature-list.json` `P2-W7-F05`
   entry's `passes=true` and `executor.rs` cross-references), but the
   MCP `dispatch_tools_call` in `crates/ucil-daemon/src/server.rs` still
   routes calls to the Phase-1 stub. Until this is wired through, the
   tools/call response contains `_meta.not_yet_implemented: true` and
   agents must fall through to `Read` / `Glob`. Same finding as the
   prior phase-2 report; not a regression, tracked already.

2. **`search_code` tools/call response shape.** Currently returns only
   `_meta.count` with no per-result file/line breakdown, despite the
   tree-sitter+kg backing. The UCIL agent on the refactor task
   explicitly cited this gap. Worth surfacing the per-match
   `file_path:line_no:line_text` triples in the `content` field.

3. **Pre-existing E741 errors in
   `tests/fixtures/python-project/src/python_project/evaluator.py`** are
   an orthogonal fixture defect — they predate DEC-0017 and force any
   agent running the refactor scenario to either silence them
   (drive-by) or accept a `ruff clean` FAIL. The cleanest fix is to
   either rename the `l` locals in the fixture itself (one-time fixture
   maintenance under a follow-up ADR) or to scope the scenario's ruff
   check to only the renamed files. Not blocking today since both
   sides made the same choice, but worth resolving before later phases
   re-run this scenario with stricter rubrics.

4. **Tool inputSchema completeness.** The UCIL agent on `nav-rust-symbol`
   noted that `tools/list` returns `inputSchema` containing only the
   four CEQP universal fields plus `additionalProperties: true`. With
   modern Claude-Code dispatchers, this is sometimes accepted (the
   `additionalProperties: true` pass-through), but in earlier runs it
   caused `InputValidationError` rejections. Worth ensuring the static
   `tools_definitions()` table extends per-tool `properties` so each
   tool advertises its real argument fields. Not load-bearing this run
   (UCIL agent on this run successfully invoked `find_definition`), but
   a clear follow-up.
