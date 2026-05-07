# Phase 2 Integration Report

**Tester session**: itg-c8f4c58f-16f1-439d-8a16-6ff535a37d8e
**Started at**:     2026-05-07T16:21:23Z
**Verified at**:    2026-05-07T16:22:00Z
**Phase**:          2 (Week 1, per `ucil-build/progress.json`)
**HEAD commit**:    c84f996d1ad48c2c3c205db76fc8935f000e521d
**Verdict**:        PASS

## Summary

Phase-2 inherits the Phase-1 black-box smoke triad (no mocks of Serena,
LSP, or the UCIL daemon) and adds a "LanceDB / ONNX availability"
sanity check on top. All three Phase-1 scripts pass on this run; the
Phase-2 vector-stack collaborators (LanceDB and ONNX Runtime) are
linked into the workspace as cargo crates and resolve from
`Cargo.lock` (`lancedb 0.16.0`, `ort 2.0.0-rc.12`). Detailed per-feature
embedding/recall benches (`bench-embed.sh`, `recall-at-10.sh`,
`golden-fusion.sh`) are run by `scripts/gate/phase-2.sh`, not by this
integration-tester pass — this pass is the agent-visible black-box
wrapper.

- `scripts/verify/e2e-mcp-smoke.sh` — **exit 0** (PASS, 931 ms).
  `cargo build -p ucil-daemon` served from the warm incremental cache
  (`target/debug/ucil-daemon` already linked from the prior session at
  HEAD `267746a`); the daemon answered `initialize` and `tools/list`
  over `ucil-daemon mcp --stdio`. All 22 frozen MCP tools advertise
  the four CEQP universal params.
- `scripts/verify/serena-live.sh` — **exit 0** (PASS, 3 220 ms).
  Serena v1.0.0 spawned via `uvx` and advertised 20 tools, including
  the three required by G1 structural (`find_symbol`,
  `find_referencing_symbols`, `get_symbols_overview`).
- `scripts/verify/diagnostics-bridge.sh` — **exit 0** (PASS, 417 ms).
  `pyright` v1.1.409 on PATH at
  `/home/rishidarkdevil/.nvm/versions/node/v22.22.2/bin/pyright`; the
  script ran `pyright --outputjson __diagnostics_probe.py` against a
  copy of `tests/fixtures/python-project/` and parsed
  `generalDiagnostics`, finding one `error`-severity diagnostic for
  the deliberate `int → str` mismatch in the probe. Third consecutive
  passing run for this script (prior passes: itg-607e685c on HEAD
  `7d89ca9`, itg-4f3a1070 on HEAD `267746a`).

Because all gate scripts pass, the overall verdict is **PASS**.

## Services

