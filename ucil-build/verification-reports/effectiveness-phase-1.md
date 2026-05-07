# Effectiveness Report — Phase 1

Run at: 2026-05-07T21:36:44Z
Commit: 5d7614b0877da392b5dd6801f93a24003ddb7ab7
Branch: feat/WO-0068-cross-group-executor-and-fusion
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
(`nav-rust-symbol`) ran end-to-end against the augmented `rust-project`
fixture (per ADR DEC-0017). Both UCIL and baseline produced
substantively-correct answers and all three acceptance checks were green
on both sides. LLM judges scored UCIL **5/5/5/5** and baseline **5/5/5/5**
across `correctness / caller_completeness / precision / formatting`.
Weighted means: UCIL **5.0000**, baseline **5.0000**, **Δ weighted = 0.0**.
No criterion underperforms the −0.5 floor. Per-criterion deltas all 0.

Per `.claude/agents/effectiveness-evaluator.md` §6 "Per-scenario verdict":
- **PASS**: acceptance_checks green AND `ucil_score >= baseline_score − 0.5` on every criterion. ✓
- **WIN**: UCIL outperforms baseline by ≥ 1.0 on weighted-average. ✗ (Δ = 0.0)
- **FAIL**: acceptance red on UCIL OR UCIL underperforms baseline by > 0.5 on any criterion. ✗

→ Verdict for `nav-rust-symbol`: **PASS** (tie, neither WIN nor LOSS).

This run resolves the cross-run boundary flake described in
`ucil-build/escalations/20260507T1930Z-effectiveness-nav-rust-symbol-doctest-caller-flake.md`
in the PASS direction: both UCIL and baseline elected the inclusive
reading and enumerated all five callers of `retry_with_backoff`,
including the rustdoc doctest at `src/http_client.rs:26`. The flake
remains structurally possible until the real `find_references` MCP
handler wiring lands (P3-W9 forward work — see "Observations" §) but did
not manifest in this run.

## Tool-availability probe

`tools/list` against `target/debug/ucil-daemon mcp --stdio --repo
/tmp/ucil-eval-probe-phase1-fixture/repo` reported all 22 §3.2 tools
registered. Both tools required by the scenario (`find_definition`,
`find_references`) were present.

