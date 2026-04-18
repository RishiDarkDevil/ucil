# Phase 1 Integration Report

**Tester session**: itg-fed2e5de-fdad-42e7-8215-46535d3668f1
**Verified at**: 2026-04-18T20:29:02Z
**Phase**: 1 (Week 1, per `ucil-build/progress.json`)
**HEAD commit**: 571789a
**Verdict**: FAIL

## Summary

Phase-1 gate requires three live smoke scripts (no mocks of Serena, LSP,
or the UCIL daemon). Two of the three failed:

- `scripts/verify/e2e-mcp-smoke.sh` — exit 1 (daemon produces no MCP
  responses: `mcp --stdio` subcommand still not wired in
  `crates/ucil-daemon/src/main.rs`).
- `scripts/verify/diagnostics-bridge.sh` — exit 1 (pyright via `npx`
  fallback path emits no framed `publishDiagnostics`).
- `scripts/verify/serena-live.sh` — exit 0 (Serena v1.0.0 up, 20 tools
  advertised).

The failure shape is the same as the prior integration pass at
2026-04-18T20:17:12Z (previous report). No source or script changes
between the two runs closed either gap.

## Services

Phase-1 scripts do not require Docker: `scripts/verify/serena-live.sh`
explicitly documents "No mocks, no docker — Phase 1 runs Serena locally
via uvx as declared in the plugin manifest (master-plan §13)." No
`docker/*-compose.yaml` files exist in the repository, consistent with
that design. A `docker ps` at the start of this run returned
`permission denied while trying to connect to the docker API at
unix:///var/run/docker.sock`: the session user is not in the `docker`
group on this host, so even if a compose file existed it could not be
stood up without a sudo/docker-group reconfiguration. Phase 1 is
designed to avoid that dependency entirely — services below run
in-process / via uvx / via npx.

| Service              | Source / Image                                                               | Up time | Healthy | Notes                                                                                                                                      |
|----------------------|------------------------------------------------------------------------------|---------|---------|--------------------------------------------------------------------------------------------------------------------------------------------|
| ucil-daemon (local)  | `cargo build -p ucil-daemon --bin ucil-daemon` (incremental cache warm)      | ~1s     | no      | Binary builds. `ucil-daemon mcp --stdio` produces no MCP responses — `McpServer::serve()` is not yet wired into `main.rs` (see §Failures). |
| Serena (uvx)         | `uvx --from git+https://github.com/oraios/serena@v1.0.0 serena-mcp-server`   | ~4s     | yes     | MCP handshake OK; 20 tools advertised including `find_symbol`, `find_referencing_symbols`, `get_symbols_overview`.                         |
| pyright-langserver   | `npx -y pyright` fallback                                                    | ~16s    | partial | Process starts; never emits a framed `textDocument/publishDiagnostics` response to the LSP didOpen probe within the 15s window.            |

## Tests

| Suite                                    | Passed | Failed | Skipped | Duration | Exit |
|------------------------------------------|--------|--------|---------|----------|------|
| scripts/verify/e2e-mcp-smoke.sh          | 0      | 1      | 0       | 3s       | 1    |
| scripts/verify/serena-live.sh            | 1      | 0      | 0       | 4s       | 0    |
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

## Failures

### 1. `scripts/verify/e2e-mcp-smoke.sh` — exit 1 (3s)

Daemon compiled cleanly (incremental build, cache warm), but no JSON-RPC
response appeared on stdout after sending `initialize` + `tools/list`
over `ucil-daemon mcp --stdio`.

Log tail (stderr):

```
[e2e-mcp-smoke] FAIL: daemon produced no stdout responses

  This usually means McpServer::serve() has not yet been wired
  into ucil-daemon's main.rs as a subcommand. server.rs has
  McpServer::serve(reader, writer) but main.rs only calls
  tracing_subscriber::fmt::init() and exits. Wire it like:

    match std::env::args().nth(1).as_deref() {
        Some("mcp") => server::McpServer::new()
            .serve(tokio::io::stdin(), tokio::io::stdout()).await?,
        _ => { /* daemon mode */ }
    }
```

Interpretation: the `mcp --stdio` subcommand is still not wired into
`crates/ucil-daemon/src/main.rs`. The MCP handshake and the 22-tool
inventory cannot be verified from the CLI surface until it is. This is
the same symptom the script's own self-diagnostic text predicts.

