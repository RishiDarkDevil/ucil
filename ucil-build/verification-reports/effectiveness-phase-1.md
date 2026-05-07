# Effectiveness Report — Phase 1

Run at: 2026-05-07T03:23:46Z
Commit: 70aa72e12f2655fbd3fbb0b5799c67a726b23f88
Branch: feat/WO-0065-vector-query-p95-bench
Evaluator: effectiveness-evaluator (fresh session, `claude-opus-4-7`)

## Summary

| metric | value |
|---|---|
| Scenarios discovered for phase 1 | 1 |
| Scenarios run | 1 |
| Scenarios skipped (tool_not_ready) | 0 |
| Scenarios PASS | 1 |
| Scenarios WIN | 0 |
| Scenarios FAIL | 0 |

**Gate verdict: PASS (substantive)** — the single phase-1-eligible
scenario (`nav-rust-symbol`) was executed end-to-end. UCIL and
baseline runs both completed, every `acceptance_check` was green on
both sides, and the LLM judge scored UCIL and baseline equally
(5.0/5.0 weighted-average each, Δ = 0.0 → PASS per rubric §6).

This re-run reproduces the substantive-tie outcome originally recorded
on 2026-04-19 (commit `26dfeb23`) at the current head
(`70aa72e1`). The Phase-1 surface that determines this scenario's
outcome — `find_definition` (real, KG-backed) and `find_references`
(registered, returns the `_meta.not_yet_implemented` stub) — has not
changed, and the fixture (`tests/fixtures/rust-project`) is byte-for-byte
identical to the prior run (sha256 of sorted file digest:
`3d8af62a21af3752c714dd40612f8e26b500cbb7775197e2d35f01f6c145c4c6`).

## Tool-availability probe

`tools/list` against `ucil-daemon mcp --stdio --repo <fixture>` reported
all 22 §3.2 tools registered. The two tools required by the scenario
(`find_definition`, `find_references`) were both present.

| tool | listed? | tools/call returns real data? |
|---|---|---|
| `find_definition` | yes | yes — `_meta.source = "tree-sitter+kg"`, `found=true`, real `file_path`/`start_line` |
| `find_references` | yes | **no** — handler returns `_meta.not_yet_implemented: true` (stub, awaiting `P2-W7-F05`) |

Per `.claude/agents/effectiveness-evaluator.md` §"Tool-availability
checks", the probe is `tools/list`, and a tool is "operational" if
registered + responsive. `find_references` is registered and the call
returns a well-formed JSON-RPC `result` envelope (no transport error).
The scenario therefore runs (rather than being marked
`skipped_tool_not_ready`); the consequences of the stub for this
scenario are documented under "Advisory" below.

## Scenarios

| scenario | tools_present | tools_real | UCIL pass? | UCIL score | Baseline score | Δ weighted | verdict |
|---|---|---|---|---|---|---|---|
| nav-rust-symbol | find_definition, find_references | find_definition (real); find_references (stub — `_meta.not_yet_implemented: true`) | yes | 5.00 | 5.00 | 0.00 | PASS |

## Per-scenario detail

### nav-rust-symbol

**Fixture**: `tests/fixtures/rust-project` — a self-contained expression
parser/evaluator. `Cargo.lock` shows zero external dependencies;
`grep -rE "retry|backoff|exponential|http|reqwest|hyper"` across the
fixture returns zero matches. Ground truth answer: "no HTTP-retry
exponential-backoff functions exist".

**Setup**
- `/tmp/ucil-eval-nav-rust-symbol/ucil/` — fresh fixture copy for UCIL run
- `/tmp/ucil-eval-nav-rust-symbol/baseline/` — fresh fixture copy for baseline run
- Output sink: `/tmp/ucil-eval-out/nav-rust-symbol.md` (rotated per side)
- Identical task prompt for both sides, captured in `task.md`

**UCIL run**
- Transport: `ucil-daemon mcp --stdio --repo /tmp/ucil-eval-nav-rust-symbol/ucil`
- MCP config: `--mcp-config mcp-ucil.json --strict-mcp-config`
- Settings: `--setting-sources ""` (no project / user / local settings)
- Session: `f8c9a5a3-bad5-4222-95ba-4a295943067d` (fresh, `--no-session-persistence`)
- Model: `claude-opus-4-7`
- `duration_ms`: 86324 (≈ 86s)
- `num_turns`: 17
- `total_cost_usd`: 0.4226
- `usage.input_tokens`: 19
- `usage.cache_read_input_tokens`: 367 623
- `usage.cache_creation_input_tokens`: 18 049
- `usage.output_tokens`: 5 011
- Stop reason: `end_turn`
- Output: 65 lines, written to `/tmp/ucil-eval-out/nav-rust-symbol.md`

**Baseline run**
- Transport: no MCP servers (`mcp-empty.json` with empty `mcpServers`)
- MCP config: `--mcp-config mcp-empty.json --strict-mcp-config`
- Settings: `--setting-sources ""` (no project / user / local settings)
- Session: `798294aa-0936-47eb-9ee6-430f3020f564` (fresh, `--no-session-persistence`)
- Model: `claude-opus-4-7`
- `duration_ms`: 77333 (≈ 77s)
- `num_turns`: 13
- `total_cost_usd`: 0.4266
- `usage.input_tokens`: 18
- `usage.cache_read_input_tokens`: 339 389
- `usage.cache_creation_input_tokens`: 22 269
- `usage.output_tokens`: 4 680
- Stop reason: `end_turn`
- Output: 92 lines, written to `/tmp/ucil-eval-out/nav-rust-symbol.md`

