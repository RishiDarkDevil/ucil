# Effectiveness Report — Phase 1

Run at: 2026-05-07T22:19:00Z
Commit: 4a609f25cec041c013ca65a792088373843865b7
Branch: feat/WO-0068-cross-group-executor-and-fusion
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
(`nav-rust-symbol`) ran end-to-end against `tests/fixtures/rust-project`.
All three acceptance checks were green on both sides, but UCIL
underperformed baseline on `caller_completeness` by exactly the −1
delta documented as the structural flake in
`ucil-build/escalations/20260507T1930Z-effectiveness-nav-rust-symbol-doctest-caller-flake.md`.
LLM judges scored UCIL **5/4/5/5** and baseline **5/5/5/5** across
`correctness / caller_completeness / precision / formatting`. Weighted
means: UCIL **4.6923**, baseline **5.0000**, **Δ weighted = −0.3077**.
The −0.5 floor is breached on `caller_completeness` (UCIL − Baseline =
−1).

Per `.claude/agents/effectiveness-evaluator.md` §6 "Per-scenario verdict":
- **PASS**: acceptance_checks green AND `ucil_score >= baseline_score − 0.5` on every criterion. ✗ (caller_completeness UCIL 4 < baseline 5 − 0.5 = 4.5)
- **WIN**: UCIL outperforms baseline by ≥ 1.0 on weighted-average. ✗ (Δ = −0.3077)
- **FAIL**: acceptance red on UCIL OR UCIL underperforms baseline by > 0.5 on any criterion. ✓ (caller_completeness Δ = −1, > 0.5)

→ Verdict for `nav-rust-symbol`: **FAIL**.

This is the second observation of this exact flake direction (UCIL
omits the rustdoc doctest caller at `src/http_client.rs:26`, baseline
catches it). The cross-run table now reads:

| run | commit | UCIL caller_completeness | Baseline caller_completeness | Δ | verdict |
|---|---|---|---|---|---|
| 2026-05-08T00:55Z | fc50ef0 | 5 | 4 | +1 | PASS (WIN) |
| 2026-05-07T19:34Z | 68e505f | 4 | 5 | −1 | FAIL |
| 2026-05-07T21:36Z | 5d7614b | 5 | 5 | 0 | PASS (tie) |
| this run (2026-05-07T22:19Z) | 4a609f2 | **4** | **5** | **−1** | **FAIL** |

Four runs, four different criterion-level outcomes. The agent's choice of
which tools to call — and whether to enumerate the doctest caller at
`:26` as a "real" caller — varies between runs and produces the
observed flake. Per the deferred-resolution note in
`20260507T1930Z-effectiveness-nav-rust-symbol-doctest-caller-flake.md`,
the structural fix lands as P3-W9 forward work (real `find_references`
MCP handler that returns a deterministic 5-caller set on this scenario).
The flake remains live until that work ships.

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
the scenario runs (rather than `skipped_tool_not_ready`).

A live MCP-connection check via the actual `claude -p` startup was
also performed: the system-init event reports `mcp_servers: [{name:
"ucil", status: "connected"}]`, confirming the daemon was reachable
throughout the UCIL run.

## Scenarios

| scenario | tools_present | tools_real | UCIL acceptance | UCIL weighted | Baseline weighted | Δ weighted | per-criterion deltas (U−B) | verdict |
|---|---|---|---|---|---|---|---|---|
| nav-rust-symbol | find_definition, find_references | find_definition (real); find_references (stub) | 3/3 PASS | 4.6923 | 5.0000 | −0.3077 | corr 0, caller_completeness −1, prec 0, fmt 0 | **FAIL** |

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
that drives `retry_with_backoff`. Its callers are
`src/main.rs:24` (production) and `src/http_client.rs:124` (the unit
test `fetch_startup_banner_succeeds_via_retry`). Both readings of
"function performing HTTP retry with exponential backoff" — canonical
helper only, vs. helper + wrapper — are admissible per the task
wording. Both sides this run elected the inclusive (helper + wrapper)
reading.

**Setup**

- `/tmp/ucil-eval-nav-rust-symbol/ucil/` — fresh fixture copy for UCIL run
- `/tmp/ucil-eval-nav-rust-symbol/baseline/` — fresh fixture copy for baseline run
- Output sink: `/tmp/ucil-eval-out/nav-rust-symbol.md` (rotated per side; copies preserved at `ucil-output.md` and `baseline-output.md`)
- Identical task prompt for both sides at `task.md`

