# Effectiveness Report — Phase 1

Run at: 2026-05-07T19:34:58Z
Commit: 68e505f96475258ae9c9e264d9bb45e75c373612
Branch: feat/WO-0067-classifier-and-reason-parser
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

**Gate verdict: FAIL** — the single phase-1-eligible scenario
(`nav-rust-symbol`) was executed end-to-end on the augmented `rust-project`
fixture (per ADR DEC-0017). Both UCIL and baseline produced
substantively-correct answers and all three acceptance checks were green
on both sides. LLM judges scored UCIL **5/4/5/5** and baseline **5/5/5/5**
across `correctness / caller_completeness / precision / formatting`.
Weighted means: UCIL **4.6923**, baseline **5.0000**, **Δ weighted =
−0.3077**. UCIL underperforms baseline by **−1** on
`caller_completeness`, exceeding the strict-rubric −0.5 floor.

Per `.claude/agents/effectiveness-evaluator.md` §6 "Per-scenario verdict":
- **FAIL**: `acceptance_checks` red on UCIL run, OR UCIL underperforms
  baseline by > 0.5 on any criterion. **The second condition holds:** the
  caller_completeness delta is −1, which is strictly greater than the
  half-point tolerance.

This run is the **inversion** of the prior phase-1 effectiveness pass at
commit `fc50ef0` (2026-05-08T00:55Z), where UCIL beat baseline by +1 on
this same criterion (UCIL 5/5/5/5, baseline 5/4/5/5 → +0.3077 weighted →
PASS). Same scenario, same fixture, same model, same tools — the
single-criterion swap is driven by run-to-run agent stochasticity over
whether to enumerate the doctest call site (`src/http_client.rs:26`,
inside the `///` rustdoc on `retry_with_backoff`). A flake escalation has
been filed at
`ucil-build/escalations/20260507T1930Z-effectiveness-nav-rust-symbol-doctest-caller-flake.md`.

## Tool-availability probe

`tools/list` against `target/debug/ucil-daemon mcp --stdio --repo
/tmp/ucil-eval-tools-probe/repo` reported all 22 §3.2 tools registered.
The two tools required by the scenario (`find_definition`,
`find_references`) were both present.

| tool | listed? | tools/call returns real data? |
|---|---|---|
| `find_definition` | yes | yes — `_meta.source = "tree-sitter+kg"`, real `file_path`/`start_line` |
| `find_references` | yes | **no** — handler still returns `_meta.not_yet_implemented: true` (Phase-1 stub awaiting `P2-W7-F05` to ship the real fused-source caller list) |

Per `.claude/agents/effectiveness-evaluator.md` §"Tool-availability checks",
the probe is `tools/list`, and a tool is "operational" if registered +
responsive. `find_references` is registered and the call returns a
well-formed JSON-RPC `result` envelope (no transport error). The scenario
therefore runs (rather than being marked `skipped_tool_not_ready`); UCIL
falls back to `search_code` + `understand_code` to enumerate callers.
Whether the doctest caller (`src/http_client.rs:26`, inside a `///` rustdoc
block) gets enumerated depends on agent decisions, not on a deterministic
tool result — hence the cross-run volatility documented above.

## Scenarios

| scenario | tools_present | tools_real | UCIL acceptance | UCIL weighted | Baseline weighted | Δ weighted | per-criterion deltas (U−B) | verdict |
|---|---|---|---|---|---|---|---|---|
| nav-rust-symbol | find_definition, find_references | find_definition (real); find_references (stub) | 3/3 PASS | 4.6923 | 5.0000 | −0.3077 | corr 0, caller_completeness **−1**, prec 0, fmt 0 | **FAIL** |

## Per-scenario detail

### nav-rust-symbol

**Fixture**: `tests/fixtures/rust-project` (augmented per ADR DEC-0017
with `src/http_client.rs` containing real `retry_with_backoff` +
`fetch_startup_banner`, plus a call site at `src/main.rs:24`). Per-file
SHA-256 inventory at `/tmp/ucil-eval-nav-rust-symbol/fixture-checksum.txt`.

**Ground truth** (verified by reading the fixture file directly): exactly
one helper function directly implements the exponential-backoff retry
loop — `rust_project::http_client::retry_with_backoff` at
`src/http_client.rs:37`. Its callers are:

- `src/http_client.rs:26` — doctest example inside the rustdoc on `retry_with_backoff`
- `src/http_client.rs:64` — inside `fetch_startup_banner` body
- `src/http_client.rs:84` — unit test `retry_returns_ok_on_first_success`
- `src/http_client.rs:91` — unit test `retry_doubles_delay_and_eventually_succeeds`
- `src/http_client.rs:110` — unit test `retry_returns_last_error_when_max_attempts_reached`

Whether `fetch_startup_banner` (a delegating wrapper at line 62) itself
qualifies is interpretation-dependent; both readings are admissible per
the scenario task wording. Both sides on this run elected the broad
reading and listed it. Its callers are `src/main.rs:24` (the production
caller) and `src/http_client.rs:124` (the unit test).

