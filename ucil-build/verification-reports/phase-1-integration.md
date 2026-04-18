# Phase 1 Integration Report

**Tester session**: itg-f2b7a03b-6825-431b-beb3-c71720327bf5
**Verified at**: 2026-04-18T21:09:41Z
**Phase**: 1 (Week 1, per `ucil-build/progress.json`)
**HEAD commit**: f15578b9efb10f928334d3d78d87cedab34c0633
**Verdict**: FAIL

## Summary

Phase-1 gate requires three live smoke scripts (no mocks of Serena, LSP,
or the UCIL daemon). Two of the three pass in this run; one still fails —
the same shape as the two previous integration reports:

- `scripts/verify/e2e-mcp-smoke.sh` — **exit 0** (PASS). Daemon binary
  builds from the warm cargo cache; `ucil-daemon mcp --stdio` returns
  `initialize` and `tools/list`; all 22 frozen MCP tools are advertised
  with the four CEQP universal params on every tool.
- `scripts/verify/serena-live.sh` — **exit 0** (PASS). Serena v1.0.0
  spawned via `uvx` in ~3 s and advertised 20 tools including the three
  required for G1 structural (`find_symbol`, `find_referencing_symbols`,
  `get_symbols_overview`).
- `scripts/verify/diagnostics-bridge.sh` — **exit 1** (FAIL, 16 s).
  pyright via the `npx -y pyright` fallback still emits no framed
  `textDocument/publishDiagnostics` response to the LSP `didOpen` probe
  within the 15-second wait window. Identical failure shape to reports
  dated 2026-04-18T20:29:02Z and 2026-04-18T20:58:44Z; nothing in
  HEAD (`f15578b`) addressed it.

Because one gate script fails, the overall verdict is **FAIL**.

## Services

Phase-1 scripts do not require Docker: `scripts/verify/serena-live.sh`
explicitly documents "No mocks, no docker — Phase 1 runs Serena locally
via uvx as declared in the plugin manifest (master-plan §13)." No
`docker/*-compose.yaml` files exist in the repository, consistent with
that design. A `docker ps` at the start of this run returned
`permission denied while trying to connect to the docker API at
unix:///var/run/docker.sock` — the session user is not in the `docker`
group on this host, so even if a compose file existed it could not be
stood up without sudo/group reconfiguration. Phase 1 is designed to
avoid that dependency entirely — services below run in-process / via
uvx / via npx. Docker-backed fixtures (Postgres/MySQL/Arc-Memory/DBHub)
become relevant only in Phase 3+ per `.claude/agents/integration-tester.md`.

| Service              | Source / Image                                                               | Up time | Healthy | Notes                                                                                                                                        |
|----------------------|------------------------------------------------------------------------------|---------|---------|----------------------------------------------------------------------------------------------------------------------------------------------|
| ucil-daemon (local)  | `cargo build -p ucil-daemon --bin ucil-daemon` (incremental cache warm)      | <1s     | yes     | Binary builds and answers MCP `initialize` + `tools/list` over stdio; 22 tools with CEQP params on all.                                      |
| Serena (uvx)         | `uvx --from git+https://github.com/oraios/serena@v1.0.0 serena-mcp-server`   | ~3s     | yes     | MCP handshake OK; 20 tools advertised including `find_symbol`, `find_referencing_symbols`, `get_symbols_overview`.                           |
| pyright-langserver   | `npx -y pyright` fallback (no `pyright-langserver` on PATH)                  | ~16s    | no      | Process starts; never emits a framed `textDocument/publishDiagnostics` response to the LSP `didOpen` probe within the 15s wait (see §Failures). |

## Tests

| Suite                                    | Passed | Failed | Skipped | Duration | Exit |
|------------------------------------------|--------|--------|---------|----------|------|
| scripts/verify/e2e-mcp-smoke.sh          | 1      | 0      | 0       | 0s       | 0    |
| scripts/verify/serena-live.sh            | 1      | 0      | 0       | 3s       | 0    |
| scripts/verify/diagnostics-bridge.sh     | 0      | 1      | 0       | 16s      | 1    |
| cargo nextest integration (deferred)     | —      | —      | —       | —        | —    |
| pnpm adapters integration (deferred)     | —      | —      | —       | —        | —    |
| pytest integration (deferred)            | —      | —      | —       | —        | —    |