**Acceptance checks** (run after copying each side's output to `/tmp/ucil-eval-out/nav-rust-symbol.md`)

| check | UCIL | Baseline |
|---|---|---|
| `test -f /tmp/ucil-eval-out/nav-rust-symbol.md` | PASS | PASS |
| `test $(wc -l < /tmp/ucil-eval-out/nav-rust-symbol.md) -ge 5` | PASS (65) | PASS (92) |
| `grep -qE "\.rs:[0-9]+" /tmp/ucil-eval-out/nav-rust-symbol.md` | PASS | PASS |

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
| formatting          | 0.5 | 5 | 5 |
| **weighted mean**   |     | **5.00** | **5.00** |

UCIL judge: "The solution correctly concludes 'none found' matching the
ground truth, and supports it with thorough negative evidence:
inspection of Cargo.toml/Cargo.lock confirming zero dependencies,
enumeration of every source file with its purpose, an extensive list
of HTTP/retry/backoff/timing patterns searched, and explicit handling
of plausible false positives (the `**` exponentiation operator's
`powf` calls, substring hits like 'surf' inside 'surface'). … No
fabricated functions or callers are introduced. The negative report is
well-structured with clear headings, an upfront verdict, and a
methodology section, which is appropriate given the H2-per-function
template doesn't apply."

Baseline judge: "The solution correctly identifies that no HTTP retry
with exponential backoff functions exist in the fixture, matching the
ground truth. The negative evidence is thorough and rigorous: it
enumerates all source files with line counts, confirms zero external
dependencies via Cargo.lock, lists all non-crate imports, and documents
an exhaustive case-insensitive search across HTTP/retry/backoff/timing-
related terms including hand-rolled patterns like `pow`, `<<`, and
`2^attempt`. … Since no functions exist, caller-completeness is
appropriately N/A and the negative reasoning explicitly establishes
there is nothing to find references for. No false positives are listed."

**Verdict: PASS** (UCIL ties baseline on every criterion; Δ weighted =
0.0, within the rubric's half-point tolerance; no FAIL conditions
triggered).

## Observations

- **Parity on correctness, slight cost-edge to baseline.** Both sides
  arrive at the correct negative conclusion with sufficient evidence.
  UCIL was ~1.12× slower (86s vs 77s) and used ~1.08× more
  cache-read tokens (367K vs 339K) — the MCP tool-schema overhead
  is real time when the task happens to be solvable by `Glob`/`Grep`/
  `Read` alone. For a positive-match task (real HTTP retry code to
  discover and cross-reference), the advantage shape would flip, but
  this fixture does not exercise that path.
- **`find_references` stub did not bite this scenario.** The ground-truth
  answer requires no `find_references` call (nothing to cross-reference
  when nothing exists). When `P2-W7-F05` lands, a follow-up run at that
  commit will exercise the positive-match path.
- **Both sides disambiguated the `**` exponentiation operator from
  exponential backoff.** The fixture's `powf` / `Pow` arms in
  `parser.rs`, `util.rs`, `eval_ctx.rs`, and `transform.rs` implement
  `aᵇ` for the toy expression language, not retry-delay computation.
  Both runs caught this and called it out explicitly.
- **Reproducibility note.** The CWD-outside-repo gotcha documented in
  the prior run (judge sessions hijacked by the repo `Stop` hook when
  CWD was inside the repo) was avoided here by `cd /tmp` + explicit
  `--setting-sources ""` on every judge invocation. Both judges
  returned clean JSON on first attempt.

## Advisory — path to a substantive WIN

This run, like the 2026-04-19 run, records a substantive tie. For UCIL
to post a substantive WIN on this scenario, two things must change:

1. `P2-W7-F05` (`find_references` real handler) graduates from stub to
   KG-backed lookup.
2. Either this scenario's fixture grows real HTTP-retry code, or a new
   phase-1-tagged scenario is added whose ground truth requires a
   multi-hop call-graph walk — i.e. where `find_references` provides
   capability beyond a single-file `grep` text-match.

Until then, phase-1 effectiveness should remain PASS on the strength of
UCIL matching the baseline — the gate contract does not require a WIN.

## Gate contract (why this run is PASS, not FAIL)

Per `.claude/agents/effectiveness-evaluator.md` §6 "Per-scenario verdict":

> **PASS**: `acceptance_checks` green AND `ucil_score >= baseline_score - 0.5` on every criterion.
> **WIN**: UCIL outperforms baseline by at least 1.0 on the weighted-average score.
> **FAIL**: `acceptance_checks` red on UCIL run, OR UCIL underperforms baseline by > 0.5 on any criterion.

UCIL scored 5/5 on every criterion; baseline scored 5/5. Δ = 0.0 ≥ -0.5
on every criterion → **PASS**. Not WIN (Δ weighted < 1.0). Not FAIL.

Per `.claude/agents/effectiveness-evaluator.md` §"Exit code":

> 0 if gate passes, 1 if any scenario FAIL, 2 on evaluator-internal error.

No FAIL recorded → **exit 0**.

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

Tempdirs are cleaned by the evaluator's `rm -rf /tmp/ucil-eval-*` step
on exit (per agent §"Exit cleanly"); the artefacts above must be
collected before the cleanup if a future operator wants to inspect a
specific run by hand.
