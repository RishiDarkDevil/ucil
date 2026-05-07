# Effectiveness Report — Phase 2

Run at: 2026-05-07T17:23Z
Commit: `1414586476678ca46564548c3ba35807c49e4907`
Branch: `main`
Evaluator: `effectiveness-evaluator` (fresh session, `claude-opus-4-7`)

## Summary

| metric | value |
|---|---|
| Scenarios discovered for phase 2 | 2 |
| Scenarios run | 2 |
| Scenarios skipped (`tool_not_ready`) | 0 |
| Scenarios skipped (`scenario_defect`) | 0 |
| Scenarios PASS | 1 |
| Scenarios WIN | 0 |
| Scenarios FAIL | 1 |

**Gate verdict: FAIL.** This is the first phase-2 effectiveness run after
ADR DEC-0017 augmented the rust-project + python-project fixtures with
the symbols both scenarios assert. Both scenarios are now substantively
runnable end-to-end (no scenario_defect skips). The two outcomes:

- **`nav-rust-symbol`** → **PASS**. UCIL and baseline both correctly
  identified `rust_project::http_client::retry_with_backoff` (defined at
  `src/http_client.rs:37`) and `rust_project::http_client::fetch_startup_banner`
  (defined at `src/http_client.rs:62`), enumerated all real callers, and
  emitted the H2-per-function + bulleted-caller structure. UCIL judge
  scored **5/5/5/5** across `correctness/caller_completeness/precision/formatting`;
  baseline scored **5/4/5/5** (UCIL caught the rustdoc-doctest caller at
  line 26 that baseline omitted). Weighted-mean Δ = **+0.3077** (UCIL
  5.0000 vs baseline 4.6923), so UCIL non-trivially outperforms baseline
  on a substantive criterion (caller_completeness +1.0); not far enough
  for WIN (Δ < 1.0) but a clean PASS.
- **`refactor-rename-python`** → **FAIL**. UCIL's safety score
  (1/5) underperforms baseline's safety (5/5) by 4.0 — far beyond the
  ±0.5 noise window. Both sides rendered the rename correctly
  (`compute_score` → `compute_relevance_score`) and both pytest suites
  pass (159/159). The differentiator is that UCIL made two *drive-by*
  edits unrelated to the rename — adding `pythonpath = ["src"]` to
  `pyproject.toml`, and renaming five `l`/`r` ambiguous one-letter
  variables to `lhs`/`rhs` in operator handlers in `evaluator.py` to
  silence the pre-existing `ruff E741` errors. UCIL satisfied all four
  acceptance checks (4/4 PASS); baseline produced a cleaner refactor
  (3 files modified, no drive-by edits) but failed acceptance check #3
  (`ruff clean`) because the pre-existing E741 errors at
  `evaluator.py:1099,1133,1156,1178,1203` remain. The rubric `safety`
  criterion (weight 1.0) explicitly penalises drive-by edits — UCIL
  earned a 1 there, baseline earned a 5.

A new escalation
(`20260507T1723Z-effectiveness-refactor-rename-python-fixture-pre-existing-ruff-errors.md`)
documents the structural tension: the scenario task demands
`ruff check . == 0` but the original fixture (`8379a06`) has 5
pre-existing E741 errors orthogonal to the rename. No agent can satisfy
both `ruff clean` AND `no drive-by edits` without modifying upstream
fixtures or relaxing the acceptance check.

## Tool-availability probe

`tools/list` against `ucil-daemon mcp --stdio --repo /tmp/ucil-mcp-probe/repo`
reported all 22 §3.2 tools registered (probe succeeded with exit 0). The
required-tools sets for both phase-2-eligible scenarios are present:

| scenario | requires_tools | listed? | tools/call returns real data? |
|---|---|---|---|
| `nav-rust-symbol` | `find_definition` | yes | yes — `_meta.callers`, `_meta.found=true`, real `file_path`/`start_line`/`qualified_name` |
| `nav-rust-symbol` | `find_references` | yes | **no** — handler returns `_meta.not_yet_implemented: true` |
| `refactor-rename-python` | `find_references` | yes | **no** — handler returns `_meta.not_yet_implemented: true` |
| `refactor-rename-python` | `refactor` | yes | **no** — handler returns `_meta.not_yet_implemented: true` |

