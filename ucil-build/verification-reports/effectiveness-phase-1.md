# Effectiveness Report — Phase 1

Run at: 2026-05-08T00:55:00Z
Commit: fc50ef0426f0ed2001e7654e42113203c833deaa
Branch: feat/WO-0067-classifier-and-reason-parser
Evaluator: effectiveness-evaluator (fresh session, `claude-opus-4-7`)

## Summary

| metric | value |
|---|---|
| Scenarios discovered for phase 1 | 1 |
| Scenarios run | 1 |
| Scenarios skipped (tool_not_ready) | 0 |
| Scenarios skipped (defect) | 0 |
| Scenarios PASS | 1 |
| Scenarios WIN | 0 |
| Scenarios FAIL | 0 |

**Gate verdict: PASS** — the single phase-1-eligible scenario
(`nav-rust-symbol`) was executed end-to-end on the augmented `rust-project`
fixture (per ADR DEC-0017). Both UCIL and baseline produced
substantively-correct answers. All three acceptance checks passed on both
sides. LLM judges scored UCIL 5/5/5/5 and baseline 5/4/5/5 across
`correctness / caller_completeness / precision / formatting`.
Weighted means: UCIL **5.0000**, baseline **4.6923**, **Δ weighted =
+0.3077** (UCIL beats baseline within the WIN threshold of +1.0 → PASS but
not WIN).

Per `.claude/agents/effectiveness-evaluator.md` §6 "Per-scenario verdict":
- PASS: `acceptance_checks` green AND `ucil_score >= baseline_score - 0.5`
  on every criterion. **Both conditions hold for this run.**

The recurrent rs-line acceptance flake documented in
`ucil-build/escalations/20260507T0357Z-effectiveness-nav-rust-symbol-rs-line-flake.md`
is no longer load-bearing: with the augmented fixture having a real
`retry_with_backoff` helper, both sides naturally cite `.rs:LINE`
references. The flake mode that drove three prior FAILs on this scenario
is closed by ADR DEC-0017's fixture augmentation.

## Tool-availability probe

`tools/list` against `target/debug/ucil-daemon mcp --stdio --repo
/tmp/ucil-eval-nav-rust-symbol/ucil` reported all 22 §3.2 tools registered.
The two tools required by the scenario (`find_definition`,
`find_references`) were both present.

| tool | listed? | tools/call returns real data? |
|---|---|---|
| `find_definition` | yes | yes — `_meta.source = "tree-sitter+kg"`, `found=true`, real `file_path`/`start_line` |
| `find_references` | yes | **no** — handler returns `_meta.not_yet_implemented: true` (Phase-1 stub, awaiting `P2-W7-F05`) |

Per `.claude/agents/effectiveness-evaluator.md` §"Tool-availability checks",
the probe is `tools/list`, and a tool is "operational" if registered +
responsive. `find_references` is registered and the call returns a
well-formed JSON-RPC `result` envelope (no transport error). The scenario
therefore runs (rather than being marked `skipped_tool_not_ready`); the
consequences of the stub for this scenario are mitigated because UCIL's
`search_code` + `find_definition` together provide enough information for
the agent to enumerate callers (UCIL did exactly this, see "UCIL run"
below).

## Scenarios

| scenario | tools_present | tools_real | UCIL acceptance | UCIL weighted | Baseline weighted | Δ weighted | verdict |
|---|---|---|---|---|---|---|---|
| nav-rust-symbol | find_definition, find_references | find_definition (real); find_references (stub) | 3/3 PASS | 5.0000 | 4.6923 | +0.3077 | **PASS** |

## Per-scenario detail

### nav-rust-symbol

**Fixture**: `tests/fixtures/rust-project` (augmented per ADR DEC-0017 with
`src/http_client.rs` containing real `retry_with_backoff` +
`fetch_startup_banner`, plus a call site at `src/main.rs:24`).
Per-file SHA-256 inventory at
`/tmp/ucil-eval-nav-rust-symbol/fixture-checksum.txt`.

**Ground truth**: exactly one helper function directly implements the
exponential-backoff retry loop —
`rust_project::http_client::retry_with_backoff` at
`src/http_client.rs:37`, with one production caller at
`src/http_client.rs:64` (inside `fetch_startup_banner`), one doctest
caller at `src/http_client.rs:26`, and three unit-test callers at
`src/http_client.rs:84`, `91`, `110`. Whether `fetch_startup_banner`
itself qualifies (as a delegating wrapper) is interpretation-dependent;
both readings are admissible per the scenario task wording.