**Setup**
- `/tmp/ucil-eval-nav-rust-symbol/ucil/` — fresh fixture copy for UCIL run
- `/tmp/ucil-eval-nav-rust-symbol/baseline/` — fresh fixture copy for baseline run
- Output sink: `/tmp/ucil-eval-out/nav-rust-symbol.md` (rotated per side)
- Identical task prompt for both sides, captured in `task.md`

**UCIL run**
- Transport: `target/debug/ucil-daemon mcp --stdio --repo /tmp/ucil-eval-nav-rust-symbol/ucil`
- MCP config: `--mcp-config mcp-ucil.json --strict-mcp-config`
- Settings: `--setting-sources ""` (no project / user / local settings)
- Session: `8924b3d7-e4c5-4639-a1c2-92aa9b9e3b0c` (fresh, deterministic UUID)
- Model: `claude-opus-4-7`
- `duration_ms`: 92 529 (≈ 93 s)
- `num_turns`: 13
- `total_cost_usd`: 0.4672
- `usage.input_tokens`: 18
- `usage.cache_read_input_tokens`: 366 155
- `usage.cache_creation_input_tokens`: 20 332
- `usage.output_tokens`: 6 254
- `is_error`: false (`success`)
- Output: 39 lines, written to `/tmp/ucil-eval-out/nav-rust-symbol.md`,
  preserved at `ucil-output.md`

**Baseline run**
- Transport: no MCP servers (`mcp-empty.json` with empty `mcpServers`)
- MCP config: `--mcp-config mcp-empty.json --strict-mcp-config`
- Settings: `--setting-sources ""` (no project / user / local settings)
- Session: `77169bd6-cb50-46bf-a93b-446abed34bce` (fresh, deterministic UUID)
- Model: `claude-opus-4-7`
- `duration_ms`: 90 253 (≈ 90 s)
- `num_turns`: 13
- `total_cost_usd`: 0.4461
- `usage.input_tokens`: 18
- `usage.cache_read_input_tokens`: 357 564
- `usage.cache_creation_input_tokens`: 18 789
- `usage.output_tokens`: 5 971
- `is_error`: false (`success`)
- Output: 38 lines, written to `/tmp/ucil-eval-out/nav-rust-symbol.md`,
  preserved at `baseline-output.md`