Phase-2 scripts do not require Docker; no `docker/*-compose.yaml`
files exist in the repository (consistent with master-plan §13 and
`scripts/verify/serena-live.sh`'s "No mocks, no docker — Phase 1 runs
Serena locally via uvx as declared in the plugin manifest"). Per
`.claude/agents/integration-tester.md`, Phase 2 layers a "LanceDB /
ONNX model check" on top of Phase 1's Serena + LSP fixtures; both
LanceDB and ONNX Runtime are linked into the workspace as Rust crate
dependencies (`Cargo.lock` resolves `lancedb 0.16.0`, `ort 2.0.0-rc.12`,
and the local `ucil-embeddings` crate references both via
`workspace = true`), not as standalone services. Docker-backed
fixtures (Postgres / MySQL / Arc-Memory / DBHub) become relevant only
in Phase 3+. A `docker info` probe at the start of this run confirmed
the host's docker client is present (Docker Engine v29.4.2, Buildx
v0.33.0, Compose v5.1.3) but the daemon socket is unreachable from
this session ("permission denied while trying to connect to the
docker API at unix:///var/run/docker.sock"), so no compose stand-up
was attempted.

| Service               | Source / Image                                                              | Up time | Healthy | Notes                                                                                                                              |
|-----------------------|-----------------------------------------------------------------------------|---------|---------|------------------------------------------------------------------------------------------------------------------------------------|
| ucil-daemon (local)   | `cargo build -p ucil-daemon --bin ucil-daemon` (warm incremental cache)     | <1s     | yes     | Binary builds and answers MCP `initialize` + `tools/list` over stdio; 22 tools with CEQP params on all.                            |
| Serena (uvx)          | `uvx --from git+https://github.com/oraios/serena@v1.0.0 serena-mcp-server`  | ~3s     | yes     | MCP handshake OK; 20 tools advertised including `find_symbol`, `find_referencing_symbols`, `get_symbols_overview`.                 |
| pyright (batch CLI)   | `pyright` v1.1.409 on PATH (nvm-installed; `pyright-langserver` co-installed) | <1s     | yes     | `pyright --outputjson` against fixture probe returned 1 diagnostic of severity=error for the deliberate `int → str` assignment.    |
| LanceDB (linked)      | `lancedb` cargo crate v0.16.0 (resolvable in Cargo.lock)                    | n/a     | n/a     | Linked into `ucil-embeddings` via workspace dep; Phase-2 acceptance for vector storage runs through workspace cargo tests.         |
| ONNX Runtime (linked) | `ort` cargo crate v2.0.0-rc.12 (resolvable in Cargo.lock)                   | n/a     | n/a     | Linked into `ucil-embeddings` via workspace dep; CodeRankEmbed throughput / latency / recall benches live under `scripts/gate/phase-2.sh`. |

## Tests

| Suite                                    | Passed | Failed | Skipped | Duration | Exit |
|------------------------------------------|--------|--------|---------|----------|------|
| scripts/verify/e2e-mcp-smoke.sh          | 1      | 0      | 0       | 931ms    | 0    |
| scripts/verify/serena-live.sh            | 1      | 0      | 0       | 3220ms   | 0    |
| scripts/verify/diagnostics-bridge.sh     | 1      | 0      | 0       | 417ms    | 0    |
| cargo nextest integration (deferred)     | —      | —      | —       | —        | —    |
| pnpm adapters integration (deferred)     | —      | —      | —       | —        | —    |
| pytest integration (deferred)            | —      | —      | —       | —        | —    |

Per-WO cargo / pnpm / pytest integration tiers are run by the verifier
subagent per work-order (see `WO-*.md` reports under
`ucil-build/verification-reports/`). Phase-2-specific gate checks
(`plugin-hot-cold.sh`, `bench-embed.sh`, `golden-fusion.sh`,
`recall-at-10.sh`, `effectiveness-gate.sh 2`, `multi-lang-coverage.sh
2`, `real-repo-smoke.sh 2`) are invoked by `scripts/gate/phase-2.sh`
and are deliberately not re-run here to avoid shadowing the gate's own
invocation.

## Passes

### 1. `scripts/verify/e2e-mcp-smoke.sh` — exit 0 (931 ms)

```
[e2e-mcp-smoke] building ucil-daemon...
[e2e-mcp-smoke] OK — 22 tools registered, CEQP params on all, daemon spoke MCP cleanly.
```

The 0.9s wall-time reflects a fully warm incremental cargo build (the
`target/debug/ucil-daemon` link survived from the prior session at the
preceding HEAD `267746a`; HEAD `c84f996` is a no-source-change
verification-report-only commit, so the cache hit was complete) plus
the MCP handshake round-trip itself. The 22 frozen tool names from
master-plan §3 are all present and every tool advertises the four
CEQP universal params (`reason`, `current_task`, `files_in_context`,
`token_budget`).

Full logs: `phase-2-integration-logs/e2e-mcp-smoke.{stdout,stderr,rc,dur}`.

### 2. `scripts/verify/serena-live.sh` — exit 0 (3 220 ms)

```
[serena-live] spawning Serena via uvx (pinned v1.0.0)...
[serena-live] OK — Serena v1.0.0 alive, advertises 20 tools including find_symbol find_referencing_symbols get_symbols_overview.
```

Serena was spawned via
`uvx --from git+https://github.com/oraios/serena@v1.0.0 serena-mcp-server --context ide-assistant --project <cwd>`
and answered the MCP handshake plus a `tools/list` with 20 tools,
including the three required by G1 structural.

Full logs: `phase-2-integration-logs/serena-live.{stdout,stderr,rc,dur}`.

### 3. `scripts/verify/diagnostics-bridge.sh` — exit 0 (417 ms)

```
[diagnostics-bridge] OK — pyright returned 1 diagnostic(s) for the probe (severity=error).
```

`pyright` and `pyright-langserver` are both installed via nvm at
`/home/rishidarkdevil/.nvm/versions/node/v22.22.2/bin/`. The script's
`pyright --outputjson __diagnostics_probe.py` invocation, run inside
a tmp copy of `tests/fixtures/python-project/`, returned a single
`generalDiagnostics` entry at `severity=error` for the deliberate
`int → str` mismatch in the probe file. Third consecutive passing
run (prior passes: itg-607e685c on HEAD `7d89ca9`, itg-4f3a1070 on
HEAD `267746a`); the earlier eight phase-1 reports recorded the same
FAIL shape until pyright was installed on PATH.

Full logs: `phase-2-integration-logs/diagnostics-bridge.{stdout,stderr,rc,dur}`.

## Failures

(none)

## Tear-down

No Docker services were started (none required for Phase 2 and none
possible on this host's current docker permissions), so no compose
`down` was needed. All three verification scripts clean up their own
tempdirs via `trap 'rm -rf "$TMP"' EXIT`. No long-lived processes
were spawned by this integration-tester pass.

## Artifacts

- `phase-2-integration-logs/e2e-mcp-smoke.{stdout,stderr,rc,dur}`
- `phase-2-integration-logs/serena-live.{stdout,stderr,rc,dur}`
- `phase-2-integration-logs/diagnostics-bridge.{stdout,stderr,rc,dur}`
- `phase-2-integration-logs/session.id`, `start.ts`, `verified_at.ts`
