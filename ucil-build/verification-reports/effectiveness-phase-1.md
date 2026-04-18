# Effectiveness Report — Phase 1

Run at: 2026-04-18T21:58:15Z
Commit: f11ebfd8b9664c0c17cda64b1aeaeb6ba0c256c3
Evaluator: effectiveness-evaluator (fresh session, `claude-opus-4-7`)

## Summary

| metric | value |
|---|---|
| Scenarios discovered for phase 1 | 1 |
| Scenarios run | 0 |
| Scenarios skipped (tool_not_ready) | 1 |
| Scenarios PASS | 0 |
| Scenarios WIN | 0 |
| Scenarios FAIL | 0 |

**Gate verdict: PASS (vacuous)** — one scenario is tagged for phase 1
(`nav-rust-symbol`), and it auto-skips because one of its
`requires_tools` (`find_references`) is still a Phase-1 stub at this
commit. The gate contract (see §"Gate contract") permits this as a
vacuous pass. The §"Advisory" section documents what would make the
pass *substantive* rather than vacuous.

## Progress since the previous report (`effectiveness-phase-1.md` @ `97932e0`)

HEAD advanced `97932e0` → `f11ebfd` (17 commits). The notable
intervening landing is **WO-0041** (merge `898032f`), which resolves
**Advisory item #1** from all four prior reports: `ucil-daemon mcp
--stdio --repo <PATH>` now bootstraps a real `KnowledgeGraph` from the
supplied directory and routes `find_definition` (and the other
KG-backed tools) through the real handlers. Advisory items #2 (host
settings registration) and #3 (phase-1-only scenario) have **not**
landed in this window.

Specifically (re-verified at `f11ebfd`):

- `ucil-daemon mcp --stdio --repo <PATH>` walks the repo, ingests
  every supported source file through `IngestPipeline`, binds the KG
  to the server, and serves — see `crates/ucil-daemon/src/main.rs:140`.
- `find_definition` over stdio now returns **real** tree-sitter + KG
  data — see Probe 3 below (`simplify` resolves to
  `./src/transform.rs::simplify@78:5`, `source: tree-sitter+kg`).
- `find_references` still returns `_meta.not_yet_implemented: true`
  unconditionally; it's the Phase-2 feature `P2-W7-F05`
  (`passes: false`, `last_verified_by: null`).
- `.claude/settings.json` still has zero `ucil` / `ucil-mcp` entries
  under `mcpServers` (six servers registered: `context7`,
  `filesystem`, `github`, `memory`, `sequential-thinking`, `serena`).

## Scenario discovery

Scanned `tests/scenarios/*.yaml`; retained any scenario whose
`phases:` list contains `1`:

| Scenario file | phases | fixture | requires_tools |
|---|---|---|---|
| `nav-rust-symbol.yaml` | `[1,2,3,4,5,6,7,8]` | `rust-project` | `find_definition`, `find_references` |
| `refactor-rename-python.yaml` | `[2,3,4,5,6,7,8]` | — | phase 2+, not applicable |
| `add-feature-ts.yaml` | `[3,4,5,6,7,8]` | — | phase 3+, not applicable |
| `arch-query.yaml` | `[3,4,5,6,7,8]` | — | phase 3+, not applicable |

Only `nav-rust-symbol` is eligible for Phase 1.

## Tool-availability probe (per `.claude/agents/effectiveness-evaluator.md`)

### Probe 1 — `ucil-mcp` binary

```
command -v ucil-mcp                → MISSING
test -x ./target/debug/ucil-mcp    → MISSING
test -x ./target/release/ucil-mcp  → MISSING
test -x ./target/debug/ucil-daemon → exists
```

No standalone `ucil-mcp` binary. Per WO-0040 the equivalent entry
point is `ucil-daemon mcp --stdio`, which the evaluator contract
§"Tool-availability checks" explicitly permits.

### Probe 2 — stdio handshake via `ucil-daemon mcp --stdio --repo <fixture>`

Full transcript captured in `/tmp/ucil-probe-1.json`. Server stderr
(log line):

```
INFO ucil_daemon: ucil-daemon mcp --stdio bootstrap complete
repo=. discovered=7 ingested=7
```

`tools/list` enumerates the canonical 22 tools including both
`find_definition` and `find_references`.

### Probe 3 — `tools/call` responsiveness (CWD = `tests/fixtures/rust-project`)

```
id=2 → find_definition {"name":"simplify","reason":"probe"}
id=3 → find_references  {"name":"simplify","reason":"probe","repo":"."}
id=4 → search_code      {"query":"simplify","reason":"probe"}
```

Response summaries:

| id | tool | `_meta.not_yet_implemented` | notable fields | verdict |
|---|---|---|---|---|
| 2 | `find_definition` | **absent** | `source: tree-sitter+kg`, `file_path: ./src/transform.rs`, `start_line: 78`, `signature: pub fn simplify(expr: &Expr) -> Expr`, `qualified_name: ./src/transform.rs::simplify@78:5` | **operational** |
| 3 | `find_references`  | **true**   | `content[0].text: "tool \`find_references\` is registered but its handler is not yet implemented (Phase 1 stub)"` | **STUB — not ready** |
| 4 | `search_code`      | absent (`count: 41`, `source: tree-sitter+ripgrep`) | — | operational (not required by this scenario, but proves the KG + ripgrep integration is live over stdio) |

The delta vs. the `97932e0` report is decisive: `find_definition` has
moved from stub → operational over the same external surface
(`claude -p` spawning a child that calls UCIL via MCP stdio).
`find_references` is still stub; no progress in this window.

### Probe 4 — host-level MCP registration

```
jq '.mcpServers | keys' .claude/settings.json
  → ["context7", "filesystem", "github", "memory",
     "sequential-thinking", "serena"]
```

No `ucil` entry under `mcpServers` in `.claude/settings.json`.
Advisory item #2 is still open: a spawned `claude -p` session would
need UCIL registered under `mcpServers.ucil` to call UCIL tools by
name, independent of the stdio binary's responsiveness. For this
evaluator the restriction is moot because we probe the binary
directly (as Probe 3 demonstrates), but it is still a blocker for
genuine end-to-end scenario runs from inside a child Claude session.

### Probe 5 — in-process feature status

Per `ucil-build/feature-list.json` at `f11ebfd`:

| Tool | Feature ID | passes | last_verified_by |
|---|---|---|---|
| `find_definition` | `P1-W4-F05` | ✅ true | `verifier-9422e28c-64e9-4bc0-a26d-cea7533de34b` |
| `find_references` | `P2-W7-F05` | ❌ false | null (Phase 2) |

`find_definition` has a real handler (`handle_find_definition` in
`crates/ucil-daemon/src/server.rs`) and, post-WO-0041, that handler
is reachable over stdio when `--repo <PATH>` is supplied.

`find_references` is a Phase-2 feature; no implementation body in the
KG-routed allow-list. Even with a KG attached, a call falls through
to the stub.

### Conclusion

- `find_definition` — **operational over stdio** when the server is
  spawned with `--repo <PATH>`. First external-surface green in
  Phase 1.
- `find_references` — **stub**. Required by this scenario. Blocks
  the run.

The scenario's task ("find every function that performs HTTP retry
with exponential backoff … list every place it is CALLED FROM")
cannot be answered without a working `find_references` (the "every
place it is CALLED FROM" bullet is `find_references`'s core
contract). Per evaluator contract §"Tool-availability checks" —
*"If any is missing, `skipped_tool_not_ready`"* — this scenario is
`skipped_tool_not_ready`.

## Per-scenario verdict

### `nav-rust-symbol`

- **Status: `skipped_tool_not_ready`**
- **Reason:** `find_references` (`P2-W7-F05`) still returns
  `_meta.not_yet_implemented: true`; no real handler. `find_definition`
  is operational post-WO-0041 but by itself cannot answer the caller
  list.
- **Fixture sanity (side-info, not gate-relevant):**
  `grep -riE "retry|backoff|exponential|http"` over
  `tests/fixtures/rust-project` returns **0 matches**. The fixture is
  a small arithmetic-expression evaluator (5 846 LoC across 7 files:
  `parser.rs`, `transform.rs`, `util.rs`, `eval_ctx.rs`, `main.rs`,
  `lib.rs`, `integration_test.rs`) with no HTTP machinery. The
  scenario's canonical answer at this fixture is therefore "no such
  function exists" — which raises the bar on `find_references`'s
  precision: the agent must prove absence across caller graphs, not
  just hallucinate a plausible list.
- **Required work to unblock at Phase 1:**
  - Either pull `P2-W7-F05` (`find_references`) forward with an ADR
    + planner approval per `CLAUDE.md` rules — likely premature given
    W5/W6 scope — **or** author a new phase-1-only scenario whose
    `requires_tools` is a subset of `{find_definition, search_code,
    get_conventions, understand_code}` (all KG-routable at
    `f11ebfd`).
  - Register UCIL under `mcpServers.ucil` in `.claude/settings.json`
    if/when a scenario is added that drives UCIL from inside a spawned
    `claude -p` child (this evaluator's probe shortcut works without
    that, but a child-session-driven scenario would need it).
- **Fixture:** `tests/fixtures/rust-project/` — present; **not
  copied** to tempdirs because no run was attempted. `/tmp/ucil-eval-*`
  not created.
- **Acceptance checks:** not run (no UCIL output to check; running
  baseline alone would violate contract §"Hard rules" — *"If you omit
  the baseline, fail the run as baseline-missing"* — and the
  companion UCIL run is unrunnable).
- **Judge session:** not spawned (no outputs to judge).

## Gate contract

Per `scripts/verify/effectiveness-gate.sh` and the evaluator contract
in `.claude/agents/effectiveness-evaluator.md`:

> Exits 0 iff:
>   - At least one scenario tagged for this phase exists
>   - Every non-skipped scenario returns a PASS or WIN verdict

Applied here:

- 1 scenario tagged for phase 1 ✅
- 1 scenario skipped (`skipped_tool_not_ready`) — permissible per
  contract
- 0 non-skipped scenarios → "every non-skipped PASS/WIN" vacuously
  true ✅

**Gate verdict: PASS.**

## Advisory (non-gating)

This is the **fifth consecutive vacuous PASS** (`316109e` →
`8d8fc0c` → `5edc200` → `97932e0` → `f11ebfd`). One of the three
advisory items from prior reports has now shipped:

| # | Advisory | Status at `f11ebfd` |
|---|----------|---------------------|
| 1 | Attach a KG from the stdio entry point so `find_definition` becomes responsive over stdio | **✅ landed** — WO-0041 / `898032f`; Probe 3 id=2 is the proof point |
| 2 | Register UCIL under `mcpServers.ucil` in `.claude/settings.json` | ⏳ still open |
| 3 | Author a phase-1-only scenario exercising only Phase-1 tools | ⏳ still open |

Residual path to a **substantive** phase-1 effectiveness pass:

1. **Add a phase-1-only scenario** (#3 above). A scenario shaped like
   "given a symbol name, emit the fully-qualified definition file:line
   + the signature + a conventions summary + a structured search for
   sibling usages" would let UCIL answer with `find_definition` +
   `search_code` (+ optionally `get_conventions` / `understand_code`
   if they end up Phase-1 KG-routable) and let the baseline answer
   with `grep + Read`. That produces a real UCIL-vs-baseline score
   delta rather than a skip. The existing `nav-rust-symbol` stays
   phase-2+ because the scenario's task explicitly requires caller
   enumeration.

2. **Register UCIL in host settings** (#2 above). Only strictly
   required for scenarios that drive UCIL from inside a spawned
   child `claude -p` session; this evaluator's own probe uses the
   binary directly and works today.

The evaluator does not block the gate on items 2 or 3 — they are
carried as planner input. Recommend planner pick item 3 up before
phase-1 ships, so the eventual phase-1 ship has at least one
substantive effectiveness datapoint instead of five vacuous passes.

## Environment notes (for reproducibility)

- Repo root: `/home/rishidarkdevil/Desktop/ucil`
- Branch: `main`
- HEAD: `f11ebfd8b9664c0c17cda64b1aeaeb6ba0c256c3`
- Evaluator binary spawn: `./target/debug/ucil-daemon mcp --stdio
  --repo tests/fixtures/rust-project` (built by WO-0041 landing;
  no rebuild forced by this evaluator pass).
- Probe artifacts (transient; recreated on every gate run):
  - `/tmp/ucil-probe-1.json` — `tools/list` + `find_definition` +
    `find_references` + `search_code` transcript that drove the
    availability decision.
  - `/tmp/ucil-probe-2.json` — negative-space probe
    (`search_code("retry")`, `search_code("backoff")`; both
    `count: 0`).
  - `/tmp/ucil-mcp-stderr*.log` — server-side tracing.
- `/tmp/ucil-eval-*` tempdirs were **not** created (no runnable
  scenario). Confirmed absent at start and end of run.
- No judge sessions spawned.
- No fixture files modified (contract §"Hard rules").
- No source files modified (contract §"Hard rules").
- No scenario files modified (contract §"Hard rules").

## Exit code

`0` — gate passes per contract.
