# Effectiveness Report — Phase 1

Run at: 2026-04-18T20:30:45Z
Commit: 316109ee3bdb5491fd0f9845991aab816a1b4779
Evaluator: effectiveness-evaluator (fresh session)

## Summary

| metric | value |
|---|---|
| Scenarios discovered for phase 1 | 1 |
| Scenarios run | 0 |
| Scenarios skipped (tool_not_ready) | 1 |
| Scenarios PASS | 0 |
| Scenarios WIN | 0 |
| Scenarios FAIL | 0 |

**Gate verdict: PASS (vacuous)** — one scenario is tagged for phase 1 and the
single eligible scenario auto-skipped because its `requires_tools` are not yet
operational. See §"Gate contract" below for why vacuous-pass is the
letter-of-the-contract outcome, and §"Advisory" for why the spirit of the
contract is not yet served by Phase 1 shippables.

## Scenario discovery

Scanned `tests/scenarios/*.yaml`; retained any scenario whose `phases:` list
contains `1`:

| Scenario file | phases | fixture | requires_tools |
|---|---|---|---|
| `nav-rust-symbol.yaml` | `[1,2,3,4,5,6,7,8]` | `rust-project` | `find_definition`, `find_references` |
| `refactor-rename-python.yaml` | `[2,3,4,5,6,7,8]` | — | — (phase 2+, not applicable) |
| `add-feature-ts.yaml` | `[3,4,5,6,7,8]` | — | — (phase 3+, not applicable) |
| `arch-query.yaml` | `[3,4,5,6,7,8]` | — | — (phase 3+, not applicable) |

Only `nav-rust-symbol` is eligible for Phase 1.

## Tool-availability probe (per `.claude/agents/effectiveness-evaluator.md`)

The evaluator contract §"Tool-availability checks" requires a live `tools/list`
probe against `ucil-mcp` over stdio before any scenario can be run.

### Probe 1 — `ucil-mcp` binary

```
command -v ucil-mcp                  → MISSING (exit 1)
test -x ./target/debug/ucil-mcp      → MISSING
test -x ./target/release/ucil-mcp    → MISSING
```

No `ucil-mcp` binary exists at the current commit. `crates/*/Cargo.toml` has
no `[[bin]] name = "ucil-mcp"` entry — only `ucil-daemon`, `mock-mcp-plugin`,
and `ucil` (CLI).

### Probe 2 — stdio handshake via `ucil-daemon mcp --stdio`

Per `scripts/verify/e2e-mcp-smoke.sh` (the canonical probe shape), the
fallback entry point is `ucil-daemon mcp --stdio`. Running it with a real
`initialize` + `tools/list` payload:

```
printf '%s\n%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":...}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' \
  | timeout 10 ./target/debug/ucil-daemon mcp --stdio

exit_code:      0
stdout bytes:   0      ← no JSON-RPC responses produced
stderr bytes:   0
```

The daemon exits cleanly but emits zero bytes — confirming `main.rs` is still
the Phase-0 skeleton that calls `tracing_subscriber::fmt::init()` and returns.
`McpServer::serve(stdin, stdout)` exists in `crates/ucil-daemon/src/server.rs`
but has not been wired into a subcommand.

The most recent `e2e-mcp-smoke` integration log
(`ucil-build/verification-reports/phase-1-integration-logs/e2e-mcp-smoke.log`)
reports the same failure with the same actionable remediation hint embedded.

### Probe 3 — host registration in `.claude/settings.json`

```
grep -cE '"ucil(-mcp)?":' .claude/settings.json  → 0
```

No `ucil` or `ucil-mcp` entry under `mcpServers`. Six MCP servers are
registered (filesystem, github, context7, memory, sequential-thinking,
serena) but none expose UCIL's tool surface. A spawned `claude -p` session —
which is how the evaluator drives a scenario — has no path to call
`find_definition` or `find_references` even at the process level.

### Probe 4 — in-process feature availability (for completeness)

Per `ucil-build/feature-list.json`:

| Tool | Feature ID | passes | last_verified_by |
|---|---|---|---|
| `find_definition` | `P1-W4-F05` | ✅ true | verifier-9422e28c-… |
| `find_references` | `P2-W7-F05` | ❌ false | null (Phase 2) |

`find_definition` has a real handler (`handle_find_definition` in
`server.rs:601+`) that dispatches against an in-memory KG. `find_references`
is not yet implemented — the `tools/call` dispatcher falls through to the
Phase-1 stub envelope with `_meta.not_yet_implemented: true` per the invariant
codified in `server.rs:548-570`.

### Conclusion

Both tools required by `nav-rust-symbol` are currently **not operational**
from the perspective of an external agent run via `claude -p`:

- `find_definition` works in-process but has no stdio entry point and is not
  registered as an MCP server in `.claude/settings.json`.