**UCIL run**

- Transport: `target/debug/ucil-daemon mcp --stdio --repo /tmp/ucil-eval-nav-rust-symbol/ucil`
- MCP config: `--mcp-config mcp-ucil.json --strict-mcp-config`
- Settings: `--setting-sources ""` (no project / user / local settings)
- Session: `cf88d481-b52a-4038-9f46-ee9336687a02` (fresh UUID)
- Model: `claude-opus-4-7`
- `duration_ms`: 81 729 (≈ 82 s)
- `num_turns`: 14
- `total_cost_usd`: 0.3279
- `usage.input_tokens`: 16
- `usage.cache_read_input_tokens`: 309 219
- `usage.cache_creation_input_tokens`: 10 069
- `usage.output_tokens`: 4 387
- `subtype`: `success` (`is_error: false`)
- Output: 29 lines, written to `/tmp/ucil-eval-out/nav-rust-symbol.md`,
  preserved at `ucil-output.md`
- Tool calls observed in stream: `Read` ×10, `Grep` ×8, `Bash` ×6,
  `Write` ×2. **Zero `mcp__ucil__*` tool calls** — the agent saw all 22
  UCIL tools exposed (system-init enumerates them) but elected to use
  built-in Read/Grep/Bash for the entire run. This is consistent with
  the structural caller-completeness flake: when the agent skips
  `find_definition` / `search_code` / `understand_code` and falls
  through to grep+Read, the rustdoc-doctest call site at
  `src/http_client.rs:26` is decided by agent judgment alone (text
  search + interpretive call vs. tree-sitter call-graph). Whether the
  agent elects to count it as a caller is variance-driven, exactly the
  flake mode documented in `20260507T1930Z`.

**Baseline run**

- Transport: no MCP servers (`mcp-empty.json` with empty `mcpServers`)
- MCP config: `--mcp-config mcp-empty.json --strict-mcp-config`
- Settings: `--setting-sources ""` (no project / user / local settings)
- Session: `ce7d5ec6-5b3a-47f0-bea7-ece19d7aac8e` (fresh UUID)
- Model: `claude-opus-4-7`
- `duration_ms`: 50 841 (≈ 51 s)
- `num_turns`: 13
- `total_cost_usd`: 0.2539
- `usage.input_tokens`: 13
- `usage.cache_read_input_tokens`: 220 991
- `usage.cache_creation_input_tokens`: 8 823
- `usage.output_tokens`: 3 506
- `subtype`: `success` (`is_error: false`)
- Output: 33 lines, written to `/tmp/ucil-eval-out/nav-rust-symbol.md`,
  preserved at `baseline-output.md`
- Tool calls observed in stream: `Read` ×10, `Bash` ×8, `Grep` ×4,
  `Write` ×2.

**Acceptance checks** (run after copying each side's output to
`/tmp/ucil-eval-out/nav-rust-symbol.md`)

| check | UCIL | Baseline |
|---|---|---|
| `test -f /tmp/ucil-eval-out/nav-rust-symbol.md` | PASS | PASS |
| `test $(wc -l < ...) -ge 5` | PASS (29) | PASS (33) |
| `grep -qE "\.rs:[0-9]+" ...` | PASS | PASS |

**Caller enumeration (each side)**

UCIL listed for `retry_with_backoff`: `src/http_client.rs:64`, `:84`,
`:91`, `:110` (4 of 5 — *missed* the doctest at `:26`).

Baseline listed for `retry_with_backoff`: `src/http_client.rs:64`,
`:84`, `:91`, `:110`, `:26` (5 of 5 — caught the doctest).

For `fetch_startup_banner`, both sides listed `src/main.rs:24` and
`src/http_client.rs:124` — agreement (2 of 2).

→ Sides diverge on the doctest caller of `retry_with_backoff`. Baseline
caught it; UCIL did not. This is the −1 caller_completeness delta.

