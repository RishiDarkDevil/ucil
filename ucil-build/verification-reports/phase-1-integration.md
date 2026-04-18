# Phase 1 Integration Report

**Tester session**: itg-5f0b42cc-d821-4f42-8e33-ac2c6bdab080
**Verified at**: 2026-04-18T20:17:12Z
**Phase**: 1 (Week 1, progress.json)
**Verdict**: FAIL

## Summary

Phase-1 gate requires three live smoke scripts (no mocks of Serena, LSP,
or the UCIL daemon). Of the three, only `serena-live.sh` passed.
`e2e-mcp-smoke.sh` and `diagnostics-bridge.sh` both failed and block the
phase gate.

## Services

Phase-1 scripts do not require Docker: `scripts/verify/serena-live.sh`
explicitly documents "No mocks, no docker — Phase 1 runs Serena locally
via uvx as declared in the plugin manifest (master-plan §13)." No
`docker/*-compose.yaml` files exist in the repository, consistent with
that design. A `docker ps` at the start of the run confirmed Docker was
not needed for phase-1 fixtures; the daemon socket is also not
accessible to the session user (`/var/run/docker.sock` is `srw-rw---- 1
root docker`; session user is not in the `docker` group), so the phase-1
scripts run everything in-process / over uvx / over npx instead.

| Service              | Source / Image                               | Up time | Healthy | Notes                                                         |
|----------------------|----------------------------------------------|---------|---------|---------------------------------------------------------------|
| ucil-daemon (local)  | `cargo build -p ucil-daemon --bin ucil-daemon` | ~4s     | no      | Binary builds, but `ucil-daemon mcp --stdio` produces no MCP responses (see e2e-mcp-smoke failure). |
| Serena (uvx)         | `uvx --from git+https://github.com/oraios/serena@v1.0.0 serena-mcp-server` | ~3s     | yes     | MCP handshake OK, 20 tools advertised incl. `find_symbol`, `find_referencing_symbols`, `get_symbols_overview`. |
| pyright-langserver   | `npx -y pyright` fallback                    | ~16s    | partial | Process starts but never emits a framed `publishDiagnostics` response to the LSP didOpen probe. |

## Tests

| Suite                                    | Passed | Failed | Skipped | Duration | Exit |
|------------------------------------------|--------|--------|---------|----------|------|
| scripts/verify/e2e-mcp-smoke.sh          | 0      | 1      | 0       | 5s       | 1    |
| scripts/verify/serena-live.sh            | 1      | 0      | 0       | 3s       | 0    |
| scripts/verify/diagnostics-bridge.sh     | 0      | 1      | 0       | 16s      | 1    |
| cargo nextest integration (skipped)      | —      | —      | —       | —        | —    |
| pnpm adapters integration (skipped)      | —      | —      | —       | —        | —    |
| pytest integration (skipped)             | —      | —      | —       | —        | —    |

Per-WO cargo/pnpm/pytest integration tiers are run by the verifier subagent per work-order (they show up in `WO-*.md` reports under `ucil-build/verification-reports/`); this phase-integration pass is the black-box wrapper that the three `scripts/verify/*.sh` entries cover for the phase-1 gate. They are deliberately not re-run here to avoid shadowing the gate's own invocation.

## Failures

### 1. `scripts/verify/e2e-mcp-smoke.sh` — exit 1 (5s)

Daemon compiled cleanly, but no JSON-RPC response appeared on stdout
after sending `initialize` + `tools/list`.

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

Interpretation: the `mcp --stdio` subcommand has not yet been wired into
`crates/ucil-daemon/src/main.rs`. The MCP handshake and the 22-tool
inventory cannot be verified until it is. This is the same symptom the
script's own self-diagnostic text predicts, so the fix is well-scoped
for a follow-up executor work-order.

### 2. `scripts/verify/diagnostics-bridge.sh` — exit 1 (16s)

Pyright was invoked through the `npx -y pyright` fallback path (system
`pyright-langserver` is not on PATH). The script's LSP handshake
completed (process ran to its 15s inner wait), but no framed
`textDocument/publishDiagnostics` message with a non-empty diagnostic
list was extracted from the Content-Length stream.

Log tail (stderr):

```
[diagnostics-bridge] FAIL: no publishDiagnostics with a non-empty diagnostic list
-- messages received --
```

(No `method` entries were listed below the banner — the framed-message
extractor produced an empty JSONL, suggesting pyright either never
emitted any LSP frames or emitted output that the extractor couldn't
parse on this runtime.)

Interpretation: either the `npx -y pyright` fallback invokes the pyright
CLI instead of the LSP server, or pyright emitted diagnostics to stderr
/ in a non-framed form that the probe doesn't capture. A system install
of `pyright-langserver` (`npm i -g pyright` or `pipx install pyright`)
would take the happy path and is the cleanest next step; the script
itself deliberately bails with a PASS when `pyright-langserver` is
present on PATH, so the fallback is not exercising the same binary the
LSP bridge would talk to in production.

This is an environmental / invocation-shape issue, not a bug in
`ucil-lsp-diagnostics` source that the integration tester should fix.
Per the integration-tester contract the tester does not edit source or
scripts — this failure is flagged and left for planner/executor
triage.

### 3. `scripts/verify/serena-live.sh` — exit 0 (3s) ✅

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

## Docker teardown

No `docker compose up` was performed — no compose files exist for phase
1, and the phase-1 scripts do not require Docker (see §Services).
Nothing to tear down.

## Verdict

**FAIL.** Two of three phase-1 gate scripts failed (`e2e-mcp-smoke.sh`,
`diagnostics-bridge.sh`). The phase-1 gate formula requires every
`scripts/verify/*.sh` entry used by `scripts/gate-check.sh 1` to exit 0;
two non-zero exits make the gate red. `serena-live.sh` is green.

Next-step recommendations (for planner, not this tester):

1. Wire `ucil-daemon mcp --stdio` in `crates/ucil-daemon/src/main.rs`
   so `McpServer::serve` receives stdio. This is already prescribed by
   the script's own diagnostic message.
2. Ensure `pyright-langserver` is available on the gate runner (e.g.
   install pyright globally as part of the gate bootstrap), or have
   `diagnostics-bridge.sh` fall back to a known-good invocation shape
   for pyright when only `npx` is present. Either change is out-of-scope
   for the integration tester.