- `find_references` is a Phase-2 feature (`P2-W7-F05`) with `passes: false`
  and no implementation body beyond the stub.

## Per-scenario verdict

### `nav-rust-symbol`

- **Status: `skipped_tool_not_ready`**
- **Reason:**
  1. `ucil-mcp` binary does not exist; no alternate stdio-MCP entry point is
     wired into `ucil-daemon`, so neither tool is externally reachable.
  2. Even if stdio were wired, `find_references` is a Phase-2 feature
     (`P2-W7-F05`, `passes: false`) and would return the Phase-1 stub
     envelope, which cannot be used to answer the scenario's "every place it
     is CALLED FROM" question.
- **Required work to unblock at Phase 1:**
  - Land the stdio subcommand — `ucil-daemon mcp --stdio` must invoke
    `McpServer::serve(tokio::io::stdin(), tokio::io::stdout())` and exit
    cleanly on EOF. The wiring is ~10 lines as documented in
    `scripts/verify/e2e-mcp-smoke.sh:57`.
  - Register UCIL under `mcpServers.ucil` in `.claude/settings.json` so that
    evaluator-spawned `claude -p` children can call UCIL tools by name.
  - Either pull `P2-W7-F05` forward into Phase 1 (requires ADR + planner
    approval per CLAUDE.md rules) OR split the scenario so only
    `find_definition` is exercised at Phase 1 (requires a new scenario; the
    current `nav-rust-symbol` explicitly depends on `find_references`).
- **Fixture:** `tests/fixtures/rust-project/` — present; not copied to
  tempdirs because no run was attempted. `/tmp/ucil-eval-*` cleaned up per
  contract §"Exit cleanly".
- **Acceptance checks:** not run (no UCIL and no baseline output produced;
  running baseline alone without a UCIL companion would violate contract
  §"Hard rules": *"If you omit the baseline, fail the run as
  baseline-missing"* — and the companion UCIL run is unrunnable).
- **Judge session:** not spawned (no outputs to judge).

## Gate contract

Per `scripts/verify/effectiveness-gate.sh`:

> Exits 0 iff:
>   - At least one scenario tagged for this phase exists
>   - Every non-skipped scenario returns a PASS or WIN verdict

Applied here:

- 1 scenario tagged for phase 1 ✅
- 0 non-skipped scenarios → "every non-skipped PASS/WIN" is vacuously true ✅

**Gate verdict: PASS.**

## Advisory (non-gating)

A phase-1 gate that passes with zero runnable effectiveness scenarios is a
**weak signal**. The spirit of the effectiveness gate — *prove UCIL helps an
agent solve real tasks better than grep+Read* — is not demonstrated by this
report. Concrete paths to substance (none block this gate; all downstream
work-orders):

1. **Wire the stdio MCP subcommand.** `crates/ucil-daemon/src/main.rs` is
   still the Phase-0 skeleton; `McpServer::serve()` already exists in the
   same crate. A match on `args.nth(1)` routing `"mcp"` to
   `McpServer::serve(stdin, stdout)` is the minimum wiring that lets
   `e2e-mcp-smoke` pass AND gives the evaluator a real stdio endpoint.

2. **Register UCIL in the host settings.** Add an `mcpServers.ucil` entry in
   `.claude/settings.json` (command = the daemon binary + `mcp --stdio`) so
   spawned `claude -p` children can address UCIL tools by name during
   evaluator runs.

3. **Add a phase-1-only scenario that exercises `find_definition` +
   `search_code`.** Both have real Phase-1 implementations. A scenario of the
   shape "given a symbol, produce its definition file:line and a structured
   search of its usages" would let the Phase-1 gate produce a **real**
   UCIL-vs-baseline delta rather than a vacuous pass. The existing
   `nav-rust-symbol` stays phase-2+-only because `find_references` is a hard
   requirement for "every place it is CALLED FROM".

Previous report (`effectiveness-phase-1.md` @ commit d02cd0c) flagged the
same three items; none have landed since (no stdio wiring in
`crates/ucil-daemon/src/main.rs`, no `mcpServers.ucil` in
`.claude/settings.json`, no new phase-1-only scenario). All remain advisory
— the gate contract does not require them at Phase 1.

## Environment notes (for reproducibility)

- Repo root: `/home/rishidarkdevil/Desktop/ucil`
- Branch: `main`
- HEAD: `316109ee3bdb5491fd0f9845991aab816a1b4779`
- `target/debug/ucil-daemon` was already built (used by previous
  integration run); no rebuild forced by this evaluator pass.
- `/tmp/ucil-eval-nav-rust-symbol/{ucil,baseline}` tempdirs were *not*
  created this run (no runnable scenario).
- No judge sessions were spawned.

## Exit code

`0` — gate passes per contract.