Per `.claude/agents/effectiveness-evaluator.md` §"Tool-availability checks"
("operational" = registered + responsive), both scenarios are eligible.
UCIL agents fall back to Edit/Read/Grep when the stub envelopes return
`not_yet_implemented`; this matches the precedent set by the prior
phase-1 and phase-2 reports. The same tool-routing follow-up identified
in commit `76045c6`'s report still applies: routing
`find_references` / `refactor` from `dispatch_tools_call` to the
fusion-layer impls remains pending.

## Scenarios

| scenario | UCIL acceptance | UCIL score (weighted) | Baseline score (weighted) | Δ weighted | verdict |
|---|---|---|---|---|---|
| `nav-rust-symbol` | 3/3 PASS | **5.0000** (5/5/5/5) | 4.6923 (5/4/5/5) | **+0.3077** | **PASS** |
| `refactor-rename-python` | 4/4 PASS | 4.2727 (5/5/1) | **5.0000** (5/5/5) | **−0.7273** | **FAIL** |

## Per-scenario detail

### nav-rust-symbol — PASS

**Fixture state.** `tests/fixtures/rust-project/src/http_client.rs` (added
in commit `1c42c77` per ADR DEC-0017) defines a real
`retry_with_backoff` combinator that doubles `delay` after each failed
attempt (`delay = delay.checked_mul(2)…` at `src/http_client.rs:52`),
plus a thin HTTP-style wrapper `fetch_startup_banner` that calls it. The
fixture was confirmed byte-for-byte at the run's HEAD by SHA-256 of all
9 source files at `/tmp/ucil-eval-nav-rust-symbol/fixture-checksum.txt`.

**Setup**
- `/tmp/ucil-eval-nav-rust-symbol/ucil/` — fresh fixture copy for UCIL run
- `/tmp/ucil-eval-nav-rust-symbol/baseline/` — fresh fixture copy for baseline run
- Output sink: `/tmp/ucil-eval-out/nav-rust-symbol.md` (rotated between runs)
- Identical task prompt for both sides modulo "Fixture root: …"; both
  prompts captured verbatim at `ucil-prompt.md` / `baseline-prompt.md`

**UCIL run**
- Transport: `ucil-daemon mcp --stdio --repo /tmp/ucil-eval-nav-rust-symbol/ucil`
- MCP config: `--mcp-config $ROOT/mcp-ucil.json --strict-mcp-config`
- Settings: `--setting-sources ""` (no project / user / local settings)
- Session: `31d501d1-8eed-43bf-a17b-25b09b2d1a35`
- Model: `claude-opus-4-7`
- `duration_ms`: 164 336 (≈ 164 s)
- `num_turns`: 26
- `total_cost_usd`: 0.7843
- `usage.input_tokens`: 33; `cache_read_input_tokens`: 532 750;
  `cache_creation_input_tokens`: 35 238; `output_tokens`: 11 874
- `is_error`: false; `stop_reason`: end_turn
- Output: 50 lines, written to `/tmp/ucil-eval-out/nav-rust-symbol.md`,
  preserved at `/tmp/ucil-eval-nav-rust-symbol/ucil-output.md`

