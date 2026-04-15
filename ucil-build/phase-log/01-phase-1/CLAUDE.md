# Phase 1 — Daemon core + tree-sitter + Serena + diagnostics bridge

## Goals summary
Build a running daemon (`ucild`) with tree-sitter indexing, a session manager, branch
detection, LMDB tag cache, SQLite knowledge graph, Serena integration, LSP diagnostics
bridge, plugin manager, and all 22 MCP tools registered. By end of phase the daemon
accepts queries within 2 s of startup and returns real symbol data for the fixture repos.

Weeks 2–5. Master plan §18 Phase 1 lines 1729–1770.

## Features in scope (34 total)
```
P1-W2-F01  ucil-treesitter: multi-language parser (≥10 languages)
P1-W2-F02  ucil-treesitter: symbol extraction (functions, classes, structs …)
P1-W2-F03  ucil-treesitter: AST-aware chunker (≤512 token chunks at function boundaries)
P1-W2-F04  ucil-treesitter: LMDB tag cache via heed (file_path+mtime → Vec<Symbol>)
P1-W2-F05  ucil-daemon: session manager (session creation, branch detection, worktree discovery)
P1-W2-F06  ucil-daemon/ucil-core: two-tier storage layout (.ucil/shared/ + .ucil/branches/<branch>/)
P1-W3-F01  ucil-daemon: process lifecycle (daemonize, PID file, SIGTERM/SIGHUP)
P1-W3-F02  ucil-daemon: file watcher (notify + debouncer, 100ms, PostToolUse fast path)
P1-W3-F03  ucil-daemon: Watchman integration (auto-detected for repos >50K files)
P1-W3-F04  ucil-core: Salsa incremental engine skeleton (DAG, invalidation, early cutoff)
P1-W3-F05  ucil-daemon: plugin manager skeleton (discover plugin.toml, spawn, health check)
P1-W3-F06  ucil-daemon: HOT/COLD plugin lifecycle (idle timeout, on-demand restart)
P1-W3-F07  ucil-daemon: basic MCP server over stdio (all 22 tools registered, stubs ok)
P1-W3-F08  ucil-daemon: progressive startup (<2s to first query acceptance)
P1-W3-F09  ucil-daemon: crash recovery (checkpoint.json, restore on restart)
P1-W4-F01  ucil-core: SQLite schema (WAL, busy_timeout, symbols/knowledge_entries/sessions tables)
P1-W4-F02  ucil-core: knowledge_graph.rs CRUD + bi-temporal queries + symbol resolution
P1-W4-F03  ucil-core: symbol resolution (name + optional file scope → definition location)
P1-W4-F04  ucil-core: tree-sitter extraction pipeline → knowledge graph population
P1-W4-F05  ucil-daemon: find_definition MCP tool (real data, CEQP params)
P1-W4-F06  ucil-core: CEQP universal parameters on all 22 tool schemas
P1-W4-F07  ucil-daemon: session state tracking (per-session call history, dedup)
P1-W4-F08  ucil-daemon: hot staging writes (raw observations captured immediately)
P1-W4-F09  ucil-daemon: session layer query deduplication
P1-W4-F10  ucil-daemon: MCP _meta.available_tools + _meta.degraded_tools in first response
P1-W5-F01  ucil-lsp-diagnostics: crate + Serena plugin manifest
P1-W5-F02  ucil-lsp-diagnostics: LSP JSON-RPC client (textDocument/diagnostic)
P1-W5-F03  ucil-lsp-diagnostics: callHierarchy + typeHierarchy client calls
P1-W5-F04  ucil-lsp-diagnostics: G7 quality pipeline (type errors → quality_issues)
P1-W5-F05  ucil-lsp-diagnostics: fallback LSP servers (pyright, rust-analyzer, tsserver)
P1-W5-F06  ucil-daemon: Serena → G1 structural fusion (find_symbol, find_references, go_to_definition)
P1-W5-F07  ucil-daemon: find_references MCP tool (real data via Serena + tree-sitter)
P1-W5-F08  ucil-daemon: search_code MCP tool (tree-sitter symbol search)
P1-W5-F09  ucil-daemon: _meta.startup_health populated on first response
```

## Gate criteria
`scripts/gate/phase-1.sh` exits 0 — which requires:
1. `cargo nextest run --workspace` green
2. `cargo clippy --workspace -- -D warnings` clean
3. `scripts/verify/e2e-mcp-smoke.sh` — 22 tools registered over stdio MCP
4. `scripts/verify/serena-live.sh` — Serena docker-backed integration
5. `scripts/verify/ts-reparse-p95.sh` — P95 incremental reparse <10ms on fixture
6. `scripts/verify/diagnostics-bridge.sh` — diagnostics bridge responds
7. `scripts/verify/effectiveness-gate.sh 1` — nav scenarios beat grep baseline
8. `scripts/verify/multi-lang-coverage.sh 1` — multi-language parse probes

## Dependencies (external services)
- Docker (for Serena integration tests) — must be running for P1-W5-F01+
- `watchman` optional binary (P1-W3-F03 auto-detects)
- `rust-analyzer`, `pyright`, `typescript-language-server` optional (diagnostics bridge fallback)

## Invariants
1. All acceptance tests must run real code — no mocks of tree-sitter, Serena MCP, SQLite, or LMDB.
2. Tree-sitter grammars loaded from crate dependencies (not system-installed binaries).
3. Session IDs are UUIDs v4. Never reused.
4. LMDB environment opened once at daemon startup via `OnceLock<heed::Env>`.
5. `ucil-treesitter` depends only on `ucil-core` and tree-sitter crates — no daemon crates.
6. MCP server communicates over stdout/stdin using JSON-RPC 2.0. No HTTP in Phase 1.
7. All `tokio::spawn` tasks use `tokio::time::timeout` on every IO `.await`.
8. SQLite databases opened in WAL mode with `PRAGMA busy_timeout = 10000`.
9. Stubs for unimplemented MCP tools MUST return `{"_meta":{"not_yet_implemented":true}}`.

## Risks carried from Phase 0
- Serena availability: Serena integration tests (W5) need Docker; run locally may be slow.
- heed/LMDB on CI: may need `libclang` or `lmdb-sys` bundled feature. Document in ADR if needed.
- Grammar crate version alignment: tree-sitter-language-pack exists as an alternative to individual grammar crates — evaluate before picking (ADR if non-obvious).
