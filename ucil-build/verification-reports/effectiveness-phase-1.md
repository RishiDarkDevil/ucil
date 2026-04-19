# Effectiveness Report — Phase 1

Run at: 2026-04-19T07:45Z
Commit: 26dfeb23f82ea03b9d942220a849a4ccd98d6ec5
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
scenario (`nav-rust-symbol`) was executed end-to-end: UCIL and
baseline runs both completed, every `acceptance_check` was green on
both sides, and the LLM judge scored UCIL and baseline equally
(5.0/5.0 weighted-average each, Δ = 0.0 → PASS per rubric §6).

Earlier phase-1 effectiveness reports auto-skipped on the theory
that `find_references` is still a Phase-1 stub (feature
`P2-W7-F05`). This run chose to execute the scenario because (a) the
evaluator contract (`.claude/agents/effectiveness-evaluator.md` §
"Tool-availability checks") defines "registered and responsive" as
the gating test, and `find_references` currently satisfies both —
`tools/list` enumerates it, and a `tools/call` returns a
well-formed JSON-RPC result (with `_meta.not_yet_implemented: true`,
but no transport error); and (b) the scenario's ground-truth answer
is "no HTTP-retry-with-exponential-backoff functions exist in the
fixture", so the task is fully resolvable with
`find_definition` + `search_code` (both real at this commit) plus
fallback `Read`/`grep`. The stub is documented under §"Advisory"
below so a future executor can light up the substantive-win path
when `P2-W7-F05` graduates.

## Scenarios

| scenario | tools_present | tools_real | UCIL pass? | UCIL score | Baseline score | Δ weighted | verdict |
|---|---|---|---|---|---|---|---|
| nav-rust-symbol | find_definition, find_references | find_definition (real); find_references (stub — returns `_meta.not_yet_implemented: true`) | yes | 5.00 | 5.00 | 0.00 | PASS |

## Per-scenario detail

### nav-rust-symbol

**Fixture**: `tests/fixtures/rust-project` (expression parser/evaluator; no HTTP, no retry, no backoff code — ground-truth answer is "none found")

**Setup**
- `/tmp/ucil-eval-nav-rust-symbol/ucil/` — fixture copy for UCIL run
- `/tmp/ucil-eval-nav-rust-symbol/baseline/` — fixture copy for baseline run
- Output sink: `/tmp/ucil-eval-out/nav-rust-symbol.md`

**UCIL run**
- Transport: `ucil-daemon mcp --repo /tmp/ucil-eval-nav-rust-symbol/ucil` (stdio MCP), wired via `--mcp-config` + `--strict-mcp-config`
- Model: `claude-opus-4-7`
- Session: `41a8d2c5-1026-42f5-9f4c-b814adb0ce84` (fresh, no persistence)
- `duration_ms`: 50545 (≈ 50s)
- `num_turns`: 12
- `input_tokens`: 27 fresh + 303 954 cache-read + 21 078 cache-creation
- `output_tokens`: 2 765
- Stop reason: `end_turn`

**Baseline run**
- Transport: no MCP (`--mcp-config` pointed at an empty `mcpServers` object; `--strict-mcp-config`). Built-in `Bash`/`Read`/`Glob`/`Grep` only; no UCIL skills/hooks.
- Model: `claude-opus-4-7`
- Session: `f4626039-b8fd-455a-808d-fd87f443cceb` (fresh, no persistence)
- `duration_ms`: 31343 (≈ 31s)
- `num_turns`: 8
- `input_tokens`: 13 fresh + 189 031 cache-read + 11 610 cache-creation
- `output_tokens`: 1 979
- Stop reason: `end_turn`

**Acceptance checks** (executed by copying each agent's output into `/tmp/ucil-eval-out/nav-rust-symbol.md` and running the scenario's commands)

| check | UCIL | Baseline |
|---|---|---|
| `test -f /tmp/ucil-eval-out/nav-rust-symbol.md` | PASS | PASS |
| `test $(wc -l < ...) -ge 5` (UCIL=27 lines, baseline=25 lines) | PASS | PASS |
| `grep -qE "\.rs:[0-9]+" ...` | PASS | PASS |

**Judge scoring** (fresh `claude -p` session per side, CWD outside repo, `--setting-sources ""`, rubric reproduced verbatim from the scenario yaml)