Per-WO cargo / pnpm / pytest integration tiers are run by the verifier
subagent per work-order (see `WO-*.md` reports under
`ucil-build/verification-reports/`). This phase-integration pass is the
black-box wrapper that the three `scripts/verify/*.sh` entries cover
for the phase-1 gate — they are deliberately not re-run here to avoid
shadowing the gate's own invocation.

## Passes

### 1. `scripts/verify/e2e-mcp-smoke.sh` — exit 0 (<1s)

```
[e2e-mcp-smoke] building ucil-daemon...
[e2e-mcp-smoke] OK — 22 tools registered, CEQP params on all, daemon spoke MCP cleanly.
```

Daemon binary served from the incremental cargo cache (effectively no
rebuild); answered both `initialize` and `tools/list` over
`ucil-daemon mcp --stdio`; the 22 frozen tool names from master-plan §3
are all present and every tool advertises the four CEQP universal
params (`reason`, `current_task`, `files_in_context`, `token_budget`).

Full logs: `phase-1-integration-logs/e2e-mcp-smoke.{stdout,stderr,rc,dur}`.

### 2. `scripts/verify/serena-live.sh` — exit 0 (3s)

```
[serena-live] spawning Serena via uvx (pinned v1.0.0)...
[serena-live] OK — Serena v1.0.0 alive, advertises 20 tools including find_symbol find_referencing_symbols get_symbols_overview.
```

Serena was spawned via `uvx --from git+https://github.com/oraios/serena@v1.0.0 serena-mcp-server --context ide-assistant --project <cwd>` and answered the MCP handshake plus a `tools/list` with 20 tools, including the three required by G1 structural.

Full logs: `phase-1-integration-logs/serena-live.{stdout,stderr,rc,dur}`.

## Failures

### 1. `scripts/verify/diagnostics-bridge.sh` — exit 1 (16s)

pyright is not installed as `pyright-langserver` on PATH, so the script
takes the declared fallback `PYRIGHT=(npx -y pyright)`. The LSP probe
sends `initialize` → `initialized` → `textDocument/didOpen` of a file
with a deliberate type error and waits 15 s for a framed
`textDocument/publishDiagnostics` reply. No such reply arrives; the
Python framed-message extractor yields an empty `out.jsonl` and the
script fails at the "no publishDiagnostics with a non-empty diagnostic
list" assertion.

Log tail (stderr):

```
[diagnostics-bridge] FAIL: no publishDiagnostics with a non-empty diagnostic list
-- messages received --
```

(The follow-up "messages received" line is blank because `out.jsonl` is
empty — pyright invoked via `npx -y pyright` runs the CLI entrypoint,
which is not an LSP server. The `pyright-langserver` binary is the LSP
server; it is shipped by the same npm package but as a separate bin.
`npx -y pyright-langserver --stdio` is the LSP-capable invocation.
Observation only — no source change performed.)

This matches the failure recorded at 2026-04-18T20:29:02Z and
2026-04-18T20:58:44Z in the previous reports; nothing between those
runs and this one addressed it. The immediate environmental options
to close the gap are:

- install `pyright` globally on the host (`npm i -g pyright` places
  both `pyright` and `pyright-langserver` on PATH), **or**
- extend `scripts/verify/diagnostics-bridge.sh` to use
  `npx -y pyright-langserver --stdio` as the fallback command instead
  of `npx -y pyright` (ADR + script edit; outside this
  integration-tester's scope).

Either path is a follow-up for the executor/planner; this report only
observes.

Full logs: `phase-1-integration-logs/diagnostics-bridge.{stdout,stderr,rc,dur}`.

## Tear-down

No Docker services were started (none required for Phase 1 and none
possible on this host's current permissions), so no compose `down` was
needed. All three verification scripts clean up their own tempdirs via
`trap 'rm -rf "$TMP"' EXIT`.

## Artifacts

- `phase-1-integration-logs/e2e-mcp-smoke.{stdout,stderr,rc,dur}`
- `phase-1-integration-logs/serena-live.{stdout,stderr,rc,dur}`
- `phase-1-integration-logs/diagnostics-bridge.{stdout,stderr,rc,dur}`
- `phase-1-integration-logs/session.id`, `start.ts`, `verified_at.ts`
