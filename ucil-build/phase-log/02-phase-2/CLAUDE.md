# Phase 2 — Plugins + G1/G2 + embeddings (Weeks 6–8)

## Goal (master-plan §18)
Plugin system operational. G1 (Structural) and G2 (Search) fully working. Embedding pipeline operational.

## Features in scope (25)
- **Week 6 — Plugin system**: `P2-W6-F01..F08`
  Plugin manifest parser (capabilities + activation rules), full lifecycle state machine
  (DISCOVERED→REGISTERED→LOADING→ACTIVE→IDLE→STOPPED→ERROR), hot-reload, circuit breakers,
  ast-grep + Probe manifests, `ucil plugin` CLI subcommands, lifecycle integration suite.
- **Week 7 — G1 + G2 fusion**: `P2-W7-F01..F09`
  G1 parallel execution (tree-sitter + Serena + ast-grep + diagnostics), G1 result fusion,
  G2 intra-group RRF (Probe×2.0, ripgrep×1.5, LanceDB×1.5), session deduplication,
  `find_references` / `search_code` MCP tools, ripgrep plugin smoke, SCIP P1, LanceDB per-branch.
- **Week 8 — Embedding pipeline**: `P2-W8-F01..F08`
  `ucil-embeddings` crate with ONNX Runtime, CodeRankEmbed (137M, CPU) default + Qwen3 (8B GPU)
  upgrade, LanceDB integration, background indexing, throughput / latency / recall benchmarks.

## Gate criteria (`scripts/gate/phase-2.sh`)
- All 25 phase-2 features `passes=true` and `last_verified_by` starts with `verifier-`.
- No `#[ignore]`/`xfail`/`skip` on phase-2 tests.
- Plugin lifecycle integration suite (`tests/integration/test_plugin_lifecycle.rs`) green.
- Vector search benchmark recorded in `docs/benchmarks.md`.

## Dependencies (external)
- ast-grep binary on PATH (Week 6).
- probe binary (https://github.com/buger/probe) on PATH.
- ripgrep on PATH (already a dev tool).
- ONNX Runtime via the `ort` crate (Week 8). CPU build is the default; GPU is opt-in behind a
  cargo feature.
- LanceDB via `lancedb` crate.

## Risks carried from Phase 1
- Plugin manifest parsing currently models only `[plugin]`, `[transport]`, `[lifecycle]`.
  `[capabilities]` (provides / languages / activation rules) and `[resources]` are NEW fields
  introduced in Week 6 — schema-compat with existing fixture manifests must be tested.
- Plugin lifecycle state machine in `crates/ucil-daemon/src/plugin_manager.rs` has only the
  HOT/COLD subset (`Active ↔ Idle`). Phase 2 must drive the remaining transitions
  (`Discovered → Registered → Loading → Active`, `* → Stopped`, `* → Error`) with logged
  transitions per master-plan §15.2 (`ucil.<layer>.<op>` span naming).
- Frozen test selectors per DEC-0007: `plugin_manager::test_manifest_parser` and
  `plugin_manager::test_lifecycle_state_machine` MUST land at the module root (NOT inside a
  `mod tests { }` wrapper) so nextest's path matches the feature-list selector.

## Standing rules for Phase 2 work-orders
1. **stdout pristine on stdio MCP path** — tracing always writes to stderr in `ucil-daemon mcp`
   (carried from Phase 1 lessons); plugin runtime logs MUST honour the same constraint.
2. **No mocks of Serena, LSP, ast-grep, Probe, ripgrep, LanceDB, ONNX Runtime** — integration
   tests use real subprocesses or the Docker fixtures.
3. **Frozen `#[test]` selectors** — every acceptance selector in `feature-list.json` must match
   exactly the path nextest reports. Wrap or unwrap `mod tests { }` accordingly. Cite DEC-0007
   when re-locating.
4. **No regressions on Phase 1 acceptance** — `cargo test --workspace --no-fail-fast` stays
   green at every WO boundary; verifier reruns the Phase 1 gate scripts.

## Lessons Learned (seeded by planner; appended by executors)

_(none yet — first WO of Phase 2 is WO-0042)_