| tool | listed? | tools/call returns real data? |
|---|---|---|
| `find_definition` | yes | yes — `_meta.source = "tree-sitter+kg"`, real `file_path` (`src/http_client.rs`) and `start_line` (37) for `retry_with_backoff` |
| `find_references` | yes | **no** — handler still returns `_meta.not_yet_implemented: true` (Phase-1 stub awaiting end-to-end wiring of `P2-W7-F05` into the daemon's MCP layer) |

Per `.claude/agents/effectiveness-evaluator.md` §"Tool-availability checks",
the probe is `tools/list`, and a tool is "operational" if it is registered
and responsive (returns a well-formed JSON-RPC `result` envelope).
`find_references` is registered, the call returns a structured response
with no transport error — so per the spec the tool is "operational" and
the scenario runs (rather than `skipped_tool_not_ready`). UCIL falls back
to `search_code` + `understand_code` for caller enumeration.

## Scenarios

| scenario | tools_present | tools_real | UCIL acceptance | UCIL weighted | Baseline weighted | Δ weighted | per-criterion deltas (U−B) | verdict |
|---|---|---|---|---|---|---|---|---|
| nav-rust-symbol | find_definition, find_references | find_definition (real); find_references (stub) | 3/3 PASS | 5.0000 | 5.0000 | 0.0 | corr 0, caller_completeness 0, prec 0, fmt 0 | **PASS** |

## Per-scenario detail

### nav-rust-symbol

**Fixture**: `tests/fixtures/rust-project` (augmented per ADR DEC-0017
with `src/http_client.rs` containing real `retry_with_backoff` +
`fetch_startup_banner`, plus a call site at `src/main.rs:24`). The
per-side SHA-256 inventory at
`/tmp/ucil-eval-nav-rust-symbol/fixture-checksum-{ucil,baseline}.txt`
confirms 10 files per side and bit-identical fixture payloads on both
sides.

**Ground truth** (verified by reading `src/http_client.rs` directly): one
helper function directly implements the exponential-backoff retry loop —
`rust_project::http_client::retry_with_backoff` at `src/http_client.rs:37`
(delay doubles at line 52, `delay = delay.checked_mul(2).unwrap_or(delay);`).
Its callers are:

- `src/http_client.rs:26` — doctest example inside the `///` rustdoc on
  `retry_with_backoff`
- `src/http_client.rs:64` — body of `fetch_startup_banner`
- `src/http_client.rs:84` — unit test `retry_returns_ok_on_first_success`
- `src/http_client.rs:91` — unit test
  `retry_doubles_delay_and_eventually_succeeds`
- `src/http_client.rs:110` — unit test
  `retry_returns_last_error_when_max_attempts_reached`

`fetch_startup_banner` at `src/http_client.rs:62` is a delegating wrapper
that drives `retry_with_backoff`. Whether to count it as a "second
function performing HTTP retry with exponential backoff" or as a thin
wrapper around the canonical helper is interpretation-dependent — both
readings are admissible per the scenario task wording. Both sides on this
run elected the inclusive reading and listed it. Its callers are
`src/main.rs:24` (production caller) and `src/http_client.rs:124` (the
unit test `fetch_startup_banner_succeeds_via_retry`).

**Setup**

- `/tmp/ucil-eval-nav-rust-symbol/ucil/` — fresh fixture copy for UCIL run
- `/tmp/ucil-eval-nav-rust-symbol/baseline/` — fresh fixture copy for baseline run
- Output sink: `/tmp/ucil-eval-out/nav-rust-symbol.md` (rotated per side; copies preserved at `ucil-output.md` and `baseline-output.md`)
- Identical task prompt for both sides at `task.md`

**UCIL run**

- Transport: `target/debug/ucil-daemon mcp --stdio --repo /tmp/ucil-eval-nav-rust-symbol/ucil`
- MCP config: `--mcp-config mcp-ucil.json --strict-mcp-config`
- Settings: `--setting-sources ""` (no project / user / local settings)
- Session: `1df1dfbb-818e-4bd4-a831-ec0cb376fed0` (fresh, deterministic UUID)
- Model: `claude-opus-4-7`
- `duration_ms`: 154 235 (≈ 154 s)
- `num_turns`: 27
- `total_cost_usd`: 0.8915
- `usage.input_tokens`: 37
- `usage.cache_read_input_tokens`: 650 517
- `usage.cache_creation_input_tokens`: 51 217
- `usage.output_tokens`: 9 814
- `subtype`: `success` (`is_error: false`)
- Output: 51 lines, written to `/tmp/ucil-eval-out/nav-rust-symbol.md`,
  preserved at `ucil-output.md`

**Baseline run**

- Transport: no MCP servers (`mcp-empty.json` with empty `mcpServers`)
- MCP config: `--mcp-config mcp-empty.json --strict-mcp-config`
- Settings: `--setting-sources ""` (no project / user / local settings)
- Session: `2ee29837-92e6-4640-a99b-04db83e3a238` (fresh, deterministic UUID)
- Model: `claude-opus-4-7`
- `duration_ms`: 60 394 (≈ 60 s)
- `num_turns`: 13
- `total_cost_usd`: 0.3497
- `usage.input_tokens`: 16
- `usage.cache_read_input_tokens`: 289 027
- `usage.cache_creation_input_tokens`: 16 446
- `usage.output_tokens`: 4 069
- `subtype`: `success` (`is_error: false`)
- Output: 31 lines, written to `/tmp/ucil-eval-out/nav-rust-symbol.md`,
  preserved at `baseline-output.md`

**Acceptance checks** (run after copying each side's output to
`/tmp/ucil-eval-out/nav-rust-symbol.md`)

| check | UCIL | Baseline |
|---|---|---|
| `test -f /tmp/ucil-eval-out/nav-rust-symbol.md` | PASS | PASS |
| `test $(wc -l < ...) -ge 5` | PASS (51) | PASS (31) |
| `grep -qE "\.rs:[0-9]+" ...` | PASS | PASS |

The augmented fixture's positive ground truth (real
`retry_with_backoff` + real callers) drives both sides to emit
`.rs:LINE` citations naturally. The rs-line acceptance flake documented
in `ucil-build/escalations/20260507T0357Z-effectiveness-nav-rust-symbol-rs-line-flake.md`
remains closed by ADR DEC-0017's fixture augmentation.

