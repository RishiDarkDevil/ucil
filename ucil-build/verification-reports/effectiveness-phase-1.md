# Effectiveness Report — Phase 1

Run at: 2026-05-07T03:21:00Z
Commit: f4adc41497d141bfcfd7adb6e539d13e5d9c75a8
Branch: feat/WO-0065-vector-query-p95-bench
Evaluator: effectiveness-evaluator (fresh session, `claude-opus-4-7`)

## Summary

| metric | value |
|---|---|
| Scenarios discovered for phase 1 | 1 |
| Scenarios run | 1 |
| Scenarios skipped (tool_not_ready) | 0 |
| Scenarios skipped (defect) | 0 |
| Scenarios PASS | 0 |
| Scenarios WIN | 0 |
| Scenarios FAIL | 1 |

**Gate verdict: FAIL (per strict contract reading)** — the single
phase-1-eligible scenario (`nav-rust-symbol`) was executed end-to-end. Both
UCIL and baseline produced substantively-correct answers ("no qualifying
functions exist", matching ground truth). Both LLM-judges scored both sides
identically at 5/5 correctness, 5/5 caller_completeness, 5/5 precision, 4/5
formatting → weighted mean 4.9231 each, **Δ weighted = 0.0**.

**Why FAIL despite a substantive tie:** acceptance_check #3 (`cites at least
one file:line`) was RED on **both sides** — neither output naturally
contained a `.rs:LINE` token because the truthful answer is "no qualifying
functions to enumerate, therefore no file:lines to cite". Per
`.claude/agents/effectiveness-evaluator.md` §6:

> **FAIL**: `acceptance_checks` red on UCIL run, OR UCIL underperforms
> baseline by > 0.5 on any criterion.

UCIL acceptance_checks contain a red → per-scenario FAIL → gate FAIL → exit 1.

This is a structural / scenario-fixture-alignment issue, not a regression
in the UCIL surface (`find_definition`, `find_references`). See
**Advisory** below for the analysis and the open escalation.

## Tool-availability probe

`tools/list` against `ucil-daemon mcp --stdio --repo <fixture>` reported all
22 §3.2 tools registered. The two tools required by the scenario
(`find_definition`, `find_references`) were both present.

| tool | listed? | tools/call returns real data? |
|---|---|---|
| `find_definition` | yes | yes — `_meta.source = "tree-sitter+kg"`, `found=true`, real `file_path`/`start_line` |
| `find_references` | yes | **no** — handler returns `_meta.not_yet_implemented: true` (Phase-1 stub, awaiting `P2-W7-F05`) |

Per `.claude/agents/effectiveness-evaluator.md` §"Tool-availability checks",
the probe is `tools/list`, and a tool is "operational" if registered +
responsive. `find_references` is registered and the call returns a
well-formed JSON-RPC `result` envelope (no transport error). The scenario
therefore runs (rather than being marked `skipped_tool_not_ready`); the
consequences of the stub for this scenario are inert here because the
ground-truth answer requires no `find_references` calls (nothing to
cross-reference when nothing exists).

## Scenarios

| scenario | tools_present | tools_real | UCIL pass? | UCIL score | Baseline score | Δ weighted | verdict |
|---|---|---|---|---|---|---|---|
| nav-rust-symbol | find_definition, find_references | find_definition (real); find_references (stub — `_meta.not_yet_implemented: true`) | **acceptance partial: 2 of 3 checks PASS, `cites file:line` FAIL on both sides** | 4.9231 | 4.9231 | 0.0000 | **FAIL** |

## Per-scenario detail

### nav-rust-symbol

**Fixture**: `tests/fixtures/rust-project` — a self-contained expression
parser/evaluator. `Cargo.lock` shows zero external dependencies;
`grep -rE "retry|backoff|exponential|http|reqwest|hyper"` across the
fixture returns zero matches.

**Ground truth**: "no HTTP-retry exponential-backoff functions exist".
Confirmed by independent grep of the fixture (sha256 of sorted file digest:
`3d8af62a21af3752c714dd40612f8e26b500cbb7775197e2d35f01f6c145c4c6` — byte-
for-byte identical to the prior run).

**Setup**
- `/tmp/ucil-eval-nav-rust-symbol/ucil/` — fresh fixture copy for UCIL run
- `/tmp/ucil-eval-nav-rust-symbol/baseline/` — fresh fixture copy for baseline run
- Output sink: `/tmp/ucil-eval-out/nav-rust-symbol.md` (rotated per side)
- Identical task prompt for both sides, captured in `task.md`

**UCIL run**
- Transport: `ucil-daemon mcp --stdio --repo /tmp/ucil-eval-nav-rust-symbol/ucil`
- MCP config: `--mcp-config mcp-ucil.json --strict-mcp-config`
- Settings: `--setting-sources ""` (no project / user / local settings)
- Session: `7b92c039-475c-43a2-aec2-4a7cc28e9b35` (fresh, `--no-session-persistence`)
- Model: `claude-opus-4-7`
- `duration_ms`: 107 988 (≈ 108 s)
- `num_turns`: 21
- `total_cost_usd`: 0.6246
- `usage.input_tokens`: 36
- `usage.cache_read_input_tokens`: 586 329
- `usage.cache_creation_input_tokens`: 30 445
- `usage.output_tokens`: 5 614
- Stop reason: `end_turn`
- Output: 46 lines, written to `/tmp/ucil-eval-out/nav-rust-symbol.md`

**Baseline run**
- Transport: no MCP servers (`mcp-empty.json` with empty `mcpServers`)
- MCP config: `--mcp-config mcp-empty.json --strict-mcp-config`
- Settings: `--setting-sources ""` (no project / user / local settings)
- Session: `b4bd2d6c-d78e-4817-8bdf-49df346e111a` (fresh, `--no-session-persistence`)
- Model: `claude-opus-4-7`
- `duration_ms`: 51 780 (≈ 52 s)
- `num_turns`: 11
- `total_cost_usd`: 0.2782
- `usage.input_tokens`: 16
- `usage.cache_read_input_tokens`: 264 975
- `usage.cache_creation_input_tokens`: 12 829
- `usage.output_tokens`: 2 596
- Stop reason: `end_turn`
- Output: 27 lines, written to `/tmp/ucil-eval-out/nav-rust-symbol.md`

**Acceptance checks** (run after copying each side's output to `/tmp/ucil-eval-out/nav-rust-symbol.md`)

| check | UCIL | Baseline |
|---|---|---|
| `test -f /tmp/ucil-eval-out/nav-rust-symbol.md` | PASS | PASS |
| `test $(wc -l < /tmp/ucil-eval-out/nav-rust-symbol.md) -ge 5` | PASS (46) | PASS (27) |
| `grep -qE "\.rs:[0-9]+" /tmp/ucil-eval-out/nav-rust-symbol.md` | **FAIL** | **FAIL** |

For both runs, the output mentions `.rs` filenames (`src/main.rs`,
`src/parser.rs`, etc.) and qualified paths like `parser::Parser::parse_expr`
and `eval_ctx::Context::eval_str`, but neither output contains the literal
form `<filename>.rs:<line>` because the conclusion is "no functions found"
and there is nothing to cite by file:line.

**Judge scoring** (fresh `claude -p` session per side, run from `/tmp` so the
repo's project hooks/settings cannot interfere; `--setting-sources ""`,
`--strict-mcp-config` with empty server map; rubric copied verbatim from
the scenario yaml; ground truth disclosed to the judge so it can score
correctness against the truth, not against the agent's own claims)

| criterion | weight | UCIL | Baseline |
|---|---|---|---|
| correctness         | 3.0 | 5 | 5 |
| caller_completeness | 2.0 | 5 | 5 |
| precision           | 1.0 | 5 | 5 |
| formatting          | 0.5 | 4 | 4 |
| **weighted mean**   |     | **4.9231** | **4.9231** |

UCIL judge: "Correctness: correctly identifies zero qualifying functions,
matching ground truth exactly; provides exhaustive evidence via case-
insensitive searches for retry/backoff/http/async/etc. and notes
Cargo.toml has no dependencies. Caller_completeness: vacuously satisfied —
no fabricated callers introduced. Precision: no false positives; correctly
excludes the parser/evaluator functions rather than misclassifying any.
Formatting: the prescribed H2-per-function + bulleted-caller structure is
N/A for an empty result set; the 'Result: None' section with module table
and evidence is clear and well-organized, though it does not cite any
file:line (acceptable here since nothing qualifies, hence slight
deduction)."

Baseline judge: "Correctness: The solution correctly concludes no qualifying
functions exist, matching ground truth, and provides supporting evidence
(the crate's purpose, file inventory, exhaustive keyword search, and
dependency check). Caller_completeness: Vacuously satisfied — no functions
enumerated means no fabricated callers introduced. Precision: Vacuously
satisfied — zero false positives since no functions were claimed.
Formatting: The H2-per-function structure technically doesn't apply, but
the response uses a clear H1 + bold conclusion + H2 'Search summary' with
bulleted evidence, which is structurally reasonable for a null result.
Minor deduction because no file:line citations are present (acceptance
check FAIL noted), though this is unavoidable given the correct null
answer."

**Verdict: FAIL** (per strict contract: UCIL acceptance_checks contain a
red, irrespective of the substantive judge-tie at 4.9231 each).

## Observations

- **Substantive parity, structural fail.** The UCIL surface (find_definition
  KG-backed, find_references stubbed) is unchanged from the prior run at
  `70aa72e`. Both UCIL and baseline correctly identified the negative
  ground truth ("no qualifying functions"). The judge's weighted mean is
  identical at 4.9231 on both sides (5/5/5/4 across the four criteria).
  **The fail is not because UCIL underperforms — it ties the baseline on
  every criterion.** The fail is because acceptance check #3
  (`cites at least one file:line`) is structurally inappropriate for the
  scenario-fixture pairing: the scenario asks for file:line of "each
  function found", but the fixture contains zero qualifying functions, so a
  truthful answer has nothing to cite.

- **Cost-edge to baseline (informational).** UCIL was ~2.08× slower (108 s
  vs 52 s) and used ~2.21× more cache-read tokens (586 K vs 265 K). When
  the task happens to be solvable by `Glob`/`Grep`/`Read` alone, the MCP
  tool-schema overhead is real time. For a positive-match task (real HTTP
  retry code to discover and cross-reference), the advantage shape would
  flip.

- **Stochastic acceptance-check satisfaction.** The prior run (4 h ago, at
  commit `70aa72e`) reported PASS on the same scenario with the same
  fixture and same UCIL surface. The prior outputs were 65 lines (UCIL) and
  92 lines (baseline) — substantially more verbose than this run's 46 and
  27 lines. With longer narrative output, agents incidentally include
  module-level file:line annotations (e.g., `src/parser.rs:42 — tokenizer
  entry`), which satisfies the regex `\.rs:[0-9]+`. With terser output (as
  in this run), the regex fails. This makes the rs-line acceptance check
  effectively flaky on the `rust-project` × `nav-rust-symbol` pairing.

- **`find_references` stub did not bite this scenario.** The ground-truth
  answer requires no `find_references` call (nothing to cross-reference
  when nothing exists). When `P2-W7-F05` lands, a follow-up run at that
  commit will exercise the positive-match path.

- **Both sides disambiguated the `**` exponentiation operator from
  exponential backoff.** The fixture's `powf` / `Pow` arms in `parser.rs`,
  `util.rs`, `eval_ctx.rs`, and `transform.rs` implement `aᵇ` for the toy
  expression language, not retry-delay computation. Both runs caught this
  and called it out explicitly.

- **Reproducibility note.** The CWD-outside-repo gotcha documented in the
  prior run (judge sessions hijacked by the repo `Stop` hook when CWD was
  inside the repo) was avoided here by `cd /tmp` + explicit
  `--setting-sources ""` on every judge invocation. Both judges returned
  clean JSON on first attempt.

## Advisory — scenario-fixture alignment defect

This run records a **strict-letter FAIL** that is not driven by a UCIL
regression. The mechanism is:

1. The scenario `nav-rust-symbol` task asks for "file:line of [each
   function's] definition" and "file:line of every caller".
2. The fixture `rust-project` contains zero qualifying functions (an
   expression parser/evaluator with no HTTP, no async, no retry).
3. A truthful answer ("no qualifying functions exist") therefore contains
   no file:line citations naturally.
4. Acceptance check #3 (`grep -qE "\.rs:[0-9]+"`) requires at least one
   `.rs:LINE` token in the output.
5. Whether the agent produces a `.rs:LINE` token incidentally (in module
   summaries, file inventories, etc.) is **stochastic LLM narrative
   style**. The prior run satisfied it; this run did not.

**Recommended scenario-fixture remediations** (to be triaged by user; see
escalation file): (a) augment the `rust-project` fixture with a
`http_client.rs` containing a real exponential-backoff function and at
least one caller, so the scenario has a positive ground truth and the
acceptance checks are naturally satisfied; or (b) split the rs-line check
so it only applies when the agent actually claims to have found a
function (e.g., gate it on `grep -q "^## " /tmp/ucil-eval-out/nav-rust-symbol.md`);
or (c) accept that this scenario-fixture pairing is a noisy-PASS / noisy-
FAIL and document that callers should not gate on this single check
unconditionally.

Until then, phase-1 effectiveness is reported as FAIL (strict contract),
with the substantive judge-tie noted prominently. The escalation file
documents the defect for human triage.

## Escalation filed

`ucil-build/escalations/20260507T0921Z-effectiveness-nav-rust-symbol-rs-line-flake.md`
documents the scenario-fixture alignment defect and proposes remediations.

## Gate contract (why this run is FAIL)

Per `.claude/agents/effectiveness-evaluator.md` §6 "Per-scenario verdict":

> **PASS**: `acceptance_checks` green AND `ucil_score >= baseline_score - 0.5` on every criterion.
> **WIN**: UCIL outperforms baseline by at least 1.0 on the weighted-average score.
> **FAIL**: `acceptance_checks` red on UCIL run, OR UCIL underperforms baseline by > 0.5 on any criterion.

UCIL acceptance_check `cites at least one file:line` is RED → per-scenario
**FAIL**. The substantive tie at 4.9231 weighted-mean is recorded but does
not override the strict-letter verdict.

Per `.claude/agents/effectiveness-evaluator.md` §"Exit code":

> 0 if gate passes, 1 if any scenario FAIL, 2 on evaluator-internal error.

One scenario FAIL recorded → **exit 1**.

## Reproducibility

All artefacts of this run are preserved under
`/tmp/ucil-eval-nav-rust-symbol/`:

- `task.md` — agent task prompt (identical for both sides)
- `ucil-output.md`, `baseline-output.md` — raw agent outputs
- `ucil-run.json`, `baseline-run.json` — `claude -p` JSON envelopes
  (duration, tokens, session ids, stop reason)
- `mcp-ucil.json`, `mcp-empty.json` — MCP configs for UCIL and baseline
- `run-ucil.sh`, `run-baseline.sh` — exact `claude` invocations
- `judge-ucil-prompt.md`, `judge-baseline-prompt.md` — verbatim judge prompts
- `judge-ucil-raw.json`, `judge-baseline-raw.json` — judge `claude -p` JSON envelopes
- `judge-ucil.json`, `judge-baseline.json` — extracted scoring JSON
- `ucil-session-id`, `baseline-session-id`, `judge-ucil-session-id`,
  `judge-baseline-session-id` — session UUIDs

Tempdirs are cleaned by the evaluator's `find -delete` step on exit (per
agent §"Exit cleanly"); the artefacts above must be collected before
the cleanup if a future operator wants to inspect a specific run by hand.
