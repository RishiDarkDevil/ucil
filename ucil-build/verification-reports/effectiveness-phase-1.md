# Effectiveness Report — Phase 1

Run at: 2026-04-19T02:30:00Z
Commit: 8d8fc0cfd0af879b33227d6946e56f58ec99180b
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

**Gate verdict: PASS (vacuous)** — one scenario is tagged for phase 1
(`nav-rust-symbol`), and it auto-skips because its `requires_tools`
(`find_definition`, `find_references`) are not operationally reachable from an
external `claude -p` run at this commit. The gate contract (see §"Gate
contract") permits this as a vacuous pass. The §"Advisory" section documents
what would make the pass *substantive* rather than vacuous.

## Progress since the previous report (`effectiveness-phase-1.md` @ `316109e`)

1. **WO-0040 merged** (`070204a`, `5418513`, `7737d92`). `ucil-daemon mcp
   --stdio` is now a wired subcommand: `main.rs` dispatches on the first
   positional arg, routes `"mcp"` to `McpServer::new().serve(stdin, stdout)`,
   and routes tracing to `stderr` so `stdout` stays pristine for the JSON-RPC
   frames. The stdio handshake **works**:

   ```
   printf '%s\n%s\n' \
     '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{...}}' \
     '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' \
     | timeout 15 ./target/debug/ucil-daemon mcp --stdio
   ```
   → two valid JSON-RPC responses; `tools/list` enumerates the full 22-tool
   catalog from master-plan §3.2.

2. **Nothing else relevant changed** for the effectiveness gate. Specifically,
   the two blockers that held back an actual scenario run last time are still
   in force:

   - `mcpServers.ucil` is still absent from `.claude/settings.json` (6
     servers registered, none named `ucil` or `ucil-mcp`).
   - The stdio MCP server is constructed via `McpServer::new()` with no KG
     handle attached. The `handle_tools_call` dispatcher at
     `crates/ucil-daemon/src/server.rs:515-571` routes `find_definition`,
     `get_conventions`, `search_code`, `understand_code` to their real
     handlers **only when `self.kg.is_some()`**; otherwise it falls through
     to the `_meta.not_yet_implemented: true` stub envelope (phase-1
     invariant #9). `find_references` is not even in the KG-routed allow-list.

## Scenario discovery

Scanned `tests/scenarios/*.yaml`; retained any scenario whose `phases:` list
contains `1`:

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
```

No `ucil-mcp` binary exists. Per WO-0040 the equivalent entry point is now
`ucil-daemon mcp --stdio`, which the evaluator contract §"Tool-availability
checks" explicitly permits.

### Probe 2 — stdio handshake via `ucil-daemon mcp --stdio`

```
printf '%s\n%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize", ...}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' \
  | timeout 15 ./target/debug/ucil-daemon mcp --stdio
```

Response frame 1 (`initialize`) — valid:
```
{"id":1,"jsonrpc":"2.0","result":{"capabilities":{"tools":{"listChanged":false}},
 "protocolVersion":"2024-11-05","serverInfo":{"name":"ucil-daemon","version":"0.1.0"}}}
```

Response frame 2 (`tools/list`) — valid; enumerates 22 tools including
`find_definition` and `find_references`.

**Both required tools are registered.** The registration check passes.

### Probe 3 — `tools/call` responsiveness check

The evaluator must confirm tools are not merely registered but actually answer
the scenario's question. Direct probe of both required tools:

```
printf '%s\n%s\n%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize", ...}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"find_definition","arguments":{"symbol":"main","reason":"test"}}}' \
  '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"find_references","arguments":{"symbol":"main","reason":"test"}}}' \
  | timeout 15 ./target/debug/ucil-daemon mcp --stdio
```

`find_definition` response:
```json
{"id":2,"jsonrpc":"2.0","result":{
  "_meta":{"not_yet_implemented":true,"tool":"find_definition"},
  "content":[{"text":"tool `find_definition` is registered but its handler is not yet implemented (Phase 1 stub)","type":"text"}],
  "isError":false}}
```

`find_references` response:
```json
{"id":3,"jsonrpc":"2.0","result":{
  "_meta":{"not_yet_implemented":true,"tool":"find_references"},
  "content":[{"text":"tool `find_references` is registered but its handler is not yet implemented (Phase 1 stub)","type":"text"}],
  "isError":false}}
```

**Both return the Phase-1 stub envelope (`_meta.not_yet_implemented: true`).**

Root cause: `main.rs:17` constructs the server via `McpServer::new()` with no
KG handle. `handle_tools_call` at `server.rs:527-546` only dispatches to real
handlers when `self.kg.is_some()`. Since the daemon has no KG init path from
the stdio entry point (and no CLI flag to point it at a project), every call
falls through to the stub.

### Probe 4 — host-level MCP registration

```
grep -cE '"ucil(-mcp)?":'  .claude/settings.json → 0
```

No `ucil` entry under `mcpServers` in `.claude/settings.json`. Six MCP
servers are registered (filesystem, github, context7, memory,
sequential-thinking, serena); none expose UCIL's tool surface. A spawned
`claude -p` session — which is how the evaluator drives a scenario — has no
path to call `find_definition` or `find_references` by name even at the
process level, independent of the stub issue above.

### Probe 5 — in-process feature status

Per `ucil-build/feature-list.json`:

| Tool | Feature ID | passes | last_verified_by |
|---|---|---|---|
| `find_definition` | `P1-W4-F05` | ✅ true | `verifier-9422e28c-…` |
| `find_references` | `P2-W7-F05` | ❌ false | null (Phase 2) |

`find_definition` has a real handler (`handle_find_definition` at
`server.rs:601+`). It works when invoked in-process with a KG attached — the
crate's own integration tests exercise it. It does **not** answer over the
stdio entry point at this commit because `McpServer::new()` does not attach
a KG.

`find_references` is a Phase-2 feature; `passes: false`, no implementation
body. Even with a KG attached, it would fall through to the stub because it
is not in the KG-routed allow-list at `server.rs:527-546`.

### Conclusion

Both required tools are **registered but non-responsive** over the only
external interface available to the evaluator (`claude -p` spawning a child
session that calls UCIL via MCP):

1. `find_definition` — registered, has a real in-process handler, but
   returns the Phase-1 stub when called over stdio because the daemon has no
   KG attached and no registration in `mcpServers`.
2. `find_references` — registered, still Phase-2 (no implementation),
   returns the Phase-1 stub unconditionally.

The scenario's task ("find every function that performs HTTP retry with
exponential backoff … list every place it is CALLED FROM") cannot be
answered by stub responses. Per evaluator contract §"Tool-availability
checks" — *"If any is missing, `skipped_tool_not_ready`"* — this scenario is
`skipped_tool_not_ready`.

## Per-scenario verdict

### `nav-rust-symbol`

- **Status: `skipped_tool_not_ready`**
- **Reason (in rank order of blocking severity):**
  1. The stdio MCP server starts with no KG attached; both required tools
     therefore return `_meta.not_yet_implemented: true` stubs and cannot
     answer the scenario question.
  2. `find_references` (`P2-W7-F05`) has no real handler at any phase yet;
     it's Phase-2 scope.
  3. `mcpServers.ucil` is absent from `.claude/settings.json`, so a spawned
     `claude -p` child would not be able to address UCIL tools by name even
     if (1) and (2) were resolved.
- **Required work to unblock at Phase 1:**
  - Add a KG-init path from the stdio entry point (either a `--project
    <path>` flag on `ucil-daemon mcp --stdio` or lazy init on first
    `find_definition` call) so the real handler actually runs over stdio.
  - Register UCIL under `mcpServers.ucil` in `.claude/settings.json` so
    evaluator-spawned `claude -p` children can call UCIL tools by name.
  - Either pull `P2-W7-F05` (`find_references`) forward into Phase 1
    (requires ADR + planner approval per CLAUDE.md rules) or author a new
    phase-1-only scenario that only depends on `find_definition` +
    `search_code`, which both have real handlers today. The existing
    `nav-rust-symbol` explicitly depends on `find_references` per its yaml
    (`requires_tools: [find_definition, find_references]`) and cannot be
    split without editing it — and scenarios are not editable to make a
    gate pass (contract §"Hard rules").
- **Fixture:** `tests/fixtures/rust-project/` — present; **not copied** to
  tempdirs because no run was attempted. `/tmp/ucil-eval-*` not created.
- **Acceptance checks:** not run (no UCIL output to check; running baseline
  alone would violate contract §"Hard rules" — *"If you omit the baseline,
  fail the run as baseline-missing"* — and the companion UCIL run is
  unrunnable).
- **Judge session:** not spawned (no outputs to judge).

## Gate contract

Per `scripts/verify/effectiveness-gate.sh` and the evaluator contract in
`.claude/agents/effectiveness-evaluator.md`:

> Exits 0 iff:
>   - At least one scenario tagged for this phase exists
>   - Every non-skipped scenario returns a PASS or WIN verdict

Applied here:

- 1 scenario tagged for phase 1 ✅
- 1 scenario skipped (`skipped_tool_not_ready`) — permissible per contract
- 0 non-skipped scenarios → "every non-skipped PASS/WIN" vacuously true ✅

**Gate verdict: PASS.**

## Advisory (non-gating)

A phase-1 gate that passes with zero runnable effectiveness scenarios is a
**weak signal**. The spirit of the gate — prove UCIL helps an agent solve
real tasks better than grep+Read — is not yet demonstrated at Phase 1.
WO-0040 landed the stdio skeleton, which is real progress; however the
stub-only dispatch means the external surface still delivers no UCIL value.

Concrete paths to a substantive pass (none blocks this gate; each is a
candidate work-order):

1. **Attach a KG from the stdio entry point.** `main.rs:17` builds
   `McpServer::new()` and calls `serve()` immediately. A `--project
   <path>` CLI flag that triggers `McpServer::with_project(path)` (a new
   constructor that loads the tag cache and KG) would make the
   already-`passes:true` `find_definition` responsive over stdio without
   changing any invariant. Without this, `P1-W4-F05` is green-at-the-unit
   level but dark at the integration level.

2. **Register UCIL in the host settings.** One `mcpServers.ucil` entry in
   `.claude/settings.json` (command = `./target/debug/ucil-daemon mcp
   --stdio`) is the last wire required for evaluator-spawned `claude -p`
   children to see UCIL as a named server.

3. **Author a phase-1-only scenario exercising only the Phase-1 tools.**
   `find_definition`, `get_conventions`, `search_code`, `understand_code`
   are all KG-routable today. A scenario of the shape "given a symbol, emit
   its definition file:line plus a structured usages search and a
   conventions summary" would let the phase-1 gate produce a **real**
   UCIL-vs-baseline delta rather than a vacuous pass. The existing
   `nav-rust-symbol` stays phase-2+ because `find_references` is a hard
   dependency for its "every place it is CALLED FROM" requirement.

All three items were advisory in the previous report (`316109e`); none
landed between `316109e` and `8d8fc0c` apart from the stdio wiring itself
(WO-0040). The evaluator does not block the gate on them — they are carried
as planner input.

## Environment notes (for reproducibility)

- Repo root: `/home/rishidarkdevil/Desktop/ucil`
- Branch: `main`
- HEAD: `8d8fc0cfd0af879b33227d6946e56f58ec99180b`
- `target/debug/ucil-daemon` was present (built by earlier work-orders); no
  rebuild forced by this evaluator pass.
- `/tmp/ucil-eval-*` tempdirs were **not** created (no runnable scenario).
  `ls /tmp/ucil-eval-* → No such file or directory` confirmed at end of
  run.
- No judge sessions spawned.
- No fixture files modified (contract §"Hard rules").
- No source files modified (contract §"Hard rules").
- No scenario files modified (contract §"Hard rules").

## Exit code

`0` — gate passes per contract.
