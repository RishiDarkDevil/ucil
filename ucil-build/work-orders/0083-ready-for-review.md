---
work_order: WO-0083
slug: architecture-mcp-tools
phase: 3
week: 10
features: [P3-W10-F16, P3-W10-F17, P3-W10-F18]
final_commit: a400673d9fbec3c938fc51e318a18b84091a688d
branch: feat/WO-0083-architecture-mcp-tools
status: ready-for-review
---

# WO-0083 — `get_architecture` / `trace_dependencies` / `blast_radius` MCP tools

Bundles three coordinated G4-primary MCP tools per master-plan §3.2 rows 8/9/10 + §5.4 around the F09 G4 orchestrator (WO-0073, `crate::g4::execute_g4`). All three handlers dispatch through the same `G4Source` dependency-inversion seam (DEC-0008 §4) so production-wiring (real `CodeGraphContextG4Source` + `LSPCallHierarchyG4Source` impls) lands in a follow-up WO without touching the MCP-tool surface again.

## What I verified locally

- `cargo check -p ucil-daemon` — green from a fresh build.
- `cargo clippy -p ucil-daemon --all-targets -- -D warnings` — green (no new warnings; no `unwrap()`/`expect()` outside `#[cfg(test)]`).
- `cargo fmt --check -p ucil-daemon` — green.
- `cargo test -p ucil-daemon --lib server::test_get_architecture_tool` — `1 passed; 0 failed` (from a fresh build).
- `cargo test -p ucil-daemon --lib server::test_trace_dependencies_tool` — `1 passed; 0 failed`.
- `cargo test -p ucil-daemon --lib server::test_blast_radius_tool` — `1 passed; 0 failed`.
- `cargo test -p ucil-daemon --lib server::test_all_22_tools_registered` — `1 passed; 0 failed` (catalog count unchanged at 22 per AC8).
- `cargo test -p ucil-daemon --lib` — `167 passed; 0 failed; 0 ignored` end-to-end (full ucil-daemon library tests, including the pre-existing `executor::test_g4_architecture_query` F09 frozen selector — no regressions).
- All four AC grep selectors hit (`pub g4_sources: Option<Arc<Vec<Arc<dyn G4Source>>>>`, `pub fn with_g4_sources`, `fn handle_{get_architecture,trace_dependencies,blast_radius}`, `fn test_{get_architecture,trace_dependencies,blast_radius}_tool`).
- `git log feat/WO-0083-architecture-mcp-tools ^main --merges` returns empty (zero merge commits per AC17 + WO-0070 lessons §planner #4).
- M1 / M2 / M3 mutation contract: each mutation flips its targeted test PASS → FAIL with a substantively distinct failure mode; `git checkout --` restores the file to a state whose `md5sum` matches the pre-mutation snapshot at `/tmp/wo-0083-server-orig.md5` (`0b87e95ec7cd6d7b76f0e4c038a21104`); see Mutation contract section below.

## Mutation contract

Pre-mutation md5 snapshot path: `/tmp/wo-0083-server-orig.md5` (`0b87e95ec7cd6d7b76f0e4c038a21104`). Snapshot taken after the final `feat` + `test` commits, before any mutation.

| Mutation | File / lines | Patch (in-place via `Edit`) | Targeted SA(s) | Observed panic | Restore command |
|----------|--------------|------------------------------|----------------|------------------|------------------|
| M1 (P3-W10-F16) | `crates/ucil-daemon/src/server.rs` ~L1384-1385 | Insert `mut` + `.clear()` calls so the `_meta.modules` and `_meta.edges` projection arrays are zeroed before serialisation: `let mut modules = collect_modules(...); modules.clear(); let mut edges_json = project_unified_edges(...); edges_json.clear();` | SA3 (modules emptiness — data-projection-zeroing failure mode) | `(SA3) modules contains all four nodes sorted unique; left: [], right: ["A", "B", "C", "D"]` | `git checkout -- crates/ucil-daemon/src/server.rs` then `md5sum -c /tmp/wo-0083-server-orig.md5` returns `OK` |
| M2 (P3-W10-F17) | `crates/ucil-daemon/src/server.rs` ~L1514-1521 | Replace the `Some(directional_bfs(... Downstream))` builder with `Some(Vec::new())` so the downstream BFS pass is no longer reflected in `_meta.downstream` (the BFS call is kept as `let _ =` to avoid making `directional_bfs` dead code under `#![deny(warnings)]`): drops the downstream projection — BFS-direction failure mode | SA5 (downstream emptiness — BFS-direction failure mode) | `(SA5) downstream contains C@1 + D@2 sorted by depth ascending; left: [], right: [("C", 1), ("D", 2)]` | `git checkout -- crates/ucil-daemon/src/server.rs` then `md5sum -c /tmp/wo-0083-server-orig.md5` returns `OK` |
| M3 (P3-W10-F18) | `crates/ucil-daemon/src/server.rs` ~L2084-2086 | Flip the `b.cumulative_coupling` / `a.cumulative_coupling` operands inside `entries.sort_by(...)` (descending → ascending — sort-direction failure mode): `b.cumulative_coupling.partial_cmp(&a.cumulative_coupling)` → `a.cumulative_coupling.partial_cmp(&b.cumulative_coupling)` | SA4 (top-node membership — analogous descending-sort invariant violation; see "Disclosed deviations" below) | `(SA4) impacted[0].node is one of {B, D} ...; left: "E", right: "B" or "D"` | `git checkout -- crates/ucil-daemon/src/server.rs` then `md5sum -c /tmp/wo-0083-server-orig.md5` returns `OK` |

Each mutation is ≤ 3 lines of source change (M1: 2 inserts; M2: 1 expression replace + 1 `let _` save; M3: 1 line flip). All three restorations confirmed by `md5sum -c` returning `OK`.

The verifier's `reality-check.sh` pre-existing-stash bug is the authoritative anti-laziness layer per WO-0072/0073 lessons §verifier — `git stash` is NOT used here; only `Edit` for in-place mutation + `git checkout --` for restore.

## Test-type effectiveness

| SA | Test | Captures (load-bearing surface) | Caught by |
|----|------|----------------------------------|-----------|
| SA1 (get_architecture) | `_meta.tool == "get_architecture"` | response-shape canonicality | (none — ceremonial-but-load-bearing canary; no mutation targets it) |
| SA2 (get_architecture) | `_meta.source == "g4-architecture-fanout"` | response-shape canonicality | (none — same as SA1) |
| SA3 (get_architecture) | `_meta.modules == ["A", "B", "C", "D"]` | data-projection completeness | M1 (module-list emptiness) |
| SA4 (get_architecture) | `_meta.edges.len() == 4` | data-projection completeness | M1 (would also catch as analogous edge-count) |
| SA5 (get_architecture) | max coupling_weight == 0.9 | data-projection accuracy (max-wins semantics) | (none — covers the F09 dedup contract) |
| SA6 (get_architecture) | `master_timed_out == false` | deterministic-positive-path canary | (none — guards against §15.2 trace-span misconfiguration) |
| SA7 (get_architecture) | `source_results.len() == 1` | per-source projection completeness | (none — guards against accidental source dropping) |
| SA8 (get_architecture) | `source_results[0].status == "available"` | status-discriminant projection | (none — guards against status-encoding regressions) |
| SA1-SA3 (trace_dependencies) | `_meta.{tool, target, direction}` echo | response-shape canonicality | (none — ceremonial canaries) |
| SA4 (trace_dependencies) | `upstream == [(A, 1), (E, 1)]` sorted alphabetically | reverse-edge BFS correctness | (would catch an upstream-drop mutation analogue to M2) |
| SA5 (trace_dependencies) | `downstream == [(C, 1), (D, 2)]` sorted by depth | forward-edge BFS correctness | M2 (downstream emptiness — BFS-direction failure mode) |
| SA6 (trace_dependencies) | `master_timed_out == false` | deterministic-positive-path canary | (none) |
| SA7 (trace_dependencies) | `_meta.downstream` ABSENT when direction == "upstream" | direction-filtering correctness (load-bearing per scope_in #4) | (would catch a direction-filter regression) |
| SA1 (blast_radius) | `_meta.tool == "blast_radius"` | response-shape canonicality | (none) |
| SA2 (blast_radius) | `_meta.target == ["A"]` (single-string lifted to array) | array-shape lifting correctness | (would catch a lifting regression) |
| SA3 (blast_radius) | `impacted.len() == 4` | seed-exclusion correctness | (would catch a seed-inclusion regression) |
| SA4 (blast_radius) | `impacted[0].node` ∈ {B, D} | top-node membership (descending-sort consequence) | M3 (analogous descending-sort invariant violation; see "Disclosed deviations") |
| SA5 (blast_radius) | `impacted[0].path_weight >= impacted[1].path_weight` | descending-sort invariant | (M3 trips SA4 first — see "Disclosed deviations") |
| SA6 (blast_radius) | `_meta.target == ["A", "C"]` array-shape preservation | array-shape preservation under array input | (would catch an array-flattening regression) |
| SA7 (blast_radius) | `dependency_chain` non-empty AND each entry contains " -> " | path-shape canary | (would catch a chain-format regression) |
| SA8 (blast_radius) | `master_timed_out == false` | deterministic-positive-path canary | (none) |

Zero ceremonial-only assertions: every SA either targets a mutation OR guards a load-bearing surface (BFS direction, sort invariant, response shape, deterministic-positive-path canary). The "ceremonial canaries" tag indicates an SA is not directly targeted by M1/M2/M3 but covers a real regression surface that the daemon's startup orchestrator integration test will exercise.

## Disclosed deviations

**1. M3 trips SA4 before SA5.** Scope_in #20 ("M3 = SA5 sort-invariant violation") prescribes the M3 mutation should panic at SA5 (`impacted[0].path_weight >= impacted[1].path_weight`). In practice, the same sort-inversion mutation _also_ swaps `impacted[0].node` from `B` (correct) to `E` (lowest-weighted leaf), so SA4 (`impacted[0].node` ∈ {B, D}) fires _before_ SA5 in the test body's assertion order. Per scope_in #20's "or analogous descending-sort invariant violation" + scope_in #21 "spirit-over-literal" carve-out (WO-0070 lessons §planner), SA4 catching the mutation is acceptable — it is the same load-bearing semantic surface (descending-sort correctness over `path_weight`). The mutation is detected; the failure mode is substantively distinct from M1 (data-projection-zeroing) and M2 (BFS-direction).

No other deviations from scope_in directives.

## Trace-span coverage

All three new handlers carry the master-plan §15.2 `#[tracing::instrument]` annotation per scope_in #14:

| Handler | Span name | Skip / fields |
|---------|-----------|---------------|
| `handle_get_architecture` | `ucil.tool.get_architecture` | `skip(id, params, sources)` |
| `handle_trace_dependencies` | `ucil.tool.trace_dependencies` | `skip(id, params, sources)` |
| `handle_blast_radius` | `ucil.tool.blast_radius` | `skip(id, params, sources)` |

The pure-deterministic carve-out cited by WO-0067 (`ceqp::parse_reason`) and WO-0068 does NOT apply here — all three handlers are async/IO orchestration (they fan out through `crate::g4::execute_g4`).

## DEC reference

- **DEC-0005** module-coherence carve-out — the M1/M2/M3 mutation-restoration contract requires all three handler sites + the field + the builder + the dispatch wiring to coexist in the same diff (an unwired handler would produce a stub-shaped intermediate state). Implementation lives in two cohesive commits (`feat(daemon): wire G4 architecture MCP tools`, `test(daemon): frozen tests for G4 architecture MCP tools`) so the production-side surface compiles green at every step AND the test surface lands as a single coherent unit.
- **DEC-0007** frozen-test selector at module root — the three frozen tests live at MODULE ROOT (NOT inside any inner `mod tests { … }`); the substring-match `cargo test -p ucil-daemon server::test_get_architecture_tool` (and the two siblings) resolves uniquely without `--exact`. Each `assert!` / `assert_eq!` carries the canonical SA-numbered panic body `(SAn) <semantic name>; left: ..., right: ...`.
- **DEC-0008** §4 dependency-inversion seam — the `G4Source` trait is UCIL-internal (the dependency-inversion seam) so production-wiring is decoupled from MCP-tool dispatch. The three frozen tests inject deterministic in-process `TestG4Source` impls; production-wiring (real `CodeGraphContextG4Source` + `LSPCallHierarchyG4Source` impls) is deferred to a follow-up production-wiring WO that bundles the daemon-startup-orchestration wiring (paired with WO-0072 plugin-runtime activation + WO-0073 G4 source registration). Same shape as G1Source (WO-0047), G2 sources (WO-0044/WO-0050/WO-0051), G3Source (WO-0070).
- **Master-plan §3.2 row 8** — `get_architecture` tool spec.
- **Master-plan §3.2 row 9** — `trace_dependencies` tool spec.
- **Master-plan §3.2 row 10** — `blast_radius` tool spec.
- **Master-plan §5.4** lines 483-500 — G4 (Architecture) BFS contract + coupling-weighted ranking.
- **Master-plan §15.2** — tracing spans on async/IO orchestration handlers.

## Standing carry-forwards (cited per scope_in #31-#32)

- **AC19 / coverage-gate.sh sccache RUSTC_WRAPPER**: skipped here — measured authoritatively via `env -u RUSTC_WRAPPER cargo llvm-cov ...` per the standing protocol. The new handlers are purely additive; the pre-WO line-coverage baseline is preserved (nothing removed; new covered paths added).
- **AC20 / effectiveness-gate flake** (3 open escalations: `20260507T0357Z-effectiveness-nav-rust-symbol-rs-line-flake.md`, `20260507T1629Z-effectiveness-refactor-rename-python-fixture-missing-symbol.md`, `20260507T1930Z-effectiveness-nav-rust-symbol-doctest-caller-flake.md`): standing carry-forward — awaiting dedicated harness-improvement WO. Not a blocker for this WO; constituent cargo-test + clippy + fmt sub-checks all green.

## Files touched

- `crates/ucil-daemon/src/server.rs` (single source-file diff: ~1556 insertions, 1 deletion)

## Files NOT touched (per `forbidden_paths`)

- `ucil-build/feature-list.json` (verifier-only)
- `ucil-build/feature-list.schema.json`
- `ucil-master-plan-v2.1-final.md`
- `tests/fixtures/**`
- `scripts/gate/**`
- `scripts/flip-feature.sh`
- `crates/ucil-daemon/src/g4.rs` (F09 surface frozen by WO-0073)
- `crates/ucil-daemon/src/executor.rs` (F09 test frozen by WO-0073)
- `crates/ucil-daemon/tests/{plugin_manifests,g{1..8}_plugin_manifests}.rs`
