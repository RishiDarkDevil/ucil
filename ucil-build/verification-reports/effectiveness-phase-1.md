# Effectiveness Report — Phase 1

Run at: 2026-05-07T05:31:35Z
Commit: 762bd5dc4e8e2725b39ad3b2819d8541a692a6ac
Branch: feat/WO-0066-find-similar-mcp-tool
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
functions exist", matching ground truth). LLM judges scored UCIL 5/5/5/3 and
baseline 5/5/5/4 across `correctness / caller_completeness / precision /
formatting`. Weighted means: UCIL **4.8462**, baseline **4.9231**, **Δ
weighted = −0.0769** (UCIL ≈ baseline within judge noise).

**Why FAIL:**
1. Acceptance check #3 (`cites at least one .rs:LINE`) was RED on the **UCIL**
   side and PASS on the baseline side. Per `.claude/agents/effectiveness-evaluator.md`
   §6 ("FAIL: acceptance_checks red on UCIL run"), this triggers a per-scenario
   FAIL.
2. The UCIL judge gave `formatting = 3` while baseline got `formatting = 4`
   (Δ = −1.0, exceeds the 0.5 tolerance window). Per the same §6 ("OR UCIL
   underperforms baseline by > 0.5 on any criterion"), this is a second
   independent FAIL trigger.

Both fail-triggers are **driven by a single root cause**: this run's UCIL
output happened to omit `.rs:LINE` tokens entirely, while the baseline
incidentally emitted `src/util.rs:352` in its evidence section. The judge
explicitly tied the formatting deduction to the missing `.rs:LINE` tokens.
The acceptance check and the formatting criterion are not independent
signals — they're both readings of the same LLM-narrative-style coin flip.

This is a **structural / scenario-fixture-alignment issue**, not a
regression in the UCIL surface (`find_definition` real KG-backed,
`find_references` Phase-1 stub awaiting `P2-W7-F05`). See **Advisory**
below for the analysis and the open (resolved-deferred) escalation that
already documents this defect class.

## Tool-availability probe

`tools/list` against
`ucil-daemon mcp --stdio --repo /tmp/ucil-mcp-probe/repo` reported all 22
§3.2 tools registered. The two tools required by the scenario
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
| nav-rust-symbol | find_definition, find_references | find_definition (real); find_references (stub — `_meta.not_yet_implemented: true`) | **acceptance partial: 2 of 3 checks PASS, `cites file:line` FAIL on UCIL only** | 4.8462 | 4.9231 | −0.0769 | **FAIL** |

## Per-scenario detail

### nav-rust-symbol

**Fixture**: `tests/fixtures/rust-project` — a self-contained expression
parser/evaluator. `Cargo.toml` declares zero dependencies;
`grep -rE "retry|backoff|exponential|http|reqwest|hyper|async"` across the
fixture returns zero matches.

**Ground truth**: "no HTTP-retry exponential-backoff functions exist".
Confirmed by independent grep of the fixture (per-file SHA-256 inventory at
`/tmp/ucil-eval-nav-rust-symbol/fixture-checksum.txt`, byte-for-byte
identical to the prior run at commit `f4adc41`).

**Setup**
- `/tmp/ucil-eval-nav-rust-symbol/ucil/` — fresh fixture copy for UCIL run
- `/tmp/ucil-eval-nav-rust-symbol/baseline/` — fresh fixture copy for baseline run
- Output sink: `/tmp/ucil-eval-out/nav-rust-symbol.md` (rotated per side)
- Identical task prompt for both sides, captured in `task.md`

**UCIL run**
- Transport: `ucil-daemon mcp --stdio --repo /tmp/ucil-eval-nav-rust-symbol/ucil`
- MCP config: `--mcp-config mcp-ucil.json --strict-mcp-config`
- Settings: `--setting-sources ""` (no project / user / local settings)
- Session: `7b287d09-c6a5-4424-a439-75f277164e19` (fresh, `--no-session-persistence`)
- Model: `claude-opus-4-7`
- `duration_ms`: 60 656 (≈ 61 s)
- `num_turns`: 9
- `total_cost_usd`: 0.2759
- `usage.input_tokens`: 14
- `usage.cache_read_input_tokens`: 225 927
- `usage.cache_creation_input_tokens`: 14 810
- `usage.output_tokens`: 2 788
- Stop reason: `end_turn`
- Output: 48 lines, written to `/tmp/ucil-eval-out/nav-rust-symbol.md`

**Baseline run**
- Transport: no MCP servers (`mcp-empty.json` with empty `mcpServers`)
- MCP config: `--mcp-config mcp-empty.json --strict-mcp-config`
- Settings: `--setting-sources ""` (no project / user / local settings)
- Session: `e3da838a-ee8d-4a97-b74f-640fda73a1a7` (fresh, `--no-session-persistence`)
- Model: `claude-opus-4-7`
- `duration_ms`: 46 473 (≈ 46 s)
- `num_turns`: 11
- `total_cost_usd`: 0.2728
- `usage.input_tokens`: 16
- `usage.cache_read_input_tokens`: 265 744
- `usage.cache_creation_input_tokens`: 12 226
- `usage.output_tokens`: 2 515
- Stop reason: `end_turn`
- Output: 27 lines, written to `/tmp/ucil-eval-out/nav-rust-symbol.md`

**Acceptance checks** (run after copying each side's output to `/tmp/ucil-eval-out/nav-rust-symbol.md`)

| check | UCIL | Baseline |
|---|---|---|
| `test -f /tmp/ucil-eval-out/nav-rust-symbol.md` | PASS | PASS |
| `test $(wc -l < /tmp/ucil-eval-out/nav-rust-symbol.md) -ge 5` | PASS (48) | PASS (27) |
| `grep -qE "\.rs:[0-9]+" /tmp/ucil-eval-out/nav-rust-symbol.md` | **FAIL** | **PASS** |

The UCIL output mentions `src/main.rs`, `src/parser.rs`, etc. (and a
qualified path `rust_project`), but never the literal `<file>.rs:<line>`
form because the conclusion is "no functions found" and there is nothing to
cite. The baseline output incidentally emits `src/util.rs:352` while
referencing the only superficially-suspicious `* 2` arithmetic match. This
is the same flake mode documented in
`ucil-build/escalations/20260507T0357Z-effectiveness-nav-rust-symbol-rs-line-flake.md`.

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
| formatting          | 0.5 | **3** | **4** |
| **weighted mean**   |     | **4.8462** | **4.9231** |

UCIL judge: "The solution correctly identifies that zero qualifying
functions exist, matching the disclosed ground truth. The methodology is
thorough: searches for HTTP/retry/backoff/async identifiers, inspection of
Cargo.toml confirming zero dependencies, and enumeration of the source
tree. Correctness, caller_completeness, and precision are all exemplary
because the empty result set is the truthful answer — no functions missed,
no fabricated callers, no false positives. Formatting is partial: the spec
asks for H2-per-function with bulleted callers, which is vacuously
satisfied by an empty set, but the document does not cite any .rs:LINE
tokens (acceptance check FAIL noted). The explanatory prose is appropriate
for a null result, though strict adherence to the H2 structure is not
applicable here."

Baseline judge: "The solution correctly identifies that zero qualifying
functions exist, matching ground truth. Evidence is thorough: cites
Cargo.toml having no HTTP/async deps, comprehensive grep across relevant
keywords, and addresses the only superficially-suspicious match
(arithmetic doubling). Caller completeness is vacuously perfect since
there are no functions to enumerate. No false positives. Formatting
deviates slightly from the spec (no H2-per-function structure since none
exist), but this is unavoidable given the correct null answer; the
document is well-structured with clear evidence sections."

**Verdict: FAIL** (per strict contract: UCIL acceptance_checks contain a
red AND UCIL formatting underperforms baseline by 1.0 — both the same
narrative-style coin flip on `.rs:LINE` token emission).

## Observations

- **Substantive parity, structural fail.** The UCIL surface (`find_definition`
  KG-backed, `find_references` stubbed) is unchanged from the prior runs.
  Both UCIL and baseline correctly identified the negative ground truth
  ("no qualifying functions"). All three weighted criteria with weight ≥ 1.0
  scored 5/5 on both sides. The only differentiator is the 0.5-weighted
  `formatting` criterion (UCIL 3, baseline 4), and the judge explicitly
  tied that 1-point gap to the missing `.rs:LINE` tokens — i.e., the same
  narrative coin flip that drives the acceptance check.
  Net Δ-weighted = −0.0769, well within judge noise.

- **Cost-edge to baseline (informational).** UCIL was ~1.31× slower (61 s
  vs 46 s) and used ~85 % of the baseline's cache-read tokens (226 K vs
  266 K). When the task happens to be solvable by `Glob`/`Grep`/`Read`
  alone, the MCP tool-schema overhead is real time. For a positive-match
  task (real HTTP retry code to discover and cross-reference), the
  advantage shape would flip.

- **Stochastic acceptance-check satisfaction (recurrent).** Across the
  three known runs at three commits (`70aa72e`, `f4adc41`, `762bd5d`):
  - `70aa72e`: PASS on both sides (verbose narrative incidentally emitted
    file:line tokens)
  - `f4adc41`: FAIL on both sides (terse narrative; neither emitted)
  - `762bd5d` (this run): UCIL FAIL, baseline PASS (asymmetric — only the
    baseline emitted)

  Same fixture, same scenario, same UCIL surface, three different
  acceptance-check verdicts. **This is definitive evidence that the
  rs-line check on the negative-ground-truth `rust-project` × `nav-rust-symbol`
  pairing is a flake.**

- **`find_references` stub did not bite this scenario.** The ground-truth
  answer requires no `find_references` call (nothing to cross-reference
  when nothing exists). When `P2-W7-F05` lands, a follow-up run at that
  commit will exercise the positive-match path and the stub will become
  load-bearing.

- **Both sides disambiguated the `**` exponentiation operator from
  exponential backoff.** The fixture's `BinOp::Pow` arms in `parser.rs`,
  `util.rs`, `eval_ctx.rs`, and `transform.rs` implement `aᵇ` for the toy
  expression language, not retry-delay computation. Both runs caught this
  and called it out explicitly.

- **Reproducibility note.** Judge sessions ran from `cd /tmp` with
  explicit `--setting-sources ""` to avoid the repo Stop-hook hijacking
  documented in the prior reports. Both judges returned clean JSON on
  first attempt.

## Advisory — scenario-fixture alignment defect (recurrent)

This run records a **strict-letter FAIL** that is not driven by a UCIL
regression. The mechanism is the one already escalated and resolved-as-deferred
in `ucil-build/escalations/20260507T0357Z-effectiveness-nav-rust-symbol-rs-line-flake.md`:

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
   style**. Three runs at three commits have produced three different
   verdicts on this single check: PASS/PASS, FAIL/FAIL, FAIL/PASS.

**Recommended scenario-fixture remediations** (to be triaged by user; see
the existing escalation): (a) augment the `rust-project` fixture with a
`http_client.rs` containing a real exponential-backoff function and at
least one caller, so the scenario has a positive ground truth and the
acceptance checks are naturally satisfied; or (b) split the rs-line check
so it only applies when the agent actually claims to have found a
function (e.g., gate it on `grep -q "^## " /tmp/ucil-eval-out/nav-rust-symbol.md`);
or (c) accept that this scenario-fixture pairing is a noisy-PASS / noisy-
FAIL and document that callers should not gate on this single check
unconditionally.

The existing escalation explicitly defers all three options to a Phase-8
release-prep effectiveness audit (gated on planner / ADR approval, since
both `tests/fixtures/**` and `tests/scenarios/**` are protected by root
CLAUDE.md). **No new escalation is filed for this run** — the defect class
is unchanged, and the prior escalation's resolution note specifies that
"the autonomous loop should not treat a FAIL on this scenario as a UCIL
regression without checking the report's Substantive judge-tie line".

## Substantive judge-tie line

Per-criterion deltas (UCIL − Baseline):
- correctness: 0
- caller_completeness: 0
- precision: 0
- formatting: −1 (entirely attributable to the missing `.rs:LINE` token,
  which is the same coin flip as acceptance check #3)

Weighted-mean delta: **−0.0769** (UCIL 4.8462 vs baseline 4.9231).

UCIL did not regress on any substantive criterion. The single-point
formatting gap is a downstream symptom of the same narrative-style flake
that drives acceptance check #3. The substantive UCIL behaviour is
**indistinguishable from baseline** on this negative-ground-truth scenario.

## Gate contract (why this run is FAIL)

Per `.claude/agents/effectiveness-evaluator.md` §6 "Per-scenario verdict":

> **PASS**: `acceptance_checks` green AND `ucil_score >= baseline_score - 0.5` on every criterion.
> **WIN**: UCIL outperforms baseline by at least 1.0 on the weighted-average score.
> **FAIL**: `acceptance_checks` red on UCIL run, OR UCIL underperforms baseline by > 0.5 on any criterion.

Two FAIL triggers fire on this run:
1. UCIL acceptance_check `cites at least one file:line` is RED (baseline PASS).
2. UCIL `formatting` = 3 vs baseline `formatting` = 4 (Δ = −1.0 > 0.5).

Both triggers are downstream of the same narrative-style coin flip
documented above and in the existing escalation. The substantive tie at
4.8462 / 4.9231 weighted-mean (Δ = −0.0769) is recorded but does not
override the strict-letter verdict.

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
- `judge-ucil-prompt.md` (`/tmp/ucil-eval-judge-nav-rust-symbol-ucil.md`),
  `judge-baseline-prompt.md` (`/tmp/ucil-eval-judge-nav-rust-symbol-baseline.md`)
  — verbatim judge prompts
- `judge-ucil-raw.json`, `judge-baseline-raw.json` — judge `claude -p`
  JSON envelopes
- `judge-ucil.json`, `judge-baseline.json` — extracted scoring JSON
- `ucil-session-id`, `baseline-session-id`, `judge-ucil-session-id`,
  `judge-baseline-session-id` — session UUIDs
- `fixture-checksum.txt` — per-file SHA-256 of the fixture

Tempdirs are cleaned by the evaluator's `find -delete` step on exit (per
agent §"Exit cleanly"); the artefacts above must be collected before
the cleanup if a future operator wants to inspect a specific run by hand.