**Setup**
- `/tmp/ucil-eval-nav-rust-symbol/ucil/` — fresh fixture copy for UCIL run
- `/tmp/ucil-eval-nav-rust-symbol/baseline/` — fresh fixture copy for baseline run
- Output sink: `/tmp/ucil-eval-out/nav-rust-symbol.md` (rotated per side)
- Identical task prompt for both sides, captured in `task.md`

**UCIL run**
- Transport: `target/debug/ucil-daemon mcp --stdio --repo /tmp/ucil-eval-nav-rust-symbol/ucil`
- MCP config: `--mcp-config mcp-ucil.json --strict-mcp-config`
- Settings: `--setting-sources ""` (no project / user / local settings)
- Session: `6f40178b-3373-438c-9c70-ed2322a9f0ef` (fresh, deterministic UUID)
- Model: `claude-opus-4-7`
- `duration_ms`: 198 616 (≈ 199 s)
- `num_turns`: 31 (max_turns capped at 30; agent was thorough — used `find_definition`,
  `search_code`, `understand_code`, `understand_code` per file etc.)
- `total_cost_usd`: 1.3091
- `usage.input_tokens`: 70
- `usage.cache_read_input_tokens`: 1 020 973
- `usage.cache_creation_input_tokens`: 75 594
- `usage.output_tokens`: 13 008
- `is_error`: true (`error_max_turns`) — but the output file was written and
  fully formed before the cap was reached. The cap fired during a final
  verification turn (UCIL was double-checking its caller list). Output is
  substantively complete and acceptance-passing.
- Output: 14 lines, written to `/tmp/ucil-eval-out/nav-rust-symbol.md`,
  preserved at `ucil-output.md`

**Baseline run**
- Transport: no MCP servers (`mcp-empty.json` with empty `mcpServers`)
- MCP config: `--mcp-config mcp-empty.json --strict-mcp-config`
- Settings: `--setting-sources ""` (no project / user / local settings)
- Session: `c3dfd64c-d531-43ae-beb5-3c0a6ec6bc8b` (fresh, deterministic UUID)
- Model: `claude-opus-4-7`
- `duration_ms`: 91 763 (≈ 92 s)
- `num_turns`: 16
- `total_cost_usd`: 0.4914
- `usage.input_tokens`: 21
- `usage.cache_read_input_tokens`: 468 104
- `usage.cache_creation_input_tokens`: 19 107
- `usage.output_tokens`: 5 486
- `is_error`: false (`success`)
- Output: 23 lines, written to `/tmp/ucil-eval-out/nav-rust-symbol.md`,
  preserved at `baseline-output.md`

