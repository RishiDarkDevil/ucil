# Phase 2 Integration Report

**Tester session**: itg-f251392c-36ad-4e96-814b-04ed66621df8
**Started at**:     2026-05-07T18:32:31Z
**Verified at**:    2026-05-07T18:33:07Z
**Phase**:          2 (Week 1, per `ucil-build/progress.json`)
**HEAD commit**:    b1459f85909f0802e5f62eb0d6c8ee3e8f57a5f6
**Verdict**:        PASS

## Summary

Phase-2 inherits the Phase-1 black-box smoke triad (no mocks of
Serena, LSP, or the UCIL daemon) and adds a "LanceDB / ONNX
availability" sanity check on top. All three Phase-1 scripts pass on
this run; the Phase-2 vector-stack collaborators (LanceDB and ONNX
Runtime) are linked into the workspace as cargo crates and resolve
cleanly from `Cargo.lock` (`lancedb 0.16.0`, `ort 2.0.0-rc.12`, with
`ucil-embeddings 0.1.0` referencing both via `workspace = true`).
Detailed per-feature embedding/recall benches (`bench-embed.sh`,
`recall-at-10.sh`, `golden-fusion.sh`) are run by
`scripts/gate/phase-2.sh`, not by this integration-tester pass — this
pass is the agent-visible black-box wrapper.

The source-code delta between this run's HEAD `b1459f8` and the prior
integration HEAD `e1844ce` is six commits — every one of which
touches `ucil-build/verification-reports/**` only (refreshes of
coverage-*.md timestamps, refreshes of this very file from a fresh
gate-check, refreshes of `phase-2-integration-logs/*`, and the
auto-commit-hook bumps that landed as the integration-tester wrote
its own log artefacts under `phase-2-integration-logs/`). The daemon,
Serena adapter, pyright bridge, and `ucil-embeddings` sources are
bit-for-bit identical to the prior verified HEAD. This run is
therefore a re-confirmation under a fresh tester session, not a
re-validation of new code.

- `scripts/verify/e2e-mcp-smoke.sh` — **exit 0** (PASS, 401 ms).
  `cargo build -p ucil-daemon` served from a fully warm incremental
  cache (no source delta versus the prior verification HEAD
  `e1844ce`); the daemon answered `initialize` and `tools/list` over
  `ucil-daemon mcp --stdio`. All 22 frozen MCP tools advertise the
  four CEQP universal params.
- `scripts/verify/serena-live.sh` — **exit 0** (PASS, 3 222 ms).
  Serena v1.0.0 spawned via `uvx` and advertised 20 tools, including
  the three required by G1 structural (`find_symbol`,
  `find_referencing_symbols`, `get_symbols_overview`).
- `scripts/verify/diagnostics-bridge.sh` — **exit 0** (PASS, 314 ms).
  `pyright` v1.1.409 on PATH at
  `/home/rishidarkdevil/.nvm/versions/node/v22.22.2/bin/pyright`; the
  script ran `pyright --outputjson __diagnostics_probe.py` against a
  copy of `tests/fixtures/python-project/` and parsed
  `generalDiagnostics`, finding one `error`-severity diagnostic for
  the deliberate `int → str` mismatch in the probe. Tenth consecutive
  passing run for this script.

Because all gate scripts pass, the overall verdict is **PASS**.

## Services

