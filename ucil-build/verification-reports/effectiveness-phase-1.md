# Effectiveness Report — Phase 1

Run at: 2026-05-07T06:07:39Z
Commit: 1f20c3bac6348cfb50d2ffd008ad0a1e6282a7fb
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
   side. Per `.claude/agents/effectiveness-evaluator.md`
   §6 ("FAIL: acceptance_checks red on UCIL run"), this triggers a per-scenario
   FAIL. (Baseline also RED on this run — see historical table below.)
2. The UCIL judge gave `formatting = 3` while baseline got `formatting = 4`
   (Δ = −1.0, exceeds the 0.5 tolerance window). Per the same §6 ("OR UCIL
   underperforms baseline by > 0.5 on any criterion"), this is a second
   independent FAIL trigger.

Both fail-triggers are **driven by a single root cause**: the truthful
answer to this task on this fixture ("no qualifying functions") naturally
contains no `.rs:LINE` tokens. Whether either side incidentally emits one
in evidence prose is stochastic LLM narrative style. On this run *neither*
side emitted, but the UCIL judge tied a one-point formatting deduction
explicitly to the missing token, while the baseline judge granted formatting
the benefit of the doubt for the same null-result adaptation. The judges
ran on independent fresh sessions; the asymmetry sits inside known judge
noise.

This is the same **structural / scenario-fixture-alignment issue** already
escalated and resolved-as-deferred in
`ucil-build/escalations/20260507T0357Z-effectiveness-nav-rust-symbol-rs-line-flake.md`.
The UCIL surface (`find_definition` real KG-backed, `find_references`
Phase-1 stub awaiting `P2-W7-F05`) is unchanged from the prior runs; the
diff from the previous evaluation commit (`762bd5d`) to this commit
(`1f20c3b`) touches **only** docs / verification-report / coverage files
— no UCIL source changes. See **Advisory** below.

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
| nav-rust-symbol | find_definition, find_references | find_definition (real); find_references (stub — `_meta.not_yet_implemented: true`) | **acceptance partial: 2 of 3 checks PASS, `cites file:line` FAIL on UCIL (and baseline)** | 4.8462 | 4.9231 | −0.0769 | **FAIL** |

## Per-scenario detail

### nav-rust-symbol

**Fixture**: `tests/fixtures/rust-project` — a self-contained expression
parser/evaluator. `Cargo.toml` declares zero dependencies;
`grep -rE "retry|backoff|exponential|http|reqwest|hyper|async"` across the
fixture returns zero matches.

**Ground truth**: "no HTTP-retry exponential-backoff functions exist".
Confirmed by independent grep of the fixture (per-file SHA-256 inventory at
`/tmp/ucil-eval-nav-rust-symbol/fixture-checksum.txt`, byte-for-byte
identical to the prior run at commit `762bd5d`).

**Setup**
- `/tmp/ucil-eval-nav-rust-symbol/ucil/` — fresh fixture copy for UCIL run
- `/tmp/ucil-eval-nav-rust-symbol/baseline/` — fresh fixture copy for baseline run
- Output sink: `/tmp/ucil-eval-out/nav-rust-symbol.md` (rotated per side)
- Identical task prompt for both sides, captured in `task.md`

**UCIL run**
- Transport: `ucil-daemon mcp --stdio --repo /tmp/ucil-eval-nav-rust-symbol/ucil`
- MCP config: `--mcp-config mcp-ucil.json --strict-mcp-config`
- Settings: `--setting-sources ""` (no project / user / local settings)
- Session: `90fc8187-ea02-43d9-ac36-d5841e347dae` (fresh, deterministic UUID)
- Model: `claude-opus-4-7`
- `duration_ms`: 68 381 (≈ 68 s)
- `num_turns`: 13
- `total_cost_usd`: 0.3361
- `usage.input_tokens`: 18
- `usage.cache_read_input_tokens`: 325 472
- `usage.cache_creation_input_tokens`: 13 569
- `usage.output_tokens`: 3 517
- `is_error`: false
- Output: 42 lines, written to `/tmp/ucil-eval-out/nav-rust-symbol.md`

**Baseline run**
- Transport: no MCP servers (`mcp-empty.json` with empty `mcpServers`)
- MCP config: `--mcp-config mcp-empty.json --strict-mcp-config`
- Settings: `--setting-sources ""` (no project / user / local settings)
- Session: `c50960b9-0857-4798-a2c7-156bafe9ae87` (fresh, deterministic UUID)
- Model: `claude-opus-4-7`
- `duration_ms`: 57 983 (≈ 58 s)
- `num_turns`: 16
- `total_cost_usd`: 0.3656
- `usage.input_tokens`: 21
- `usage.cache_read_input_tokens`: 394 544
- `usage.cache_creation_input_tokens`: 12 757
- `usage.output_tokens`: 3 514
- `is_error`: false
- Output: 28 lines, written to `/tmp/ucil-eval-out/nav-rust-symbol.md`

**Acceptance checks** (run after copying each side's output to `/tmp/ucil-eval-out/nav-rust-symbol.md`)

| check | UCIL | Baseline |
|---|---|---|
| `test -f /tmp/ucil-eval-out/nav-rust-symbol.md` | PASS | PASS |
| `test $(wc -l < /tmp/ucil-eval-out/nav-rust-symbol.md) -ge 5` | PASS (42) | PASS (28) |
| `grep -qE "\.rs:[0-9]+" /tmp/ucil-eval-out/nav-rust-symbol.md` | **FAIL** | **FAIL** |

The UCIL output cites parser line numbers as "`src/parser.rs at lines 581,
892, 914`" (prose form, not `<file>.rs:<line>` token form). The baseline
output enumerates the crate layout (`lib.rs`, `main.rs`, `eval_ctx.rs`,
…) without any line-number citation. Neither side produced a literal
`.rs:NUMBER` token because both correctly concluded "no qualifying
functions exist" and there was nothing to cite. This is the same flake
mode documented in
`ucil-build/escalations/20260507T0357Z-effectiveness-nav-rust-symbol-rs-line-flake.md`,
and matches the f4adc41 case in the historical record below.

**Judge scoring** (fresh `claude -p` session per side, run from `/tmp` so
the repo's project hooks/settings cannot interfere; `--setting-sources ""`,
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

UCIL judge: "Correctly identifies the truthful empty result with thorough
negative evidence (Cargo.toml inspection, dependency check, exhaustive
keyword scan). No fabricated functions or callers. Formatting deviates
from the prescribed H2-per-function structure (uses Summary/Evidence
sections instead) and writes line references as 'parser.rs at lines 581,
892, 914' rather than the canonical file.rs:line token, which is why the
acceptance grep failed."

Baseline judge: "The solution correctly identifies that no qualifying
functions exist, matching the ground truth, and provides strong evidence
(crate layout, dependency list, exhaustive keyword grep). The acceptance
check for `.rs:<line>` tokens is inapplicable to the correct answer.
Formatting is slightly docked because the spec called for H2-per-function
structure; with no functions, the document reasonably uses an H2 for the
search summary instead, which is a sensible adaptation."

**Verdict: FAIL** (per strict contract: UCIL acceptance_checks contain a
red AND UCIL formatting underperforms baseline by 1.0 — both same
narrative-style coin flip on `.rs:LINE` token emission).

## Observations

- **Substantive parity, structural fail.** The UCIL surface is unchanged
  from the prior runs (no source diff between `762bd5d` and `1f20c3b`
  touches UCIL crates). Both UCIL and baseline correctly identified the
  negative ground truth ("no qualifying functions"). All three weighted
  criteria with weight ≥ 1.0 scored 5/5 on both sides. The only
  differentiator is the 0.5-weighted `formatting` criterion (UCIL 3,
  baseline 4) — judge noise, with the deduction explicitly anchored to the
  missing `.rs:LINE` token (the same coin flip that drives the acceptance
  check). Net Δ-weighted = −0.0769, well within judge noise.

- **Cost-edge to baseline (informational).** UCIL was ~1.18× slower
  (68 s vs 58 s) and used ~83 % of the baseline's cache-read tokens
  (325 K vs 395 K). When the task happens to be solvable by
  `Glob`/`Grep`/`Read` alone, the MCP tool-schema overhead is real time.
  For a positive-match task (real HTTP retry code to discover and
  cross-reference), the advantage shape would flip. UCIL's total_cost_usd
  was actually slightly lower this run (0.3361 vs 0.3656) — cost is
  driven primarily by output tokens, which were near-identical
  (3517 vs 3514).

- **Stochastic acceptance-check satisfaction (recurrent).** Across the
  four known runs at four commits:

  | commit | UCIL acceptance #3 | Baseline acceptance #3 |
  |---|---|---|
  | `70aa72e` | PASS | PASS |
  | `f4adc41` | FAIL | FAIL |
  | `762bd5d` | FAIL | PASS |
  | `1f20c3b` (this run) | FAIL | FAIL |

  Same fixture, same scenario, same UCIL surface, four different
  acceptance-check pairs. **This is definitive evidence that the rs-line
  check on the negative-ground-truth `rust-project` × `nav-rust-symbol`
  pairing is a flake.** This run is the third instance of the
  documented flake mode.

- **`find_references` stub did not bite this scenario.** The ground-truth
  answer requires no `find_references` call (nothing to cross-reference
  when nothing exists). When `P2-W7-F05` lands, a follow-up run at that
  commit will exercise the positive-match path and the stub will become
  load-bearing.

- **Both sides disambiguated the `**` exponentiation operator from
  exponential backoff.** The fixture's `BinOp::Pow` arms in `parser.rs`,
  `util.rs`, `eval_ctx.rs`, and `transform.rs` implement `aᵇ` for the toy
  expression language, not retry-delay computation. UCIL explicitly
  noted that the only `loop` matches are tokenizer/parser inner loops in
  `parser.rs`; baseline's grep set explicitly excluded math operators
  and ran negative on every retry/backoff/HTTP keyword.

- **Reproducibility note.** Judge sessions ran from `cd /tmp` with
  explicit `--setting-sources ""` to avoid the repo Stop-hook hijacking
  documented in the prior reports. Both judges returned clean JSON on
  first attempt.

## Advisory — scenario-fixture alignment defect (recurrent)

This run records a **strict-letter FAIL** that is not driven by a UCIL
regression. The mechanism is the one already escalated and
resolved-as-deferred in
`ucil-build/escalations/20260507T0357Z-effectiveness-nav-rust-symbol-rs-line-flake.md`:

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
   style**. Four runs at four commits have produced four different
   acceptance-check pair verdicts on this single check.

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
1. UCIL acceptance_check `cites at least one file:line` is RED (baseline
   also RED on this run, but the rubric reads UCIL acceptance independently).
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
- `ucil-run.stderr`, `baseline-run.stderr` — child-process stderr
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
- `fixture-checksum.txt` — per-file SHA-256 of the fixture (9 files)

Tempdirs are cleaned by the evaluator's `find -delete` step on exit (per
agent §"Exit cleanly"); the artefacts above must be collected before
the cleanup if a future operator wants to inspect a specific run by hand.