**Acceptance checks** (run after copying each side's output to `/tmp/ucil-eval-out/nav-rust-symbol.md`)

| check | UCIL | Baseline |
|---|---|---|
| `test -f /tmp/ucil-eval-out/nav-rust-symbol.md` | PASS | PASS |
| `test $(wc -l < ...) -ge 5` | PASS (39) | PASS (38) |
| `grep -qE "\.rs:[0-9]+" ...` | PASS | PASS |

The augmented fixture's positive ground truth cleanly drives both sides
to emit `.rs:LINE` citations naturally — the rs-line acceptance flake
documented in
`ucil-build/escalations/20260507T0357Z-effectiveness-nav-rust-symbol-rs-line-flake.md`
remains closed by ADR DEC-0017's fixture augmentation.

**Caller enumeration delta (the substantive difference)**

UCIL listed for `retry_with_backoff`: `src/http_client.rs:64`, `:84`,
`:91`, `:110` (4 callers — **missed `:26` doctest**).

Baseline listed for `retry_with_backoff`: `src/http_client.rs:26`, `:64`,
`:84`, `:91`, `:110` (5 callers — **caught `:26` doctest**).

Both sides listed the same set for `fetch_startup_banner`
(`src/main.rs:24`, `src/http_client.rs:124`) — agreement.

The single-row delta is the rustdoc doctest at `src/http_client.rs:26`,
which sits inside a `///` doc-comment block. UCIL — using `find_definition`
+ `search_code` + `understand_code` (since `find_references` is a Phase-1
stub) — chose not to enumerate the doctest match this run. Baseline,
relying on `grep -rn "retry_with_backoff"`, included it. The previous
phase-1 run had the inverse outcome (UCIL caught the doctest, baseline
missed it). Cross-run swap on this one criterion → noisy effectiveness
signal on this scenario × this fixture combination.

**Judge scoring** (fresh `claude -p` session per side, run from `/tmp` with
`--setting-sources ""` and `--strict-mcp-config` against an empty server
map; rubric copied verbatim from the scenario yaml; ground truth disclosed
to the judge so it can score correctness against the truth, not against
the agent's own claims)

| criterion | weight | UCIL | Baseline | Δ (U−B) |
|---|---|---|---|---|
| correctness         | 3.0 | 5 | 5 | 0 |
| caller_completeness | 2.0 | **4** | **5** | **−1** |
| precision           | 1.0 | 5 | 5 | 0 |
| formatting          | 0.5 | 5 | 5 | 0 |
| **weighted mean**   |     | **4.6923** | **5.0000** | **−0.3077** |

UCIL judge: "Caller_completeness loses one point because the doctest call
site at src/http_client.rs:26 inside the rustdoc on retry_with_backoff is
missing; the other four retry_with_backoff callers (64, 84, 91, 110) and
both fetch_startup_banner callers (main.rs:24 and http_client.rs:124) are
all present and accurate. Precision is full marks: no fabricated or stdlib
callers, and no non-exponential retry functions are included."

Baseline judge: "All 5 callers of retry_with_backoff are listed with exact
line numbers (26 doctest, 64 wrapper, 84/91/110 unit tests), matching the
ground truth precisely. Both callers of fetch_startup_banner are listed
correctly (src/main.rs:24 and src/http_client.rs:124). No false
positives, no fabricated callers, no stdlib confusion."

**Verdict: FAIL.** Acceptance checks all green on UCIL. UCIL underperforms
baseline by 1 (more than the 0.5 noise tolerance) on
`caller_completeness`. Per the rubric, this trips the FAIL condition. The
weighted-mean delta of −0.3077 is well inside the ±1.0 WIN/LOSS band, so
the substantive impact is small — but the criterion-level rule is
mechanical and applies.

## Observations

- **Inverted from the prior run.** At commit `fc50ef0`, UCIL caught the
  doctest at line 26 and baseline missed it; UCIL got 5/5 on
  `caller_completeness`, baseline 4/5 → PASS. This run, the catch flipped:
  baseline got 5/5, UCIL got 4/5 → FAIL. Same fixture, same prompt, same
  model, same tools. The only thing that changed is the agent's run-time
  decision about whether to surface the doctest caller. Two consecutive
  evaluations therefore disagree on the gate verdict because the relevant
  signal sits on the boundary of the ±0.5 tolerance and the only criterion
  in play has 1-point granularity (so any swap = boundary cross).
- **`find_references` Phase-1 stub is the structural root cause.** UCIL's
  caller enumeration depends on `search_code` (text-search) + agent
  judgment about which matches qualify as "calls". Once `P2-W7-F05`
  (find_references) ships a real fused-source caller list, both sides
  should converge to the same caller set deterministically (UCIL via the
  tool, baseline via `grep`). At that point the cross-run swap should
  disappear. P2-W7-F05 is `passes=true` in feature-list.json but the
  `ucil-daemon` MCP handler still answers `not_yet_implemented: true` —
  the integration-into-MCP work is what gates this scenario. (See
  fingerprint via the find_references probe below.)
- **`find_references` MCP handler not wired.** Confirmed via direct
  JSON-RPC probe: `tools/call find_references` returns
  `{"_meta":{"not_yet_implemented":true,"tool":"find_references"}}`. This
  is the same Phase-1 stub the prior report described. So long as this
  stub remains, UCIL must rely on agent-stochastic text-search-plus-judgment
  for caller enumeration, and the cross-run swap on this scenario will
  recur with ≥ ~50% probability per run.
- **No regression in UCIL substantive accuracy.** UCIL's caller list is
  4/5 correct; the missing entry is a doctest, not a production call site
  or a unit test. UCIL's correctness, precision, and formatting all
  remain 5/5. The signal "UCIL produces a substantively wrong answer" is
  *not* present here; the signal "UCIL omits one boundary-case caller
  some fraction of runs" *is*.
- **Cost shape this run** — UCIL: 92.5 s wall, $0.467 cost, 6 254 output
  tokens; baseline: 90.3 s wall, $0.446 cost, 5 971 output tokens. Near
  parity, in contrast to the prior run (where UCIL was ~2.16× slower and
  ~2.7× more expensive due to a max_turns fight). UCIL converged in 13
  turns this time (well under the 30-turn cap) — the agent recognised
  the answer earlier and didn't re-verify.

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
  (UCIL `8924b3d7-e4c5-4639-a1c2-92aa9b9e3b0c`,
  baseline `77169bd6-cb50-46bf-a93b-446abed34bce`,
  judge-UCIL `915ef84c-bcb8-4cb7-9a20-5882ec6f474d`,
  judge-baseline `b6e55a17-0e40-45fe-b96d-7b0dcb8aad20`)
- `acceptance/ucil.txt`, `acceptance/baseline.txt` — per-side acceptance
  check transcripts (3/3 PASS each)
- `fixture-checksum.txt` — per-file SHA-256 of the fixture (10 files × 2 sides)

## Gate contract (why this run is FAIL)

Per `.claude/agents/effectiveness-evaluator.md` §6 "Per-scenario verdict":

> **PASS**: `acceptance_checks` green AND `ucil_score >= baseline_score - 0.5` on every criterion.
> **WIN**: UCIL outperforms baseline by at least 1.0 on the weighted-average score.
> **FAIL**: `acceptance_checks` red on UCIL run, OR UCIL underperforms baseline by > 0.5 on any criterion.

PASS conditions do not both hold:
1. All three UCIL acceptance_checks GREEN. ✓
2. Per-criterion deltas (UCIL − Baseline): correctness 0, caller_completeness −1, precision 0, formatting 0.
   The minimum criterion delta is −1; the −0.5 floor is breached on caller_completeness. ✗

FAIL condition holds: UCIL underperforms baseline by > 0.5 on
`caller_completeness` (delta −1).

Per `.claude/agents/effectiveness-evaluator.md` §"Exit code":

> 0 if gate passes, 1 if any scenario FAIL, 2 on evaluator-internal error.

One scenario FAIL → **exit 1**.