**Caller enumeration (both sides)**

UCIL listed for `retry_with_backoff`: `src/http_client.rs:26`, `:64`,
`:84`, `:91`, `:110` (5 callers — caught the doctest).

Baseline listed for `retry_with_backoff`: `src/http_client.rs:64`, `:84`,
`:91`, `:110`, `:26` (5 callers — caught the doctest, listed in a
different order).

For `fetch_startup_banner`, both sides listed `src/main.rs:24` and
`src/http_client.rs:124` — agreement.

→ Both sides converge on the same 5+2 caller set this run. The
caller-completeness criterion that flipped between earlier runs is
0-delta this run.

**Judge scoring** (fresh `claude -p` session per side, run from `/tmp`
with `--setting-sources ""`, `--strict-mcp-config` against an empty
server map, and integers-only scoring; rubric copied verbatim from the
scenario yaml; ground truth disclosed to the judge so it scores
correctness against the truth, not against the agent's own claims)

| criterion | weight | UCIL | Baseline | Δ (U−B) |
|---|---|---|---|---|
| correctness         | 3.0 | 5 | 5 | 0 |
| caller_completeness | 2.0 | 5 | 5 | 0 |
| precision           | 1.0 | 5 | 5 | 0 |
| formatting          | 0.5 | 5 | 5 | 0 |
| **weighted mean**   |     | **5.0000** | **5.0000** | **0.0** |

UCIL judge (session `e10f6455-065c-405b-ad83-5de881f91ac5`):
"The solution correctly identifies both admissible interpretations: the
canonical helper `retry_with_backoff` at src/http_client.rs:37 and the
delegating wrapper `fetch_startup_banner` at src/http_client.rs:62. All
five callers of `retry_with_backoff` (lines 26 doctest, 64 wrapper,
84/91/110 tests) and both callers of `fetch_startup_banner`
(src/main.rs:24, src/http_client.rs:124) match the ground truth exactly.
Backoff behavior is verified with the doubling line (52). No false
positives—test functions are correctly classified as call sites rather
than separate retry functions, and the import on src/main.rs:15 is
explicitly excluded as not a call site. Format follows the
H2-per-function with bulleted caller list structure precisely."

Baseline judge (session `479c0d57-73cb-41b9-b371-db03dd3e0b4e`):
"Solution correctly identifies the canonical `retry_with_backoff`
helper at src/http_client.rs:37 and admissibly includes the delegating
wrapper `fetch_startup_banner` at src/http_client.rs:62 with explicit
framing. All five callers of retry_with_backoff are listed at the
correct line numbers (26 doctest, 64 wrapper body, 84/91/110 unit
tests), and both callers of fetch_startup_banner are captured
(main.rs:24, http_client.rs:124). No fabricated or missed callers, no
false positives, and the output follows the H2-per-function +
bulleted-caller structure exactly."

**Verdict: PASS.** All three UCIL acceptance checks GREEN. Per-criterion
deltas (UCIL − Baseline): correctness 0, caller_completeness 0,
precision 0, formatting 0. The −0.5 floor is not breached on any
criterion. Weighted Δ = 0.0 (UCIL does not WIN, but is at strict parity
with baseline on this scenario).

## Observations

- **Cross-run flake quiescent this run.** The doctest-caller boundary
  flake documented in
  `20260507T1930Z-effectiveness-nav-rust-symbol-doctest-caller-flake.md`
  is structurally still possible (UCIL relies on
  `search_code`+`understand_code` for caller enumeration since
  `find_references` is a Phase-1 stub) but did not trip this run: both
  sides made the inclusive choice and listed `src/http_client.rs:26`.
  The cross-run table now shows three observations:
  | run | commit | UCIL caller_completeness | Baseline caller_completeness | Δ | verdict |
  |---|---|---|---|---|---|
  | 2026-05-08T00:55Z | fc50ef0 | 5 | 4 | +1 | PASS (WIN) |
  | 2026-05-07T19:34Z | 68e505f | 4 | 5 | −1 | FAIL |
  | this run (2026-05-07T21:36Z) | 5d7614b | **5** | **5** | **0** | **PASS (tie)** |
  Three runs, three different criterion-level outcomes — UCIL and
  baseline have each missed the doctest once and caught it twice. The
  underlying agent-decision stochasticity is real; it just landed on
  "agreement" this run.
- **`find_references` MCP handler still a stub** — confirmed via direct
  JSON-RPC probe against the daemon. So long as that handler returns
  `not_yet_implemented`, UCIL's caller enumeration on this scenario
  remains agent-driven (text-search + judgment), and the cross-run swap
  on the doctest-as-caller question can recur with non-trivial
  probability. Per the deferred-resolution note in the prior escalation,
  P3-W9 forward work should wire the real fused-source caller list end
  to end; once it does, both sides converge on a deterministic 5-caller
  set and this scenario stops flaking.
- **No regression in UCIL substantive accuracy.** UCIL's correctness,
  caller_completeness, precision, and formatting are all 5/5 this run.
  Acceptance checks 3/3 GREEN. The run-to-run signal "UCIL might omit
  one boundary-case caller some fraction of runs" is *not* a UCIL
  defect — it is a known consequence of the Phase-1 `find_references`
  stub and is on the Phase-3 work-trajectory.
- **Cost shape this run** — UCIL: 154 s wall, $0.891 cost, 9 814 output
  tokens, 27 turns; baseline: 60 s wall, $0.350 cost, 4 069 output
  tokens, 13 turns. UCIL is ~2.55× slower and ~2.55× more expensive
  this run. The extra UCIL cost reflects `tools/call`-driven exploration
  via `find_definition`, `search_code`, and `understand_code`; baseline
  goes straight to grep+Read with fewer turns. Effectiveness parity at
  higher cost is a known UCIL trade-off at Phase-1 (richer tool
  responses, more agent verification turns). Cost-effectiveness deltas
  are tracked separately and are not a gate criterion at this phase.
- **UCIL extra context.** UCIL's output explicitly notes that
  `src/main.rs:15` is an `use` import (not a call site), uses the lib
  name from `Cargo.toml`, and cites the doubling expression at
  `src/http_client.rs:52` as backoff confirmation — small but real
  signals that UCIL's tooling surfaces additional context the baseline's
  grep-driven path didn't enumerate. The judge weighted these signals
  inside the rubric criteria already; they don't flip a 5/5 to a 5/5
  with WIN, but they're worth noting for future rubric tightening.

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
  (UCIL `1df1dfbb-818e-4bd4-a831-ec0cb376fed0`,
  baseline `2ee29837-92e6-4640-a99b-04db83e3a238`,
  judge-UCIL `e10f6455-065c-405b-ad83-5de881f91ac5`,
  judge-baseline `479c0d57-73cb-41b9-b371-db03dd3e0b4e`)
- `acceptance/ucil.txt`, `acceptance/baseline.txt` — per-side
  acceptance check transcripts (3/3 PASS each)
- `fixture-checksum-ucil.txt`, `fixture-checksum-baseline.txt` — per-file
  SHA-256 of the fixture (10 files × 2 sides; bit-identical to source)

## Gate contract (why this run is PASS)

Per `.claude/agents/effectiveness-evaluator.md` §6 "Per-scenario verdict":

> **PASS**: `acceptance_checks` green AND `ucil_score >= baseline_score - 0.5` on every criterion.
> **WIN**: UCIL outperforms baseline by at least 1.0 on the weighted-average score.
> **FAIL**: `acceptance_checks` red on UCIL run, OR UCIL underperforms baseline by > 0.5 on any criterion.

PASS conditions both hold:
1. All three UCIL acceptance_checks GREEN. ✓
2. Per-criterion deltas (UCIL − Baseline): correctness 0,
   caller_completeness 0, precision 0, formatting 0. The minimum
   criterion delta is 0; the −0.5 floor is not breached on any
   criterion. ✓

WIN does not hold (Δ weighted = 0.0 < +1.0); the run is a tie, scored
PASS. FAIL does not hold.

Per `.claude/agents/effectiveness-evaluator.md` §"Exit code":

> 0 if gate passes, 1 if any scenario FAIL, 2 on evaluator-internal error.

Zero scenarios FAIL → **exit 0**.