This is NOT a tester-side issue: fixing it is a source edit in
`ucil-daemon` and therefore belongs to the planner/executor loop, not
the integration tester.

### 2. `scripts/verify/diagnostics-bridge.sh` — exit 1 (16s)

Pyright was invoked through the `npx -y pyright` fallback path (system
`pyright-langserver` is not on `PATH`). The script's LSP handshake was
sent (process ran through its 15s inner wait), but no framed
`textDocument/publishDiagnostics` message with a non-empty diagnostic
list was extracted from the Content-Length stream.

Log tail (stderr):

```
[diagnostics-bridge] FAIL: no publishDiagnostics with a non-empty diagnostic list
-- messages received --
```

(No `method` entries were listed below the banner — the framed-message
extractor produced an empty JSONL, suggesting pyright either never
emitted any LSP frames or emitted output in a form the Content-Length
extractor in the script didn't parse on this runtime.)

Interpretation: either the `npx -y pyright` fallback invokes the
pyright CLI (not the LSP server) under the installed npx/Node version,
or pyright is emitting diagnostics to stderr / in a non-framed form
that the probe doesn't capture. A system install of
`pyright-langserver` on `PATH` (via `npm i -g pyright` or
`pipx install pyright`) would take the happy path and is the cleanest
next step; the script deliberately prefers `pyright-langserver --stdio`
when present, so the fallback is not exercising the same binary the
LSP bridge would talk to in production.

This is an environmental / invocation-shape issue, not a bug in
`ucil-lsp-diagnostics` source that the integration tester should fix.
Per the integration-tester contract the tester does not edit source or
scripts — this failure is flagged and left for planner / executor
triage.

### 3. `scripts/verify/serena-live.sh` — exit 0 (4s) ✅

Passed. Serena v1.0.0 spawned via `uvx`, responded to MCP
`initialize` + `tools/list`, advertised 20 tools including the three
required by the G1 structural group (`find_symbol`,
`find_referencing_symbols`, `get_symbols_overview`). Log:

```
[serena-live] spawning Serena via uvx (pinned v1.0.0)...
[serena-live] OK — Serena v1.0.0 alive, advertises 20 tools including find_symbol find_referencing_symbols get_symbols_overview.
```

## Artifacts

Full logs captured at:

```
ucil-build/verification-reports/phase-1-integration-logs/
├── e2e-mcp-smoke.stdout    e2e-mcp-smoke.stderr    e2e-mcp-smoke.rc    e2e-mcp-smoke.dur
├── serena-live.stdout      serena-live.stderr      serena-live.rc      serena-live.dur
├── diagnostics-bridge.stdout  diagnostics-bridge.stderr  diagnostics-bridge.rc  diagnostics-bridge.dur
├── session.id              start.ts                 verified_at.ts
```

- Session id: `itg-fed2e5de-fdad-42e7-8215-46535d3668f1`
- Start: `2026-04-18T20:28:10Z`
- Verified at: `2026-04-18T20:29:02Z`
- Duration: ~52s (all three scripts incl. per-script setup)

## Docker teardown

No `docker compose up` was performed — no compose files exist for
phase 1, and the phase-1 scripts do not require Docker (see §Services).
Nothing to tear down. No stray containers or networks from this
tester session.

## Verdict

**FAIL.** Two of three phase-1 gate scripts failed (`e2e-mcp-smoke.sh`,
`diagnostics-bridge.sh`). The phase-1 gate formula requires every
`scripts/verify/*.sh` entry used by `scripts/gate-check.sh 1` to exit 0;
two non-zero exits keep the gate red. `serena-live.sh` is green.

Next-step recommendations (for planner, not this tester):

1. Wire `ucil-daemon mcp --stdio` in `crates/ucil-daemon/src/main.rs`
   so `McpServer::serve` receives stdio. This is already prescribed by
   the `e2e-mcp-smoke.sh` self-diagnostic message.
2. Make `pyright-langserver` available on the gate runner (install via
   `npm i -g pyright` or `pipx install pyright`), or have
   `diagnostics-bridge.sh` detect the `npx` fallback shape and invoke
   pyright in a form that actually launches the language-server. Either
   change is out-of-scope for the integration tester.