**Judge scoring** (fresh `claude -p` session per side, run from `/tmp`
with `--setting-sources ""`, `--strict-mcp-config` against an empty
server map, and integers-only scoring; rubric copied verbatim from the
scenario yaml; ground truth disclosed to the judge so it scores
correctness against the truth, not against the agent's own claims)

| criterion | weight | UCIL | Baseline | Δ (U−B) |
|---|---|---|---|---|
| correctness         | 3.0 | 5 | 5 |  0 |
| caller_completeness | 2.0 | 4 | 5 | **−1** |
| precision           | 1.0 | 5 | 5 |  0 |
| formatting          | 0.5 | 5 | 5 |  0 |
| **weighted mean**   |     | **4.6923** | **5.0000** | **−0.3077** |

UCIL judge (session `c5902388-dee4-499e-ac59-2a8d36664e3a`):
"Both qualifying functions are correctly identified at the right line
numbers, and the dual interpretation (helper + wrapper) is admissible
per the task. Precision is clean — no false positives. Formatting
matches the H2-per-function + bulleted callers spec exactly. The only
gap is in caller_completeness: the doctest call at
src/http_client.rs:26 (inside the /// rustdoc of retry_with_backoff,
exercised by cargo test --doc) is not listed, so 4 of 5 callers of
retry_with_backoff are captured; fetch_startup_banner's two callers
are both correct."

Baseline judge (session `a5e7cc7d-c479-4e44-bc67-cfb1a97c8ac7`):
"Both listed functions are valid under the admissible interpretation
noted in the ground truth: retry_with_backoff is the canonical helper
at src/http_client.rs:37, and fetch_startup_banner at
src/http_client.rs:62 is the delegating wrapper. All five callers of
retry_with_backoff (lines 26 doctest, 64, 84, 91, 110) are present and
accurate, and both callers of fetch_startup_banner (src/main.rs:24 and
src/http_client.rs:124) are listed correctly. There are no fabricated
callers or false-positive functions, and the output adheres to the
specified H2-per-function plus bulleted-caller-list structure with
appropriate fully-qualified paths and file:line citations."

**Verdict: FAIL.** All three UCIL acceptance checks GREEN. But
per-criterion deltas (UCIL − Baseline): correctness 0,
caller_completeness **−1**, precision 0, formatting 0. The −0.5 floor
is breached on `caller_completeness` (Δ = −1, |Δ| > 0.5). Weighted Δ =
−0.3077 (UCIL underperforms baseline; not a tie, not a WIN).

## Observations

- **UCIL MCP server reachable but unused this run.** The system-init
  stream event reports `mcp_servers: [{name: "ucil", status:
  "connected"}]`, so the daemon was up and registered. Yet the UCIL
  agent made zero `mcp__ucil__*` tool calls and answered the task
  using `Read` (×10), `Grep` (×8), `Bash` (×6), and `Write` (×2) —
  exactly the toolset the baseline used. The agent's choice not to
  invoke any UCIL tool is an effectiveness signal in itself: at
  Phase 1, with `find_references` still a stub and `find_definition`
  the only "real" graph-backed handler, the agent's heuristic prefers
  built-in Read/Grep — and when both sides answer with the same
  toolset, the comparison reduces to LLM stochasticity on the
  doctest-caller boundary case. This is consistent with the
  Phase-3-forward fix path documented in
  `20260507T1930Z-effectiveness-nav-rust-symbol-doctest-caller-flake.md`:
  once `find_references` returns a deterministic 5-caller set and
  the agent has a reason to call it (richer caller metadata, lower
  latency than re-grepping), both runs converge on caller-completeness
  5/5 and the flake closes.
- **Earlier "Connection closed" hallucination.** A pre-test run of
  `run-ucil.sh` produced a result string claiming the UCIL MCP
  server returned `Connection closed`. Direct stdio probes against
  the same daemon binary (with the full `initialize` →
  `notifications/initialized` → `tools/list` → `tools/call` sequence)
  succeeded immediately, and the live MCP-connection metadata in the
  re-run's system-init event confirms the server was up.
  The "Connection closed" string was an agent hallucination produced
  to justify falling back to grep+Read, not a real transport error.
  The retry of the run with stream-json output and the same
  `mcp-ucil.json` config showed `mcp_servers.ucil.status =
  "connected"` for the entire run.
- **Latent dispatcher behaviour on JSON-RPC notifications.** The
  daemon's `handle_line` returns a JSON-RPC error object for any
  unrecognised method, including notifications such as
  `notifications/initialized` (which by JSON-RPC 2.0 §4.1 must not
  receive a response). For the 22 phase-1 tool catalogue + the three
  recognised methods (`initialize`, `tools/list`, `tools/call`) this
  is irrelevant — claude does not seem to fail the connection on the
  spurious response in this run — but it is a lurking protocol-strict
  bug worth flagging. (Out of scope for the evaluator to fix.)
- **No regression in UCIL substantive accuracy on the recognized
  callers.** UCIL's correctness, precision, and formatting are all
  5/5 this run. The single −1 lives entirely in
  `caller_completeness` and is the doctest-caller flake that has now
  manifested twice (this run and 2026-05-07T19:34Z) out of four
  observed runs.
- **Cost shape this run** — UCIL: 82 s wall, $0.328 cost, 4 387
  output tokens, 14 turns; baseline: 51 s wall, $0.254 cost, 3 506
  output tokens, 13 turns. UCIL is ~1.6× slower and ~1.3× more
  expensive — much smaller delta than 2026-05-07T21:36Z's 2.5× spread,
  because both sides used the same toolset this run. UCIL's marginal
  extra cost reflects ingestion / startup overhead of the MCP server
  even when the agent never calls into it.

## Reproducibility

All artefacts of this run are preserved under
`/tmp/ucil-eval-nav-rust-symbol/`:

- `task.md` — agent task prompt (identical for both sides)
- `ucil-output.md`, `baseline-output.md` — raw agent outputs
- `ucil-run.json`, `baseline-run.json` — final `claude -p` JSON
  envelope (last line of the stream-json transcript)
- `ucil-run.jsonl`, `baseline-run.jsonl` — full `claude -p`
  stream-json transcripts (system-init, tool_use events, message
  blocks, final result)
- `ucil-run.stderr`, `baseline-run.stderr` — child-process stderr
- `mcp-ucil.json`, `mcp-empty.json` — MCP configs for UCIL and baseline
- `run-ucil.sh`, `run-baseline.sh` — exact `claude` invocation
  templates (the actual run used inline session IDs and stream-json
  output to capture tool counts)
- `judge-ucil-prompt.md` (`/tmp/ucil-eval-judge-nav-rust-symbol-ucil.md`),
  `judge-baseline-prompt.md`
  (`/tmp/ucil-eval-judge-nav-rust-symbol-baseline.md`) — verbatim
  judge prompts
- `judge-ucil-raw.json`, `judge-baseline-raw.json` — judge `claude
  -p` JSON envelopes
- `judge-ucil.json`, `judge-baseline.json` — extracted scoring JSON
- `ucil-session-id`, `baseline-session-id`, `judge-ucil-session-id`,
  `judge-baseline-session-id` — session UUIDs
  (UCIL `cf88d481-b52a-4038-9f46-ee9336687a02`,
  baseline `ce7d5ec6-5b3a-47f0-bea7-ece19d7aac8e`,
  judge-UCIL `c5902388-dee4-499e-ac59-2a8d36664e3a`,
  judge-baseline `a5e7cc7d-c479-4e44-bc67-cfb1a97c8ac7`)
- `acceptance/ucil.txt`, `acceptance/baseline.txt` — per-side
  acceptance check transcripts (3/3 PASS each)
- `fixture-checksum-ucil.txt`, `fixture-checksum-baseline.txt` —
  per-file SHA-256 of the fixture (10 files × 2 sides; bit-identical
  to source)

## Gate contract (why this run is FAIL)

Per `.claude/agents/effectiveness-evaluator.md` §6 "Per-scenario verdict":

> **PASS**: `acceptance_checks` green AND `ucil_score >= baseline_score - 0.5` on every criterion.
> **WIN**: UCIL outperforms baseline by at least 1.0 on the weighted-average score.
> **FAIL**: `acceptance_checks` red on UCIL run, OR UCIL underperforms baseline by > 0.5 on any criterion.

PASS does not hold:
- All three UCIL acceptance_checks GREEN. ✓
- Per-criterion deltas (UCIL − Baseline): correctness 0,
  caller_completeness **−1**, precision 0, formatting 0. The minimum
  criterion delta is −1; the −0.5 floor IS breached on
  `caller_completeness`. ✗

FAIL holds: UCIL underperforms baseline by 1 (> 0.5) on
`caller_completeness`.

Per `.claude/agents/effectiveness-evaluator.md` §"Exit code":

> 0 if gate passes, 1 if any scenario FAIL, 2 on evaluator-internal error.

One scenario FAIL → **exit 1**.

The structural fix path is unchanged: the open escalation
`ucil-build/escalations/20260507T1930Z-effectiveness-nav-rust-symbol-doctest-caller-flake.md`
documents the deferred resolution (P3-W9 forward work — wire real
`find_references` MCP handler so caller enumeration is deterministic
on this scenario regardless of agent variance).
