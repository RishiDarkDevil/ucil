# DEC-0009: search_code uses in-process ripgrep libraries, not the `rg` binary

**Status**: accepted
**Date**: 2026-04-18
**Phase**: 1
**Feature**: `P1-W5-F09`
**Work-order**: `WO-0035`

## Context

`P1-W5-F09` promotes the `search_code` MCP tool from its `_meta.not_yet_implemented: true` stub to a real handler that returns "results from tree-sitter symbol index and **ripgrep** text search merged and de-duplicated" (`ucil-build/feature-list.json`, frozen description).

The word *ripgrep* is in the spec. There are two ways to satisfy "ripgrep text search":

1. **Shell out to the `rg` binary** via `tokio::process::Command`.
2. **Embed ripgrep's own Rust libraries in-process** (`ignore` for filesystem walking + `.gitignore` respect, `grep-searcher` for per-file streaming, `grep-regex` for matcher construction, `grep-matcher` for the `Matcher` trait).

The master plan does not resolve the ambiguity; §3.2 row 4 and §18 Phase 1 Week 5 line 1765 both say "search_code via MCP" without prescribing the implementation channel.

## Decision

Use the **in-process ripgrep libraries** (`ignore`, `grep-searcher`, `grep-regex`, `grep-matcher`) added as workspace dependencies in `Cargo.toml`.

Specifically:

* `ignore = "0.4"` — recursive directory walker that respects `.gitignore`, `.ignore`, hidden-file rules, and symlink cycles. Same walker ripgrep itself uses.
* `grep-searcher = "0.1"` — per-file streaming searcher with bounded memory and line-by-line match emission.
* `grep-regex = "0.1"` — regex-based `Matcher` implementation.
* `grep-matcher = "0.1"` — the `Matcher` trait the searcher consumes.

The `search_code` handler wires these into a `text_search(root, query, max_results)` helper in `crates/ucil-daemon/src/text_search.rs` that returns a `Vec<TextMatch { file_path, line_number, line_text }>`.

## Rationale

* **No external binary dependency.** Phase 1 invariant #2 ("tree-sitter grammars loaded from crate dependencies, not system-installed binaries") is the analogous precedent for parser crates; the same spirit applies here — UCIL should not rely on `rg` being on the operator's PATH.
* **CI determinism.** Integration tests on CI runners without `rg` installed stay green; avoids a new install step in the harness.
* **Anti-laziness contract compliance.** The contract forbids mocking critical deps (Serena, LSP, SQLite, LMDB, Docker). Shelling out to `rg` would create a hidden "critical dep" that the acceptance test would need to either mock (forbidden) or install at test time (fragile).
* **Tokio async hygiene.** Spawning `rg` per call through `tokio::process::Command` would add process-fork overhead on every `search_code` call. In-process walking under `tokio::task::spawn_blocking` is cheaper and keeps the hot path inside the tokio worker pool.
* **Wire identical behavior.** `grep-searcher` + `grep-regex` + `ignore` are the *exact* libraries `ripgrep` itself composes (`rg` is ~1000 lines of CLI glue on top of these). Behavior parity is guaranteed by construction.
* **Licence compatibility.** All four crates are MIT/Unlicense dual-licensed (same as UCIL's other deps).

## Consequences

* Adds four workspace `[workspace.dependencies]` entries (`ignore`, `grep-searcher`, `grep-regex`, `grep-matcher`).
* `ucil-daemon` gets a new `text_search.rs` module. Strictly bounded: walks paths under a caller-supplied root, returns a `Vec<TextMatch>`, no global state, no retention.
* The regex engine is `regex-automata` under the hood (pulled transitively through `grep-regex`). `grep-regex` 0.1 is on regex 1, matching the workspace regex toolchain.
* Phase 2+ fusion (§18 Phase 2 Week 7 line 1783 "Wire into find_definition, find_references, search_code") replaces this narrow helper with the full G1/G2 fusion layer — the Phase 1 handler is the foundational building block, not the final shape. The handler's `_meta.source: "tree-sitter+ripgrep"` lineage tag advertises this so future phases can extend without clobbering.
* `search_code` remains filesystem-bound. Vector / semantic search (Phase 2 Week 8) does not touch this handler's scope.

## Revisit trigger

Revisit if any of:

1. Operators report measurable overhead from in-process walking on very large repos (>5M files) where the `rg` binary's mmap strategy would outperform the Rust lib composition.
2. `grep-searcher` / `grep-regex` / `ignore` stop being co-maintained with `rg` (as of 2026-04 they share the `BurntSushi/ripgrep` monorepo and are versioned together).
3. Phase 2 fusion work discovers a fundamental limitation of `grep-searcher`'s match emission model that forces a binary spawn.

## Alternatives considered

**Shell out to `rg`.** Rejected for CI determinism and anti-laziness compliance above. A per-call subprocess spawn is also latency-adverse.

**Hand-rolled regex scan using `regex` only.** Would lose `.gitignore` respect (a core ripgrep feature), and `grep-searcher`'s bounded-memory streaming is non-trivial to replicate. Not worth the code.

**Tantivy full-text index.** Out of scope for Phase 1 — Tantivy is a separate indexer with its own invalidation story. Phase 2+ may add it for `find_similar`; orthogonal to this decision.

## References

* `ucil-master-plan-v2.1-final.md` §3.2 row 4 (`search_code` tool) and §18 Phase 1 Week 5 line 1765 (Phase 1 deliverable).
* `ucil-build/feature-list.json` row `P1-W5-F09` (frozen description).
* `ucil-build/phase-log/01-phase-1/CLAUDE.md` invariant #2 (grammars from crate deps, not binaries — precedent).
* `CLAUDE.md` Anti-laziness contract (no mocks of critical deps; no new hidden binary deps).
* BurntSushi/ripgrep repo — proves `grep-*` + `ignore` are the libraries that compose into the `rg` binary itself.