Phase-2 scripts do not require Docker; no `docker/*-compose.yaml`
files exist anywhere in the repository (consistent with master-plan
§13 and `scripts/verify/serena-live.sh`'s "No mocks, no docker —
Phase 1 runs Serena locally via uvx as declared in the plugin
manifest"). Per `.claude/agents/integration-tester.md`, Phase 2 layers
a "LanceDB / ONNX model check" on top of Phase 1's Serena + LSP
fixtures; both LanceDB and ONNX Runtime are linked into the workspace
as Rust crate dependencies (`Cargo.lock` resolves `lancedb 0.16.0`,
`ort 2.0.0-rc.12`, and the local `ucil-embeddings 0.1.0` crate
references both via `workspace = true`), not as standalone services.
Docker-backed fixtures (Postgres / MySQL / Arc-Memory / DBHub) become
relevant only in Phase 3+. A `docker info` probe at the start of this
run confirmed the host's docker client is present (Docker Engine
v29.4.2, Buildx plugin v0.33.0, Compose v5.1.3) but the daemon socket
is unreachable from this session ("permission denied while trying to
connect to the docker API at unix:///var/run/docker.sock"), so no
compose stand-up was attempted — also unnecessary for Phase 2.

| Service               | Source / Image                                                                | Up time | Healthy | Notes                                                                                                                              |
|-----------------------|-------------------------------------------------------------------------------|---------|---------|------------------------------------------------------------------------------------------------------------------------------------|
| ucil-daemon (local)   | `cargo build -p ucil-daemon --bin ucil-daemon` (warm incremental cache)       | <1s     | yes     | Binary builds and answers MCP `initialize` + `tools/list` over stdio; 22 tools with CEQP params on all.                            |
| Serena (uvx)          | `uvx --from git+https://github.com/oraios/serena@v1.0.0 serena-mcp-server`    | ~3s     | yes     | MCP handshake OK; 20 tools advertised including `find_symbol`, `find_referencing_symbols`, `get_symbols_overview`.                 |
| pyright (batch CLI)   | `pyright` v1.1.409 on PATH (nvm-installed; `pyright-langserver` co-installed) | <1s     | yes     | `pyright --outputjson` against fixture probe returned 1 diagnostic of severity=error for the deliberate `int → str` assignment.    |
| LanceDB (linked)      | `lancedb` cargo crate v0.16.0 (resolvable in Cargo.lock)                      | n/a     | n/a     | Linked into `ucil-embeddings` via workspace dep (`req=^0.16`); Phase-2 acceptance for vector storage runs through workspace cargo tests. |
| ONNX Runtime (linked) | `ort` cargo crate v2.0.0-rc.12 (resolvable in Cargo.lock)                     | n/a     | n/a     | Linked into `ucil-embeddings` via workspace dep (`req==2.0.0-rc.12`); CodeRankEmbed throughput / latency / recall benches live under `scripts/gate/phase-2.sh`. |

## Tests

| Suite                                          | Passed | Failed | Skipped | Duration |
|------------------------------------------------|--------|--------|---------|----------|
| `scripts/verify/e2e-mcp-smoke.sh`              | 1      | 0      | 0       | 401 ms   |
| `scripts/verify/serena-live.sh`                | 1      | 0      | 0       | 3 222 ms |
| `scripts/verify/diagnostics-bridge.sh`         | 1      | 0      | 0       | 314 ms   |
| LanceDB / ONNX linkage probe (Cargo.lock grep) | 2      | 0      | 0       | <1 ms    |
| `cargo build -p ucil-embeddings --quiet`       | 1      | 0      | 0       | 165 ms   |

Per-feature acceptance tests (`cargo nextest`, `pnpm vitest`,
`pytest`) are owned by the per-WO verifier sessions and the
phase-gate (`scripts/gate-check.sh 2`) — not duplicated here, by
design.

## Failures

(none)

## Logs

Per-script captures live in
`ucil-build/verification-reports/phase-2-integration-logs/`:

```
phase-2-integration-logs/
  e2e-mcp-smoke.rc          → 0
  e2e-mcp-smoke.dur         → 401 (ms)
  e2e-mcp-smoke.stdout      → "[e2e-mcp-smoke] OK — 22 tools registered, CEQP params on all, daemon spoke MCP cleanly."
  e2e-mcp-smoke.stderr      → empty
  serena-live.rc            → 0
  serena-live.dur           → 3222 (ms)
  serena-live.stdout        → "[serena-live] OK — Serena v1.0.0 alive, advertises 20 tools …"
  serena-live.stderr        → empty
  diagnostics-bridge.rc     → 0
  diagnostics-bridge.dur    → 314 (ms)
  diagnostics-bridge.stdout → "[diagnostics-bridge] OK — pyright returned 1 diagnostic(s) for the probe (severity=error)."
  diagnostics-bridge.stderr → empty
  ucil-embeddings-build.rc  → 0
  ucil-embeddings-build.dur → 165 (ms)
  lancedb-onnx.txt          → Cargo.lock grep + ucil-embeddings dependency declarations + cargo build smoke
```

## Teardown

Nothing to tear down: no docker compose stand-up was performed
(daemon socket unreachable + Phase 2 does not require it). uvx
processes for Serena spawn and exit per script invocation; pyright
batch CLI is one-shot. All temp dirs from
`scripts/verify/{e2e-mcp-smoke,serena-live,diagnostics-bridge}.sh`
are removed via the script-internal `trap 'rm -rf "$TMP"' EXIT`
handlers.

## Provenance

- HEAD at start of run: `a6fdb267ba31f767d60e4a7406ea3c94eb1706d3` (clean working tree, ahead=0).
- HEAD at end of run:   `b1459f85909f0802e5f62eb0d6c8ee3e8f57a5f6` (six intervening auto-commit-hook commits in `ucil-build/verification-reports/**`; no source touched).
- Tester role:          `integration-tester` (per `.claude/agents/integration-tester.md`).
- Phase from progress:  `2` (`jq .phase ucil-build/progress.json`).
- Toolchain probed:     docker v29.4.2 (compose v5.1.3, daemon socket unreachable); uvx 0.11.6; pyright 1.1.409; cargo 1.94.1; rustc 1.94.1.
- Concurrency note:     a parallel integration-tester pass (session `itg-33c2b400-cb9d-4a42-8cc6-2df22c11a213`, start `2026-05-07T18:34:35Z`) was active during this report write and refreshed `phase-2-integration-logs/{e2e-mcp-smoke,serena-live,ucil-embeddings-build}.dur` to within ±20 ms of the values cited above (`e2e-mcp-smoke 401→415 ms`, `serena-live 3222→3226 ms`, `ucil-embeddings-build 165→183 ms`); `diagnostics-bridge.dur` was unchanged at 314 ms. Verdicts are identical (rc=0 across the board), so the variance does not affect this report's conclusion.