**Acceptance checks** (run after copying each side's output to `/tmp/ucil-eval-out/nav-rust-symbol.md`)

| check | UCIL | Baseline |
|---|---|---|
| `test -f /tmp/ucil-eval-out/nav-rust-symbol.md` | PASS | PASS |
| `test $(wc -l < ...) -ge 5` | PASS (14) | PASS (23) |
| `grep -qE "\.rs:[0-9]+" ...` | PASS | PASS |

The augmented fixture's positive ground truth cleanly drives both sides to
emit `.rs:LINE` citations naturally (UCIL: `src/http_client.rs:37`,
`src/http_client.rs:64`, etc.; baseline: same set + `src/main.rs:24`,
`src/http_client.rs:124`).

**Judge scoring** (fresh `claude -p` session per side, run from `/tmp` with
`--setting-sources ""` and `--strict-mcp-config` against an empty server
map; rubric copied verbatim from the scenario yaml; ground truth disclosed
to the judge so it can score correctness against the truth, not against
the agent's own claims)

| criterion | weight | UCIL | Baseline |
|---|---|---|---|
| correctness         | 3.0 | 5 | 5 |
| caller_completeness | 2.0 | **5** | **4** |
| precision           | 1.0 | 5 | 5 |
| formatting          | 0.5 | 5 | 5 |
| **weighted mean**   |     | **5.0000** | **4.6923** |

UCIL judge: "The solution correctly identifies
`rust_project::http_client::retry_with_backoff` as the qualifying function
with the exact definition line (src/http_client.rs:37) and explicitly
justifies treating `fetch_startup_banner` as a caller rather than a
separate qualifying function — an interpretation explicitly allowed by
the ground truth. All five callers from the ground truth are listed with
correct file:line references (26 doctest, 64 fetch_startup_banner, 84/91/110
tests), with no fabricated or stdlib callers. The H2-per-function
structure with a bulleted caller list is followed cleanly, and the
explanatory note adds clarity without violating the format."

Baseline judge: "Both qualifying functions are correctly identified with
accurate definition lines (`retry_with_backoff` at `src/http_client.rs:37`
and `fetch_startup_banner` at `src/http_client.rs:62`)…The one omission is
the doctest caller at `src/http_client.rs:26`, which the task's 'every
place it is CALLED FROM' phrasing would include, costing a point on
caller_completeness. Formatting follows the required H2-per-function with
bulleted caller lists, and the extra intro paragraph does not violate the
structure."

**Verdict: PASS.** Acceptance checks all green on UCIL. UCIL >= baseline
on every criterion (UCIL beats baseline by +1 on `caller_completeness`,
ties on the other three). UCIL weighted mean (5.0000) exceeds baseline
(4.6923) by +0.3077, comfortably above the −0.5 PASS floor but below the
+1.0 WIN threshold — a clean PASS, not a WIN.

## Observations

- **Fixture augmentation worked as designed.** The DEC-0017 fixture
  augmentation closes out the recurrent rs-line flake. Across the four
  prior runs (commits `70aa72e` → `f4adc41` → `762bd5d` → `1f20c3b`),
  acceptance check #3 produced four different verdict pairs because both
  sides were guessing-at-citations on a negative-ground-truth fixture.
  This run, with one positive ground-truth function to cite, both sides
  trivially satisfied the check. The flake mode is closed for this
  scenario × fixture pairing.
- **UCIL hit max_turns=30 but produced complete output.** The
  `error_max_turns` is cosmetic for this run — the output file was
  written and fully formed before the cap. UCIL ran `find_definition`
  on `retry_with_backoff` (got `src/http_client.rs:37` via tree-sitter+kg),
  then leaned on `search_code` to enumerate callers (since `find_references`
  is a Phase-1 stub). The agent then verified each caller line with
  `understand_code`. A future run after `P2-W7-F05` lands `find_references`
  should converge in 10–15 turns instead.
- **UCIL won caller_completeness by catching the doctest caller
  (line 26).** This is a real signal: UCIL's `search_code` returned the
  doc-comment example match, and the agent included it. Baseline's
  `grep -rn "retry_with_backoff"` would have surfaced the same match,
  but the baseline agent narrated only the production + test callers,
  not the doctest. Net: +1 caller_completeness for UCIL → +0.3077
  weighted, the only material delta.
- **Cost-edge favors baseline (informational).** UCIL was ~2.16× slower
  (199 s vs 92 s) and consumed ~2.4× the tokens (1.10M vs 487K
  cache_read; 13K vs 5.5K output) and ~2.7× the cost ($1.31 vs $0.49).
  The slowdown is concentrated in the max_turns cap fight (`find_references`
  stub forces UCIL to fall back to `search_code` + `understand_code`,
  which UCIL then verifies twice). When `find_references` lands proper, the
  cost shape should narrow significantly. Substantive accuracy still
  favored UCIL (+1 on caller_completeness with no precision regression).
- **`find_references` stub mitigation.** UCIL successfully worked around
  the stub by calling `search_code` (which returns real text-search hits
  including the doctest) and then using `understand_code` for line-level
  context. This is the "graceful degradation" path documented in the
  ucil-daemon design — the scenario didn't have to be skipped.
- **Token cost shape.** UCIL output_tokens (13 008) was 2.37× baseline
  (5 486), driven by UCIL's verbose thinking traces. UCIL cache_read was
  2.18× baseline. UCIL's larger input footprint reflects the MCP tool
  schemas being attached (22 tool definitions vs 0). When a task is
  symbol-heavy (positive matches found), this overhead amortises; when
  it's text-search-only, baseline can be cheaper (as on this run).

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
- `fixture-checksum.txt` — per-file SHA-256 of the fixture (10 files)

## Gate contract (why this run is PASS)

Per `.claude/agents/effectiveness-evaluator.md` §6 "Per-scenario verdict":

> **PASS**: `acceptance_checks` green AND `ucil_score >= baseline_score - 0.5` on every criterion.
> **WIN**: UCIL outperforms baseline by at least 1.0 on the weighted-average score.
> **FAIL**: `acceptance_checks` red on UCIL run, OR UCIL underperforms baseline by > 0.5 on any criterion.

PASS conditions both hold:
1. All three UCIL acceptance_checks GREEN.
2. Per-criterion deltas (UCIL − Baseline): correctness 0, caller_completeness +1, precision 0, formatting 0.
   No criterion shows UCIL underperforming. The minimum criterion delta is 0; the maximum is +1.

WIN condition does NOT hold (Δ weighted = +0.3077, below the +1.0
threshold). Verdict is PASS.

Per `.claude/agents/effectiveness-evaluator.md` §"Exit code":

> 0 if gate passes, 1 if any scenario FAIL, 2 on evaluator-internal error.

Zero scenarios FAIL → **exit 0**.
