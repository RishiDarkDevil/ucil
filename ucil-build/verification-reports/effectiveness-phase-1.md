# Effectiveness Report — Phase 1

Run at: 2026-04-18T20:23:52Z
Commit: d02cd0c5b94bc146c46b20141219c4eb7ca3b713
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

**Gate verdict: PASS (vacuous)** — at least one scenario is tagged for phase 1
and no non-skipped scenario returned FAIL. See §"Gate contract" below for why
vacuous-pass is the letter-of-the-contract outcome and §"Advisory" for why the
spirit of the contract is not yet served by what Phase 1 ships.

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

## Tool-availability probe

Per the evaluator contract (`.claude/agents/effectiveness-evaluator.md`
§"Tool-availability checks"), each scenario's `requires_tools` must be
registered AND responsive on the UCIL MCP stdio interface before the scenario
can be run.

### Probe 1 — `ucil-mcp` binary

```
command -v ucil-mcp           → MISSING
test -x ./target/debug/ucil-mcp → MISSING
```

No standalone `ucil-mcp` binary exists in this tree. The Phase-1 MCP server
lives in the `ucil-daemon` crate (per `ucil-build/phase-log/01-phase-1/CLAUDE.md`
feature `P1-W3-F07`). Fall back to probing `ucil-daemon mcp --stdio`.

### Probe 2 — `ucil-daemon mcp --stdio` JSON-RPC handshake

```
printf '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}\n' \
  | timeout 5 ./target/debug/ucil-daemon mcp --stdio

exit=0
stdout=<empty, 0 bytes>
```

The daemon exits cleanly but emits **zero response frames**. The binary does
not implement a stdio JSON-RPC loop; `crates/ucil-daemon/src/main.rs` is still
the Phase-0 skeleton:

```rust
// crates/ucil-daemon/src/main.rs
#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    tracing::info!(version = ucil_core::VERSION, "ucil-daemon starting");
    Ok(())
}
```

`crates/ucil-daemon/src/server.rs` DOES contain a full `McpServer::serve()`
implementation (tools/list returns all 22 frozen tool descriptors, CEQP
params on every tool) — but `main.rs` never calls it. This is the same gap
flagged by `scripts/verify/e2e-mcp-smoke.sh` (run inline for this report):

```
[e2e-mcp-smoke] FAIL: daemon produced no stdout responses

  This usually means McpServer::serve() has not yet been wired
  into ucil-daemon's main.rs as a subcommand. server.rs has
  McpServer::serve(reader, writer) but main.rs only calls
  tracing_subscriber::fmt::init() and exits.
```

### Probe 3 — Host MCP configuration

`/home/rishidarkdevil/Desktop/ucil/.claude/settings.json` registers these MCP
servers: `filesystem`, `github`, `context7`, `memory`, `sequential-thinking`,
`serena`. **No `ucil` entry.** Even if `ucil-daemon mcp --stdio` were
wired, a spawned `claude -p` session would not see UCIL tools in its MCP
surface without an explicit `mcpServers.ucil` block.

### Probe 4 — Library-level tool availability (sanity check)

For the record, the library-level tools ARE present and partially implemented:

| tool | in `tools/list` | handler |
|---|---|---|
| `find_definition` | yes (server.rs:236) | `McpServer::handle_find_definition` (server.rs:601) — real impl backed by KG |
| `find_references` | yes (server.rs:240) | no dedicated handler; falls through to stub returning `{_meta:{not_yet_implemented:true}}` |

This matches the feature ledger:

- `P1-W5-F02` (Serena wired into G1: find_symbol, find_references, go_to_definition) passes=true, but the
  claim is about Serena wiring into the G1 layer, not the MCP surface.
- `P2-W7-F05` ("find_references MCP tool returns all references ...") passes=**false** (Phase 2 feature).

So `find_references` at Phase 1 is a stub regardless of stdio wiring.

## Per-scenario verdict

### `nav-rust-symbol`

- **Status: `skipped_tool_not_ready`**
- **Reason:** `find_definition` + `find_references` are not externally reachable
  via MCP stdio at the current commit (daemon `main.rs` is Phase-0 skeleton),
  AND `find_references` is not yet implemented even at the in-process library
  level (Phase 2 feature `P2-W7-F05`).
- **Required work to unblock:** wire `McpServer::serve(stdin, stdout)` into
  `ucil-daemon`'s `mcp --stdio` subcommand (matches the existing
  `scripts/verify/e2e-mcp-smoke.sh` expectation at line 57), AND land
  `P2-W7-F05` so `find_references` returns real data. Both are downstream of
  Phase 1 Week 3 wiring.
- **Fixture:** `tests/fixtures/rust-project/` — verified present; not copied to
  tempdir because no run was attempted.
- **Acceptance checks:** not run (no UCIL output and no baseline output were
  produced; running baseline alone would produce an uncalibrated score per
  contract §"Hard rules": *"If you omit the baseline, fail the run as
  baseline-missing"* — and the companion UCIL run is unrunnable, so running
  baseline alone is not informative for the gate).

## Gate contract

Per `scripts/verify/effectiveness-gate.sh`:

> Exits 0 iff:
>   - At least one scenario tagged for this phase exists
>   - Every non-skipped scenario returns a PASS or WIN verdict

Applied here:

- 1 scenario tagged for phase 1 ✅
- 0 non-skipped scenarios → *every non-skipped PASS/WIN* is vacuously true ✅

**Gate verdict: PASS.**

## Advisory (non-gating)

A phase-1 gate that passes with zero runnable effectiveness scenarios is a
**weak signal**. The spirit of the effectiveness gate — *prove UCIL helps an
agent solve real tasks better than grep+Read* — is not demonstrated by this
report. Actions (none blocking this gate, all downstream work):

1. **Wire the stdio MCP subcommand** — `ucil-daemon mcp --stdio` must serve
   `McpServer::serve(stdin, stdout)` and exit cleanly on EOF. The gap is
   documented in-repo: `crates/ucil-daemon/src/main.rs` is a Phase-0 skeleton,
   but `McpServer::serve()` and `ProgressiveStartup::start()` already exist
   in the same crate. The wiring is ~10 lines.
2. **Register UCIL in the host settings** — add an `mcpServers.ucil` entry in
   `.claude/settings.json` (or a dedicated `.mcp.json`) so spawned
   `claude -p` sessions can address UCIL tools by name during evaluator runs.
3. **Add a phase-1-valid scenario that only needs `find_definition` + `search_code`** —
   both have real Phase-1 implementations (`handle_find_definition`, symbol
   search via tree-sitter). A scenario that does not require `find_references`
   would let the gate produce a real UCIL-vs-baseline comparison in Phase 1.
   `nav-rust-symbol` itself could be adjusted (separate work-order) to drop the
   `find_references` requirement, or a sibling scenario could be added for
   Phase 1 only.

These are advisory because the gate as specified passes vacuously; they are
the concrete path to a Phase-1 gate that passes with **substance**.

## Environment notes (for reproducibility)

- Repo root: `/home/rishidarkdevil/Desktop/ucil`
- Branch: `main`
- `target/debug/ucil-daemon` built (built from d02cd0c by the earlier smoke probe)
- `/tmp/ucil-eval-nav-rust-symbol/{ucil,baseline}` tempdirs were created but
  unused; cleaned up at end of run.
- No judge sessions were spawned (no outputs to judge).

## Exit code

`0` — gate passes per contract.
