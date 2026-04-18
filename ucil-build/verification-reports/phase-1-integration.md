# Phase 1 Integration Report

**Tester session**: itg-9f8747bf-3838-468e-a5b6-bd778247c07e
**Started at**:     2026-04-18T23:00:48Z
**Verified at**:    2026-04-18T23:01:42Z
**Phase**:          1 (Week 1, per `ucil-build/progress.json`)
**HEAD commit**:    66245340c4358d98c9cbf44a960f0400eea336e4
**Verdict**:        FAIL

## Summary

Phase-1 gate requires three live smoke scripts to pass (no mocks of
Serena, LSP, or the UCIL daemon). Two of the three pass; the third
fails with the same shape recorded in every prior phase-1 integration
pass — pyright LSP cannot be reached because no `pyright-langserver`
binary is on PATH and the script's `npx -y pyright` fallback invokes
the pyright CLI rather than the LSP server.

- `scripts/verify/e2e-mcp-smoke.sh` — **exit 0** (PASS, 368ms). The
  daemon binary builds (incremental cargo cache);
  `ucil-daemon mcp --stdio` answers both `initialize` and
  `tools/list`; all 22 frozen MCP tools advertise the four CEQP
  universal params.
- `scripts/verify/serena-live.sh` — **exit 0** (PASS, 3222ms). Serena
  v1.0.0 spawned via `uvx` and advertised 20 tools including the
  three required for G1 structural (`find_symbol`,
  `find_referencing_symbols`, `get_symbols_overview`).
- `scripts/verify/diagnostics-bridge.sh` — **exit 1** (FAIL, 16036ms).
  `pyright-langserver` not on PATH; the declared `npx -y pyright`
  fallback runs the CLI, not the LSP server, so no framed
  `textDocument/publishDiagnostics` ever arrives within the 15-second
  wait. Identical shape to the eight previous phase-1 integration
  reports (commits `855cdfa`, `f11ebfd`, `97932e0`, `5edc200`,
  `8d8fc0c`, `316109e`, `04d5130`, `341b815`); HEAD `6624534` (chore:
  manual escalation resolve + kill stuck triage subagent) did not
  change the script or install pyright.

Because one gate script fails, the overall verdict is **FAIL**.

## Services

Phase-1 scripts do not require Docker. `scripts/verify/serena-live.sh`
explicitly documents "No mocks, no docker — Phase 1 runs Serena
locally via uvx as declared in the plugin manifest (master-plan §13)."
No `docker/*-compose.yaml` files exist in the repository, consistent
with that design. A `docker ps` at the start of this run returned
`permission denied while trying to connect to the docker API at
unix:///var/run/docker.sock` — the session user is not in the `docker`
group on this host, so even if a compose file existed it could not be
stood up without sudo/group reconfiguration. Phase 1 is designed to
avoid that dependency entirely — services below run in-process / via
uvx / via npx. Docker-backed fixtures (Postgres/MySQL/Arc-Memory/DBHub)
become relevant only in Phase 3+ per
`.claude/agents/integration-tester.md`.

| Service             | Source / Image                                                              | Up time | Healthy | Notes                                                                                                                                       |
|---------------------|-----------------------------------------------------------------------------|---------|---------|---------------------------------------------------------------------------------------------------------------------------------------------|
| ucil-daemon (local) | `cargo build -p ucil-daemon --bin ucil-daemon` (incremental cache warm)     | <1s     | yes     | Binary builds and answers MCP `initialize` + `tools/list` over stdio; 22 tools with CEQP params on all.                                     |
| Serena (uvx)        | `uvx --from git+https://github.com/oraios/serena@v1.0.0 serena-mcp-server`  | ~3s     | yes     | MCP handshake OK; 20 tools advertised including `find_symbol`, `find_referencing_symbols`, `get_symbols_overview`.                          |
| pyright-langserver  | `npx -y pyright` fallback (no `pyright-langserver` on PATH)                 | ~16s    | no      | Process starts; never emits a framed `textDocument/publishDiagnostics` response to the LSP `didOpen` probe within the 15s wait (see §Failures). |

## Tests

| Suite                                    | Passed | Failed | Skipped | Duration | Exit |
|------------------------------------------|--------|--------|---------|----------|------|
| scripts/verify/e2e-mcp-smoke.sh          | 1      | 0      | 0       | 368ms    | 0    |
| scripts/verify/serena-live.sh            | 1      | 0      | 0       | 3222ms   | 0    |
| scripts/verify/diagnostics-bridge.sh     | 0      | 1      | 0       | 16036ms  | 1    |
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

### 1. `scripts/verify/e2e-mcp-smoke.sh` — exit 0 (368ms)

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

### 2. `scripts/verify/serena-live.sh` — exit 0 (3222ms)

```
[serena-live] spawning Serena via uvx (pinned v1.0.0)...
[serena-live] OK — Serena v1.0.0 alive, advertises 20 tools including find_symbol find_referencing_symbols get_symbols_overview.
```

Serena was spawned via
`uvx --from git+https://github.com/oraios/serena@v1.0.0 serena-mcp-server --context ide-assistant --project <cwd>`
and answered the MCP handshake plus a `tools/list` with 20 tools,
including the three required by G1 structural.

Full logs: `phase-1-integration-logs/serena-live.{stdout,stderr,rc,dur}`.

## Failures

### 1. `scripts/verify/diagnostics-bridge.sh` — exit 1 (16036ms)

`pyright-langserver` is not installed on PATH, so the script takes its
declared fallback `PYRIGHT=(npx -y pyright)`. The LSP probe sends
`initialize` → `initialized` → `textDocument/didOpen` of a file with a
deliberate type error and waits 15s for a framed
`textDocument/publishDiagnostics` reply. No such reply arrives; the
Python framed-message extractor yields an empty `out.jsonl` and the
script fails at the "no publishDiagnostics with a non-empty diagnostic
list" assertion.

Log tail (stderr):

```
[diagnostics-bridge] FAIL: no publishDiagnostics with a non-empty diagnostic list
-- messages received --
```

(The follow-up "messages received" line is blank because `out.jsonl`
is empty. Observation: `npx -y pyright` invokes the pyright CLI
entrypoint, which is not an LSP server. The `pyright-langserver`
binary is the LSP server; it is shipped by the same npm package but
as a separate bin. `npx -y pyright-langserver --stdio` would be the
LSP-capable invocation. Observation only — no source change
performed, per the integration-tester's read-only charter.)

This matches the failure recorded in the eight previous phase-1
integration reports; nothing between those runs and this one
addressed it. The immediate environmental options to close the gap
are:

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
possible on this host's current permissions), so no compose `down`
was needed. All three verification scripts clean up their own
tempdirs via `trap 'rm -rf "$TMP"' EXIT`.

## Artifacts

- `phase-1-integration-logs/e2e-mcp-smoke.{stdout,stderr,rc,dur}`
- `phase-1-integration-logs/serena-live.{stdout,stderr,rc,dur}`
- `phase-1-integration-logs/diagnostics-bridge.{stdout,stderr,rc,dur}`
- `phase-1-integration-logs/session.id`, `start.ts`, `verified_at.ts`