**UCIL agent self-report (excerpt).** The agent attempted UCIL MCP tools
first per the prompt, fell back to `Glob`+`Grep`+`Read` when the
deferred-tool dispatcher rejected per-tool argument schemas (still the
known `inputSchema.properties` defect from commit `76045c6`'s report),
and produced the canonical answer with all real callers cited. The
agent included the rustdoc doctest invocation at `src/http_client.rs:26`
as a caller — which the judge correctly credited.

**Baseline run**
- Transport: no MCP servers (`mcp-empty.json` with empty `mcpServers`)
- MCP config: `--mcp-config $ROOT/mcp-empty.json --strict-mcp-config`
- Settings: `--setting-sources ""`
- Session: `67566a42-68cc-46a3-894f-7eb7d72e3b81`
- Model: `claude-opus-4-7`
- `duration_ms`: 48 331 (≈ 48 s)
- `num_turns`: 12
- `total_cost_usd`: 0.2921
- `usage.input_tokens`: 13; `cache_read_input_tokens`: 208 070;
  `output_tokens`: 3 573
- `is_error`: false; `stop_reason`: end_turn
- Output: 32 lines, written to `/tmp/ucil-eval-out/nav-rust-symbol.md`,
  preserved at `/tmp/ucil-eval-nav-rust-symbol/baseline-output.md`

Baseline produced an essentially-equivalent answer but did **not**
include the rustdoc-doctest caller at `src/http_client.rs:26`, which the
judge credited UCIL for; that single caller is the source of UCIL's
+1 caller_completeness lead.

**Acceptance checks**

| check | UCIL | Baseline |
|---|---|---|
| `test -f /tmp/ucil-eval-out/nav-rust-symbol.md` | PASS | PASS |
| `test $(wc -l < /tmp/ucil-eval-out/nav-rust-symbol.md) -ge 5` | PASS (50 lines) | PASS (32 lines) |
| `grep -qE "\\.rs:[0-9]+" /tmp/ucil-eval-out/nav-rust-symbol.md` | **PASS** | **PASS** |

The `.rs:LINE` flake from prior phase-2 runs (escalation
`20260507T0357Z-effectiveness-nav-rust-symbol-rs-line-flake.md`) is
**resolved** by DEC-0017's positive-ground-truth fixture: both sides
naturally cite `src/http_client.rs:<line>` tokens because the answer is
non-empty.

**Judge scoring** (fresh `claude -p` session per side, run from
`cd /tmp` so the repo's project hooks/settings cannot interfere;
`--setting-sources ""`, `--strict-mcp-config` with empty server map; the
ground truth is disclosed to the judge so it can score correctness
against the truth, not against the agent's own claims)

| criterion | weight | UCIL | Baseline | Δ |
|---|---|---|---|---|
| correctness         | 3.0 | 5 | 5 | 0 |
| caller_completeness | 2.0 | **5** | **4** | **+1** |
| precision           | 1.0 | 5 | 5 | 0 |
| formatting          | 0.5 | 5 | 5 | 0 |
| **weighted mean**   |     | **5.0000** | **4.6923** | **+0.3077** |

UCIL judge (`/tmp/ucil-eval-nav-rust-symbol/judge-ucil-session-id`):

> *"The solution correctly identifies retry_with_backoff at
> src/http_client.rs:37 as the primary exponential-backoff retry
> primitive and includes fetch_startup_banner at src/http_client.rs:62
> with explicit justification for treating the delegating wrapper as a
> qualifying function (which the rubric explicitly allows). All five
> real callers of retry_with_backoff are listed with correct file:line
> references, including the rustdoc doctest at line 26, and both callers
> of fetch_startup_banner (src/main.rs:24, src/http_client.rs:124) are
> present and accurate. There are no fabricated callers, no stdlib
> references, and no functions misclassified as exponential-backoff. The
> structure follows H2-per-function with bulleted caller lists; the
> small intro paragraph and trailing acceptance-check note are minor
> additions but do not violate the required format."*

Baseline judge (`/tmp/ucil-eval-nav-rust-symbol/judge-baseline-session-id`):

> *"Both qualifying functions are correctly identified with accurate
> definition lines (37 and 62), and the doubling mechanism is correctly
> cited at line 52. Including fetch_startup_banner is one of the two
> acceptable judgment calls per the rubric. All four fixture-tree
> callers of retry_with_backoff (lines 64, 84, 91, 110) and both
> callers of fetch_startup_banner (main.rs:24, http_client.rs:124) are
> listed with correct file:line references and no fabrications. The one
> real omission is the doctest invocation at src/http_client.rs:26,
> which the ground truth explicitly enumerates as a caller, costing a
> point on caller_completeness. Precision is clean (no spurious entries)
> and the H2-per-function plus bulleted-caller formatting matches the
> spec exactly."*

Both judges returned clean JSON on first attempt.

**Verdict: PASS** (UCIL acceptance all green; UCIL ≥ baseline − 0.5 on
every criterion: 5≥4.5, 5≥3.5, 5≥4.5, 5≥4.5; not WIN since Δ weighted
+0.3077 < 1.0).

### refactor-rename-python — FAIL

**Fixture state.** `tests/fixtures/python-project/src/python_project/scoring.py`
(added in commit `14bbace` per ADR DEC-0017) defines `compute_score`
plus a wrapper `aggregate_scores` that calls it; `evaluator.py` defines
a builtin `_builtin_compute_score` registered under the key
`"compute_score"` in the builtins dict; `tests/test_scoring.py` covers
both functions with 13 cases including a `hasattr(scoring, "compute_score")`
**string** reference. The original fixture (commit `8379a06`) has 5
pre-existing `ruff E741` errors at `evaluator.py:1099,1133,1156,1178,1203`
(ambiguous variable name `l` in the `-`/`/`/`%`/`^` operator handlers).
Fixture state confirmed at HEAD via SHA-256 inventory at
`/tmp/ucil-eval-refactor-rename-python/fixture-checksum.txt` (12 files).

**Setup**
- `/tmp/ucil-eval-refactor-rename-python/ucil/` — fresh fixture copy for UCIL
- `/tmp/ucil-eval-refactor-rename-python/baseline/` — fresh fixture copy for baseline
- Each agent runs the rename in its own copy; the harness re-runs
  `grep`/`ruff`/`pytest` against each copy directly (no shared output sink)
- Identical prompts modulo "Fixture root: …"; both at `ucil-prompt.md` /
  `baseline-prompt.md`

**UCIL run**
- Transport: `ucil-daemon mcp --stdio --repo /tmp/ucil-eval-refactor-rename-python/ucil`
- Session: `f253433d-a197-4936-8814-4f6c34ddd189`
- Model: `claude-opus-4-7`
- `duration_ms`: 233 342 (≈ 233 s); `num_turns`: 41 (hit `max_turns=40`,
  `terminal_reason=max_turns`); `total_cost_usd`: 1.3520
- `usage.input_tokens`: 45; `cache_read_input_tokens`: 1 514 388;
  `cache_creation_input_tokens`: 35 600; `output_tokens`: 14 857
- `is_error`: true (max_turns); the rename + post-checks completed
  on-disk before the cap fired

**UCIL files modified (4 files; cache dirs excluded):**
1. `pyproject.toml` — added `pythonpath = ["src"]` under
   `[tool.pytest.ini_options]` (drive-by; not required by the rename)
2. `src/python_project/scoring.py` — `compute_score` →
   `compute_relevance_score` (definition + docstring examples + caller)
3. `src/python_project/evaluator.py` — `compute_score` →
   `compute_relevance_score` (`_builtin_compute_score` →
   `_builtin_compute_relevance_score`, all docstring + 4 error strings +
   import + builtins-table key) **AND** drive-by-renamed five `l`/`r`
   ambiguous one-letter variables to `lhs`/`rhs` in operator handlers
   (`-`, `/`, `%`, `^`) at lines 1099/1133/1156/1178/1203 to silence the
   pre-existing E741s
4. `tests/test_scoring.py` — all `compute_score` occurrences renamed,
   including the `hasattr(scoring, "compute_score")` string

**Baseline run**
- Transport: no MCP servers
- Session: `9d6d9363-d0ff-476b-b203-88bf897ba821`
- Model: `claude-opus-4-7`
- `duration_ms`: 251 664 (≈ 252 s); `num_turns`: 38;
  `terminal_reason=completed`; `total_cost_usd`: 1.7278
- `usage.input_tokens`: 48; `cache_read_input_tokens`: 1 749 270;
  `cache_creation_input_tokens`: 73 333; `output_tokens`: 15 759
- `is_error`: false

**Baseline files modified (3 files):**
1. `src/python_project/scoring.py` — rename only
2. `src/python_project/evaluator.py` — rename only (no E741 fix)
3. `tests/test_scoring.py` — rename only (incl. `hasattr` string)

The baseline agent's final report (verbatim excerpt):

> *"The rename is complete and consistent; the two non-zero exit codes
> come from environment/baseline issues outside the rename's scope.
> ruff check . → 1. Five E741 'Ambiguous variable name: \`l\`' errors
> in src/python_project/evaluator.py at lines 1099, 1133, 1156, 1178,
> 1203 — these are pre-existing in operator methods I did not touch
> (my edits were only in lines 189–210 and 675)."*

i.e. baseline transparently reported the ruff failure as orthogonal to
its scope, refused to do a drive-by fix, and still satisfied the two
substantive checks (old name gone, new name present) plus the pytest
suite.

**Acceptance checks**

| check | UCIL | Baseline |
|---|---|---|
| `! grep -rn --include="*.py" "\\bcompute_score\\b" .` (old gone) | PASS | PASS |
| `grep -rn --include="*.py" "\\bcompute_relevance_score\\b" .` (new present) | PASS | PASS |
| `ruff check .` (rc=0) | PASS (rc=0; only because UCIL drive-by-fixed E741s) | **FAIL (rc=1; 5 pre-existing E741 errors remain)** |
| `pytest -q` (rc=0) | PASS (159 passed) | PASS (159 passed) |

(Note: the harness invokes `PYTHONPATH=src python3 -m pytest -q` for
acceptance check #4; both sides return rc=0/159-passed under that
invocation. The scenario yaml's literal `pytest -q 2>&1` would error
on baseline because the package isn't installed in the eval env, which
the baseline agent flagged in its final report. The harness's
PYTHONPATH-augmented invocation is the gate-side standard already used
in commit `76045c6`'s report and `tests/scenarios/README.md` precedent.)

**Judge scoring** (fresh `claude -p` per side, `cd /tmp`,
`--setting-sources ""`, `--strict-mcp-config` empty)

| criterion | weight | UCIL | Baseline | Δ |
|---|---|---|---|---|
| correctness       | 3.0 | 5 | 5 | 0 |
| test_preservation | 1.5 | 5 | 5 | 0 |
| safety            | 1.0 | **1** | **5** | **−4** |
| **weighted mean** |     | **4.2727** | **5.0000** | **−0.7273** |

UCIL judge:

> *"Correctness is exemplary: every reference to compute_score across
> scoring.py, evaluator.py, and tests/test_scoring.py was renamed
> including the docstring/error-string mentions, the builtins-table
> key, and the hasattr string reference; mechanical checks confirm the
> old name is gone and the new name is present. Test preservation is
> full: 159 tests pass and the only test edits are the required
> mechanical rename (including the string-name reference), which is
> justified by the task. Safety is poor: the agent made two drive-by
> changes unrelated to the rename — adding pythonpath=['src'] to
> pyproject.toml (no scenario justification at all, this is purely
> environmental convenience) and renaming five l/r operator-handler
> variables to lhs/rhs to silence the pre-existing E741 errors. Even
> granting the genuine ruff-vs-no-drive-by tension as a defensible call
> for the E741 fixes, the pyproject.toml edit has no such excuse, and
> together they constitute clear drive-by scope creep that the rubric
> explicitly penalises."*

Baseline judge:

> *"The rename is complete and accurate across all four expected
> locations: scoring.py (definition, docstring examples, and
> aggregate_scores caller), evaluator.py (_builtin_compute_score
> wrapper definition, docstring, import, error message strings, body
> call, and the builtins-table key string), and tests/test_scoring.py
> (import, call sites, and the hasattr string reference). The agent's
> grep verification confirms no stray compute_score references remain
> and the new name is present. Tests pass cleanly (159 passed) when
> invoked the way the repo's own run-acceptance.sh invokes them
> (PYTHONPATH=src), confirming the rename preserves behavior without
> modifying tests. The agent correctly refused drive-by edits to the
> pre-existing E741 errors in unrelated operator methods (lines
> 1099-1203) that the rename does not touch, which is the behavior the
> safety criterion rewards; it also transparently reported the ruff
> non-zero exit and traced it to baseline issues with precise line
> numbers, distinguishing them from the rename's scope. The ruff
> failure is entirely attributable to pre-existing fixture errors
> orthogonal to the task."*

Both judges returned clean JSON on first attempt.

**Verdict: FAIL** (UCIL underperforms baseline by 4.0 on `safety`,
exceeding the ±0.5 noise window). UCIL acceptance is all green, but the
rubric's strict-letter contract triggers FAIL on the per-criterion drop.

## Substantive analysis — root cause of refactor FAIL

UCIL's safety drop has two distinct components:

1. **`pyproject.toml` drive-by** (added `pythonpath = ["src"]`). This
   has no defensible justification under the rename task. UCIL's
   environment convenience (so `pytest -q` from the fixture root works
   without `PYTHONPATH=src`) is unrelated to renaming `compute_score`.
   This is a clean rubric violation.
2. **`l`/`r` → `lhs`/`rhs` rename in five operator handlers**. This
   silences the **pre-existing** ruff E741 errors at
   `evaluator.py:1099,1133,1156,1178,1203` (introduced in commit
   `8379a06`, never touched by DEC-0017). Without this fix, acceptance
   check #3 (`ruff check . == 0`) FAILS because the original fixture is
   already non-clean. There is genuine tension between the scenario's
   `ruff clean` requirement and the rubric's `safety` criterion: any
   agent doing a faithful, scope-bounded rename will hit the
   pre-existing E741s and the acceptance check will go red.

Component 1 alone is enough to drop UCIL's safety score; component 2
compounds it. The tension in component 2 is a **fixture quality
defect** (`fixture-pre-existing-ruff-errors`). It is filed as a new
escalation (`20260507T1723Z-effectiveness-refactor-rename-python-fixture-pre-existing-ruff-errors.md`)
for planner triage. Possible remediations (planner / ADR-gated):

- **A. Fix the fixture's E741 errors upstream**, in a follow-up to
  DEC-0017, so `ruff check .` is clean *before* any rename. This is
  the cleanest fix; it removes the safety-vs-acceptance tension
  entirely. Both UCIL and baseline would then be expected to PASS this
  scenario deterministically.
- **B. Loosen acceptance check #3** to skip the E741 rule in the
  scenario yaml (e.g. `ruff check . --select E,W,F --ignore E741`).
  This degrades the effectiveness signal slightly (one rule's worth of
  lint coverage gone) but preserves both the rename and the safety
  contract.
- **C. Update the rubric** to explicitly carve out "drive-by edits
  required to satisfy a stated post-condition" as not-penalised. This
  is more permissive and might erode the safety signal in future
  scenarios; option A is preferred.

This run records the FAIL faithfully per the strict contract; the gate
should remain red until one of A/B/C lands.

## Per-criterion deltas (UCIL − Baseline)

| scenario | criterion | weight | UCIL − Baseline |
|---|---|---|---|
| nav-rust-symbol | correctness | 3.0 | 0 |
| nav-rust-symbol | caller_completeness | 2.0 | **+1** |
| nav-rust-symbol | precision | 1.0 | 0 |
| nav-rust-symbol | formatting | 0.5 | 0 |
| refactor-rename-python | correctness | 3.0 | 0 |
| refactor-rename-python | test_preservation | 1.5 | 0 |
| refactor-rename-python | safety | 1.0 | **−4** |

## Gate contract (why this run is FAIL)

Per `.claude/agents/effectiveness-evaluator.md` §6 "Per-scenario verdict":

> **PASS**: `acceptance_checks` green AND `ucil_score >= baseline_score - 0.5` on every criterion.
> **WIN**: UCIL outperforms baseline by at least 1.0 on the weighted-average score.
> **FAIL**: `acceptance_checks` red on UCIL run, OR UCIL underperforms baseline by > 0.5 on any criterion.

For `refactor-rename-python` UCIL underperforms baseline by 4 points on
`safety` (>0.5 tolerance) → **FAIL** under the strict-letter rule.
`nav-rust-symbol` is a clean PASS (UCIL ties or wins on every
criterion). One scenario FAIL → gate FAIL.

Per `.claude/agents/effectiveness-evaluator.md` §"Exit code":

> 0 if gate passes, 1 if any scenario FAIL, 2 on evaluator-internal error.

Exit code: **1**.

## Reproducibility

All artefacts of this run are preserved under the per-scenario tempdirs:

### `/tmp/ucil-eval-nav-rust-symbol/`
- `task.md` — scenario task prompt (verbatim from yaml)
- `ucil-prompt.md`, `baseline-prompt.md` — full agent task prompts (with fixture root)
- `ucil-output.md`, `baseline-output.md` — raw agent outputs (copies of `/tmp/ucil-eval-out/nav-rust-symbol.md`)
- `ucil-run.json`, `baseline-run.json` — `claude -p` JSON envelopes
- `ucil-run.stderr`, `baseline-run.stderr` — child-process stderr
- `mcp-ucil.json`, `mcp-empty.json` — MCP configs
- `run-ucil.sh`, `run-baseline.sh` — exact `claude` invocations
- `run-judge.sh ucil|baseline` — judge invocations
- `judge-ucil-prompt.md` (`/tmp/ucil-eval-judge-nav-rust-symbol-ucil.md`),
  `judge-baseline-prompt.md` (`/tmp/ucil-eval-judge-nav-rust-symbol-baseline.md`)
- `judge-ucil-raw.json`, `judge-baseline-raw.json` — judge envelopes
- `judge-ucil.json`, `judge-baseline.json` — extracted scoring JSON
- `ucil-acceptance.txt`, `baseline-acceptance.txt` — acceptance check results
- `ucil-session-id`, `baseline-session-id`, `judge-{ucil,baseline}-session-id`
- `fixture-checksum.txt` — per-file SHA-256 of the fixture (9 files)

### `/tmp/ucil-eval-refactor-rename-python/`
- `task.md`, `ucil-prompt.md`, `baseline-prompt.md`
- `ucil-output.md` — synthesized summary of UCIL agent's actions (since
  max_turns truncated the agent's own self-report)
- `baseline-output.md` — baseline agent's verbatim final message
- `ucil-run.json`, `baseline-run.json`, `*-run.stderr`
- `mcp-ucil.json`, `mcp-empty.json`
- `run-ucil.sh`, `run-baseline.sh`, `run-acceptance.sh`, `run-judge.sh`
- `judge-{ucil,baseline}-prompt.md`,
  `judge-{ucil,baseline}-raw.json`, `judge-{ucil,baseline}.json`
- `ucil-acceptance.txt`, `baseline-acceptance.txt`,
  `{ucil,baseline}-acc{1..4}-*.txt` — per-acceptance-check artefacts
- `{ucil,baseline}-session-id`, `judge-{ucil,baseline}-session-id`
- `fixture-checksum.txt` — per-file SHA-256 of the fixture (12 files)

## Advisory — fixture quality issue (Phase-2 escalation filed)

The pre-existing E741 errors in `tests/fixtures/python-project/src/python_project/evaluator.py`
predate DEC-0017 (introduced by commit `8379a06`, the original fixture
seed). They constitute a fixture quality defect that surfaces as a
structural FAIL trigger on the `refactor-rename-python` scenario: any
faithful, scope-bounded rename agent hits the same trap. A new
escalation
(`ucil-build/escalations/20260507T1723Z-effectiveness-refactor-rename-python-fixture-pre-existing-ruff-errors.md`)
documents this for planner triage.

The `nav-rust-symbol` scenario is now flake-free post-DEC-0017; the
prior `.rs:LINE` flake (escalation
`20260507T0357Z-effectiveness-nav-rust-symbol-rs-line-flake.md`) is
**resolved** by the augmented fixture (positive ground truth). That
escalation should be marked `resolved: true` in a follow-up.

## Follow-up notes for the executor pool (no separate WO filed)

1. **`find_references` / `refactor` MCP routing patch.** Same
   dispatcher-routing follow-up identified in the prior phase-2
   effectiveness report. Until those handlers route to their fusion-layer
   impls, UCIL agents fall back to `Edit/Read/Grep` for symbol-rename
   workflows; this matters less for the gate verdict here than the
   fixture quality issue.
2. **Tool inputSchema completeness.** Same finding as commit `76045c6`'s
   report — `tools_definitions()` in
   `crates/ucil-daemon/src/server.rs:280-369` still advertises only the
   universal CEQP fields. Until per-tool argument fields are declared,
   Claude Code's deferred-tool dispatcher rejects MCP tool calls and
   the agent silently falls through to built-in tools, neutralising
   UCIL's edge. This is the most load-bearing finding from this run.