| criterion | weight | UCIL | Baseline |
|---|---|---|---|
| correctness        | 3.0 | 5 | 5 |
| caller_completeness| 2.0 | 5 | 5 |
| precision          | 1.0 | 5 | 5 |
| formatting         | 0.5 | 5 | 5 |
| **weighted mean**  |     | **5.00** | **5.00** |

UCIL justification: "Correctly concludes none found; provides thorough evidence with file:line citations, lists files consulted and search queries run, no hallucinations."

Baseline justification: "Correctly identifies no HTTP retry functions exist. Provides thorough evidence with file:line citations, search queries run, and clear structured negative report."

**Verdict: PASS** (UCIL ties baseline on every criterion; Δ = 0.0, within the rubric's half-point tolerance).

## Observations

- **Parity on correctness, not speed.** Both agents correctly inferred from `Cargo.toml` that the fixture is a pure expression evaluator with no HTTP/retry crates, and cited file-level evidence. UCIL took ~1.6× longer (50s vs 31s) and consumed ~1.6× more cache-read tokens (~304K vs ~189K) — the MCP tool-schema overhead costs real time when the task happens to be solvable by grep alone. For a positive-match task (real HTTP retry code to discover), the advantage shape would flip, but this fixture does not exercise that path.
- **UCIL's `search_code` did not surface false positives.** Both UCIL calls to `search_code` ("exponential backoff retry http", "retry backoff") returned zero matches, consistent with the ground truth.
- **`find_references` stub did not bite this scenario.** The ground-truth answer requires no `find_references` call (nothing to cross-reference when nothing exists). When `P2-W7-F05` lands, a follow-up run at that commit will exercise the positive-match path.
- **Judge hook-interference gotcha.** The first UCIL judge session was hijacked by the repository's `Stop` hook (wip-commit watchdog) because CWD was inside the repo — the judge returned `"Committed and pushed as wip: carryover (26dfeb2). Working tree clean."` instead of JSON. Rerunning with `cd /tmp` + `--setting-sources ""` produced clean JSON. Future evaluator runs should always judge from a cwd outside the UCIL repo root. (This did not corrupt any scoring — only the UCIL judge needed a rerun; baseline judge was already clean because it ran when the working tree was already stable.)

## Advisory — path to a substantive WIN

This run records a substantive tie. For UCIL to post a substantive WIN on this scenario, two things must change:

1. `P2-W7-F05` (`find_references` real handler) graduates from stub to KG-backed lookup.
2. Either this scenario's fixture grows real HTTP-retry code, or a new phase-1-tagged scenario is added whose ground truth requires a multi-hop call-graph walk (where `find_references` provides > grep's single-file-string-match capability).

Until then, phase-1 effectiveness should remain PASS on the strength of UCIL matching the baseline — the gate contract does not require a WIN.

## Gate contract (why this run is PASS, not FAIL)

Per `.claude/agents/effectiveness-evaluator.md` §6 "Per-scenario verdict":

> **PASS**: `acceptance_checks` green AND `ucil_score >= baseline_score - 0.5` on every criterion.
> **WIN**: UCIL outperforms baseline by at least 1.0 on the weighted-average score.
> **FAIL**: `acceptance_checks` red on UCIL run, OR UCIL underperforms baseline by > 0.5 on any criterion.

UCIL scored 5/5 on every criterion; baseline scored 5/5. Δ = 0.0 ≥ -0.5 on every criterion → PASS. Not WIN (Δ weighted < 1.0). Not FAIL.

Per `.claude/agents/effectiveness-evaluator.md` §"Exit code":

> 0 if gate passes, 1 if any scenario FAIL, 2 on evaluator-internal error.

No FAIL recorded → **exit 0**.

## Reproducibility

All artefacts of this run are preserved under `/tmp/ucil-eval-nav-rust-symbol/`:
- `ucil-output.md`, `baseline-output.md` — raw agent outputs
- `ucil-run.json`, `baseline-run.json` — `claude -p` JSON envelopes (duration, tokens, session ids)
- `judge-ucil.json`, `judge-baseline.json` — LLM judge scoring envelopes
- `judge-ucil-prompt.txt`, `judge-baseline-prompt.txt` — exact prompts fed to the judge
- `mcp-config.json`, `empty-mcp.json` — MCP configs for UCIL and baseline
- `task-ucil.md`, `task-baseline.md` — agent task prompts
- `probe.out` — `tools/list` probe confirming all 22 tools registered
