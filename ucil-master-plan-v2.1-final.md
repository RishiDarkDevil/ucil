# UCIL — Unified Code Intelligence Layer
## Definitive implementation plan v2.1

**Project codename**: `ucil`
**Repository**: `ucil` (standalone, MIT license)
**Languages**: Rust (daemon core, CLI, performance-critical) + TypeScript (MCP server, host adapters) + Python (ML pipelines, embeddings)
**What it is**: A persistent per-project daemon that orchestrates 60+ code intelligence tools into a single unified interface — a project-specific brain that evolves over time. Exposed as an MCP server with progressive tool disclosure, distributed as a Claude Code plugin with host-specific adapters for Claude Code, Codex CLI, Aider, Cline/Roo Code, Cursor, and Ollama-based agents.
**Timeline**: 24 weeks across 9 phases.

---

## Table of contents

1. [System overview](#1-system-overview)
2. [Architecture layers](#2-architecture-layers)
3. [Tool surface — 22 tools, always available](#3-tool-surface)
4. [Tool inventory and groups](#4-tool-inventory-and-groups)
5. [Intra-group fusion strategy](#5-intra-group-fusion-strategy)
6. [Cross-group fusion and the unified pipeline](#6-cross-group-fusion-and-the-unified-pipeline)
7. [Agent layer](#7-agent-layer)
8. [Context-Enriched Query Protocol (CEQP)](#8-context-enriched-query-protocol)
9. [Host adapters and Claude Code plugin](#9-host-adapters-and-claude-code-plugin)
10. [Daemon architecture](#10-daemon-architecture)
11. [Multi-agent, branches, and worktrees](#11-multi-agent-branches-and-worktrees)
12. [Knowledge graph and persistent storage](#12-knowledge-graph-and-persistent-storage)
13. [Serena + LSP diagnostics bridge](#13-serena-lsp-diagnostics-bridge)
14. [Plugin system](#14-plugin-system)
15. [Observability and telemetry](#15-observability-and-telemetry)
16. [CLI specification](#16-cli-specification)
17. [Directory structure](#17-directory-structure)
18. [Phase-wise implementation plan](#18-phase-wise-implementation-plan)
19. [Testing and validation strategy](#19-testing-and-validation-strategy)
20. [Configuration reference](#20-configuration-reference)
21. [Production readiness](#21-production-readiness)

---

## 1. System overview

### 1.1 What UCIL is

UCIL is a **project-specific brain** for AI coding agents. It runs as a persistent daemon for each project/repository, continuously indexes and understands the codebase, and provides a unified API that any AI coding agent can query to get deep, accurate, token-efficient context about the code. It is a centralized brain — not an inter-agent communication framework.

UCIL differentiates from generic MCP gateways (AIRIS, MetaMCP) through **deep code intelligence fusion** — combining type information, call graphs, data flow, structural importance (PageRank), semantic embeddings, and temporal knowledge into a unified ranking that pure aggregators cannot replicate.

### 1.2 Core principles

1. **Quality first, waste nothing**: Every query returns the deepest, richest context for what's relevant — full detail where it matters, excluded where it doesn't. No artificial budget caps that force compression of important content. No bloat that dumps irrelevant modules. Every piece of information gets the tokens it deserves — nothing less, nothing more.
2. **Agent-rich, not rule-bound**: LLM agents are involved in most queries, not just edge cases. Agents provide richer interpretation, deeper synthesis, more nuanced conflict resolution, and contextual narrative that deterministic rules cannot match. The deterministic path is the fallback (when no LLM provider is configured), not the default.
3. **All tools, all the time**: Every tool/MCP/plugin in a group runs on every relevant query. Results are fused together — not cascaded with fallbacks. Fallbacks exist only for genuinely unavailable tools (not installed, crashed). During init and session start, UCIL verifies every tool is operational.
4. **Evolving intelligence**: The system gets smarter over time as it accumulates knowledge from agent sessions, human commits, PRs, issues, documentation, and architectural decisions.
5. **Full tool surface, always**: All 22 tools are always available to every agent. Even at ~1000 tokens per tool definition, the full set costs ~22K tokens — roughly 11% of a 200K context window. Every host gets everything.
6. **Token-smart, not token-cheap**: No artificial budget caps that force lossy compression of relevant content. But also no bloat — irrelevant results are excluded, not compressed. Session dedup avoids repeating what the agent already has. PageRank relevance ranking ensures the most important content comes first. `_meta.token_count` lets hosts trim from the bottom if needed. Complex tasks get more context; simple lookups stay lean.
7. **Vendor agnostic**: Works with any AI coding agent through host adapters. The unified layer knows nothing about specific LLM vendors.
8. **Fully local**: Zero cloud dependencies. Everything runs on the developer's machine.
9. **Extensible**: Adding a new tool = adding a new plugin directory with a manifest + MCP server. No core code changes.
10. **Multi-agent native**: Supports 3–5 concurrent agents across branches and worktrees. Knowledge flows naturally through the shared brain.
11. **Observable**: OpenTelemetry instrumented from day one. Every tool call, fusion step, and agent invocation is traced.

### 1.3 What UCIL provides to agents

Every query returns not just what was asked, but everything the agent needs:

- **Structural understanding**: Functions, classes, modules, signatures, relationships, call chains
- **Semantic understanding**: What code does conceptually, why it was written that way
- **Type intelligence**: Type errors, hover information via Serena + LSP diagnostics bridge
- **Architectural context**: How components connect, dependency chains, blast radius
- **Historical context**: Why decisions were made (PRs, issues, ADRs), how code evolved
- **Conventions**: Coding style, naming patterns, project-specific rules (proactively included)
- **Pitfalls**: Known issues, foot-guns, past mistakes (proactively included as bonus context)
- **Quality signals**: Lint warnings, type errors, security vulnerabilities, test coverage gaps
- **Memory**: Agent learnings from previous sessions, shared across the project brain
- **Related code**: Utilities, helpers, patterns the agent should reuse (proactively found)
- **Tests**: Test files that need updating after changes (proactively identified)
- **Runtime context**: Error traces, performance data, crash reports (when connected to Sentry/Datadog)

### 1.4 Naming conventions

- Daemon process: `ucild`
- CLI: `ucil`
- MCP server: `ucil-mcp`
- Plugin tools: `ucil-plugin-{name}`
- Config file: `ucil.toml`
- Project data directory: `.ucil/`
- Claude Code plugin: `ucil-plugin/`

---

## 2. Architecture layers

```
┌─────────────────────────────────────────────────────────┐
│  Layer 7: DISTRIBUTION (Claude Code Plugin)             │
│  .claude-plugin/ · Skills · Hooks · Subagents · Rules   │
├─────────────────────────────────────────────────────────┤
│  Layer 6: HOST ADAPTERS                                 │
│  Claude Code · Codex CLI · Aider · Cline · Cursor       │
├─────────────────────────────────────────────────────────┤
│  Layer 5: UNIFIED API (MCP Server, 22 tools)            │
│  All tools always loaded · MCP Elicitation               │
│  + CEQP context params · OpenTelemetry instrumentation   │
├─────────────────────────────────────────────────────────┤
│  Layer 4: AGENT LAYER                                   │
│  Query-path: Interpreter · Synthesis · Conflict ·        │
│              Clarification (Elicitation)                 │
│  Background: Convention · Memory Curator · Arch Narr     │
├─────────────────────────────────────────────────────────┤
│  Layer 3: ORCHESTRATION ENGINE (deterministic)           │
│  Query Router → Parallel Executor → RRF Fusion           │
│  → Conflict Resolution → Context Compiler                │
│  → Bonus Context Selector → CEQP Response Builder        │
├─────────────────────────────────────────────────────────┤
│  Layer 2: TOOL GROUPS (8 groups, intra-group fusion)     │
│  G1:Structural · G2:Search · G3:Knowledge                │
│  G4:Architecture · G5:Context · G6:Platform              │
│  G7:Quality+Security · G8:Testing+CI                     │
├─────────────────────────────────────────────────────────┤
│  Layer 1.5: LSP DIAGNOSTICS BRIDGE                      │
│  Complements Serena: type errors, lint warnings,         │
│  call/type hierarchies from pyright, rust-analyzer, etc. │
├─────────────────────────────────────────────────────────┤
│  Layer 1: DAEMON CORE                                   │
│  Incremental Engine (Salsa) · File Watcher (notify,     │
│  optional Watchman for large repos)                     │
│  Persistent DB (SQLite WAL) · Vector Store (LanceDB)    │
│  Plugin Manager · Tiered Cache · Compaction Agent        │
│  Session Manager · Branch Index Manager                  │
│  OpenTelemetry Exporter                                 │
└─────────────────────────────────────────────────────────┘
```

### 2.1 Layer 1 — Daemon core

The foundation. Manages the lifecycle of all other layers.

**Components**:
- **Incremental computation engine**: Salsa 2022 (v0.25+). Tracks a dependency DAG of all computations. When a file changes, only recomputes affected queries. Early cutoff: if a recomputed function returns the same result, propagation stops. **Critical design constraint**: intermediate representations must be position-independent (use AstId references, not byte offsets) to preserve early cutoff when whitespace changes. Durability system: standard library signatures tagged HIGH durability, user code tagged LOW — skips ~300ms of wasted verification per keystroke.
- **File watcher**: `notify` crate as default (used by rust-analyzer, Helix, Zed, cargo-watch) with `notify-debouncer-full` for production event dedup. Monitors all worktree directories. Debounces rapid changes (100ms window for human edits; PostToolUse hook notifications bypass debounce). **Watchman as optional upgrade**: auto-detected at `ucil init`, recommended for repos >50K files. Watchman provides persistent subscriptions, cookie-based sync, and a query language that outperforms `notify` at scale. Fallback to `PollWatcher` for network mounts and Docker on macOS.
- **Persistent storage manager**: Manages shared databases (knowledge.db, memory.db, history.db) and per-branch databases (symbols.db, vectors/, tags.lmdb). Handles migrations, WAL checkpointing, and integrity checks. All write transactions use `BEGIN IMMEDIATE` to avoid SQLITE_BUSY upgrade failures. `PRAGMA busy_timeout = 10000`.
- **Plugin manager**: Discovers plugins via manifest scanning, lazy-loads them on first relevant query, manages their lifecycle. HOT/COLD lifecycle: servers start on-demand, auto-terminate when idle.
- **Tiered cache**: L0 (in-memory LRU, <1ms), L1 (local index/LMDB, <10ms), L2 (full tool invocation, 100ms–seconds).
- **Compaction agent**: Background process that periodically prunes stale knowledge graph nodes, re-ranks importance scores, compresses memory.
- **Session manager**: Manages per-agent sessions with isolated state, branch detection, and worktree mapping.
- **Branch index manager**: Creates, updates, prunes, and archives per-branch code indexes. Delta indexing from parent branches for fast creation.
- **OpenTelemetry exporter**: `opentelemetry-rust` crate with span naming `ucil.{layer}.{operation}`. Exports to Jaeger/OTLP/stdout. Every tool call, cache check, fusion step, and plugin invocation is traced.

### 2.2 Layer 1.5 — LSP diagnostics bridge

A lightweight daemon-internal component that taps into the same LSP servers that Serena manages (or independently spawns them) to pull **diagnostics, type errors, and lint warnings** into the quality pipeline. This complements Serena — Serena handles navigation and refactoring via MCP (find_symbol, find_references, go_to_definition, replace_symbol_body), while the diagnostics bridge extracts type errors and warnings that Serena doesn't surface well.

**What the diagnostics bridge provides** (capabilities Serena lacks):
- Diagnostics: type errors, lint warnings, compilation errors → feeds G7 Quality
- Call hierarchy (incoming/outgoing calls) → enriches G4 Architecture
- Type hierarchy (inheritance chains) → enriches G4 Architecture

**What Serena provides** (capabilities the bridge defers to):
- Go-to-definition, find-references, hover information → G1 Structural (Serena's core strength)
- Code actions: quick fixes, refactoring suggestions → refactoring tools
- Rename symbol with cross-file reference updates → refactoring tools
- Symbol search, LSP-powered navigation across 40+ languages → G1 Structural

**LSP servers shared between Serena and the bridge**:

| Language | LSP Server | Detection |
|----------|-----------|-----------|
| Python | pyright / pylsp | pyproject.toml, *.py |
| TypeScript/JS | typescript-language-server | tsconfig.json, package.json |
| Rust | rust-analyzer | Cargo.toml |
| Go | gopls | go.mod |
| C/C++ | clangd | CMakeLists.txt, compile_commands.json |
| Java | eclipse.jdt.ls | pom.xml, build.gradle |

**Architecture**: When Serena is active (P0 plugin), the diagnostics bridge connects to the same LSP server instances Serena manages — no duplicate processes. When Serena is not available (degraded mode), the bridge spawns its own LSP servers. The bridge runs as a daemon-internal component (not a separate plugin process).

### 2.3 Layer 2 — Tool groups

Eight groups of tools. Each group fuses results internally (intra-group fusion) before passing up. See Section 4 for the tool inventory and Section 5 for fusion strategies.

**Change from v1**: Added G7 (Quality & Security) and G8 (Testing & CI) as dedicated groups. These were previously scattered across other groups or absent entirely.

### 2.4 Layer 3 — Orchestration engine

Receives queries from the agent layer, classifies them, dispatches to relevant tool groups, fuses results via weighted RRF, resolves conflicts, compiles context within token budgets, selects bonus context, and formats the CEQP response. See Section 6.

### 2.5 Layer 4 — Agent layer

Seven internal agents. Four activate selectively in the query path for complex/ambiguous queries. Three run as background daemons enriching the knowledge graph continuously. See Section 7.

### 2.6 Layer 5 — Unified API

Single MCP server exposing **22 tools, all always loaded**. Each tool accepts CEQP universal parameters (`reason`, `current_task`, `files_in_context`, `token_budget`). Supports MCP Elicitation for clarification. See Section 3 for the complete tool list.

### 2.7 Layer 6 — Host adapters

Vendor-specific bridges. See Section 9.

### 2.8 Layer 7 — Distribution (Claude Code Plugin)

UCIL ships as a Claude Code plugin containing MCP server config, skills, hooks, subagent definitions, and rules. This is also the distribution mechanism for other hosts via the Agent Skills open standard (agentskills.io). See Section 9.

---

## 3. Tool surface — 22 tools, always available

### 3.1 Design philosophy

All 22 UCIL tools are always registered with every host. Even at ~1000 tokens per tool definition (rich descriptions with CEQP parameter documentation), the full set costs ~22K tokens — roughly 11% of a 200K context window. This is well within budget for Claude Code, Codex, Cursor (22 tools is far under its ~40 tool cap), Cline, and Aider. The simplicity of "everything is always available" eliminates deferred loading machinery, Tool Search dependency, intent classification risk, and host-specific capability negotiation.

Each tool has a single, unambiguous purpose. The agent picks the right one directly. CEQP enrichment (reason, current_task, files_in_context, token_budget) applies to every tool — the `reason` parameter adds proactive bonus context to ANY call.

### 3.2 Complete tool list

| # | Tool | Purpose | Primary groups |
|---|------|---------|----------------|
| 1 | `understand_code` | Explain what a file/function/module does, why it exists, its context | G1, G3, G5 |
| 2 | `find_definition` | Go-to-definition with full context (signature, docs, callers) | G1, G2 |
| 3 | `find_references` | All references to a symbol, grouped by usage type (call, import, type) | G1, G2, G4 |
| 4 | `search_code` | Hybrid search: text + structural + semantic | G2, G1 |
| 5 | `find_similar` | Find code similar to a given snippet or pattern | G2, G1 |
| 6 | `get_context_for_edit` | Optimal context for editing a file/region. Token-budget-aware. Conventions, pitfalls, related code, tests included. | All groups |
| 7 | `get_conventions` | Project coding style, naming conventions, patterns in use | G3, G5, G1 |
| 8 | `get_architecture` | High-level architecture overview, module boundaries, data flow | G4, G3, G5 |
| 9 | `trace_dependencies` | Upstream and downstream dependency chains for a file/module/symbol | G4, G1, G3 |
| 10 | `blast_radius` | What would be affected by changing this code? | G4, G1, G3 |
| 11 | `explain_history` | Why was this code written this way? PR/issue/ADR context | G3, G6, G5 |
| 12 | `remember` | Store or retrieve agent learnings, decisions, observations | G3 |
| 13 | `review_changes` | Analyze diff/PR against conventions, quality, security, tests, blast radius | G7, G8, G4, G1 |
| 14 | `check_quality` | Run lint + type check + security scan on specified code | G7 |
| 15 | `run_tests` | Execute tests for changed code, return results + coverage | G8 |
| 16 | `security_scan` | Deep security analysis: SAST + SCA + secrets + container scan | G7 |
| 17 | `lint_code` | Language-specific deep linting (ESLint, Ruff, RuboCop, clippy) | G7 |
| 18 | `type_check` | Type checking diagnostics via LSP diagnostics bridge | G7 |
| 19 | `refactor` | Safe refactoring with cross-file reference updates via Serena | G1 |
| 20 | `generate_docs` | Generate/update project documentation (architecture, module, API, onboarding) | G3, G4, G5 |
| 21 | `query_database` | Schema inspection, migration status, query analysis | G5, G4 |
| 22 | `check_runtime` | Query Sentry/Datadog for errors, traces, performance data | G6 |

### 3.3 How CEQP enrichment works on any tool

```
Agent calls: find_definition(target="process_order", reason="Need to understand payment flow")
    │
    ├─ UCIL knows exactly what to do — unambiguous tool
    ├─ Dispatches to G1 (Serena lookup), enriches with G3 (knowledge)
    ├─ Returns: definition + signature + callers + callees + doc_comment
    │
    └─ Because reason mentioned "payment flow", bonus context is proactively included:
       conventions: ["Error handling uses thiserror + ModuleError"]
       pitfalls: ["PaymentGateway.charge() not idempotent"]
       related_code: ["src/utils/retry.rs"]
       quality_issues: ["Type warning on line 42"]
       _guidance.suggested_tools: ["trace_dependencies", "blast_radius"]
```

The `reason` parameter is what drives enrichment, not which tool was called. A `find_definition` call with a rich reason gets conventions, pitfalls, and related code. The same call without a reason gets just the definition. This works identically across all 22 tools.

### 3.4 Response budget across hosts

Tool **definitions** are always loaded (~22K tokens). Tool **responses** are fitted to host-specific output limits:

| Host | Max tool output | UCIL response strategy |
|------|----------------|----------------------|
| Claude Code | 25K tokens (configurable) | Full enriched responses with all bonus context |
| Codex CLI | 10KB / 256 lines | Aggressive compression, pagination tokens, summary-first |
| Cursor | Configurable | Full enriched responses (22 tools well under 40-tool cap) |
| Cline/Roo Code | Configurable per-server | Full enriched responses |
| Aider | N/A (prompt-based) | Pre-compressed via HTTP bridge |
| Ollama/local | 4K–32K context | Signature-only mode, minimal bonus context |

---

## 4. Tool inventory and groups

### 4.1 Group 1 — Structural intelligence

AST-level parsing, symbol navigation, type information, call chains.

| Tool | Role | Interface | Priority |
|------|------|-----------|----------|
| **tree-sitter** | Foundation parser. AST for 248+ languages. Incremental re-parsing. | Library (Rust bindings) | P0 |
| **Serena** | LSP-powered symbol navigation. find_symbol, find_references, go_to_definition, replace_symbol_body for 40+ languages. 17K+ stars, battle-tested. | MCP server (stdio) | P0 |
| **ast-grep** | Structural pattern matching. Code-like patterns for AST search. | CLI + MCP server | P0 |
| **LSP diagnostics bridge** | Complements Serena: pulls type errors, lint warnings, call/type hierarchies from the same LSP servers Serena manages. | Internal daemon component | P0 |
| **SCIP** | Compiler-accurate cross-repo symbol index. 15+ languages. | CLI → SQLite | P1 |
| **Joern** | Code Property Graphs. Inter-procedural data flow, taint analysis. | CLI (Scala) | P2 |
| **Codegen/Graph-sitter** | Programmatic code manipulation with auto reference updates. Python/TS/JS. | Python library | P2 |

**Serena + LSP diagnostics bridge — division of labor**: Serena is the primary interface for symbol navigation and refactoring (it already wraps pyright, rust-analyzer, typescript-ls, gopls, and 40+ other LSP servers via MCP). The diagnostics bridge is a lightweight daemon-internal component that taps into the same LSP server instances Serena manages to extract diagnostics (type errors, lint warnings) and hierarchies (call/type) that Serena doesn't surface well. When Serena is active, no duplicate LSP processes are spawned. When Serena is unavailable (degraded mode), the bridge spawns its own LSP servers.

### 4.2 Group 2 — Search

Finding code by text, structure, or meaning.

| Tool | Role | Interface | Priority |
|------|------|-----------|----------|
| **ripgrep** | Fast text search baseline. Respects .gitignore. | CLI | P0 |
| **Probe** | AST-aware search. Complete function bodies. Token budgeting. Session dedup. | CLI + MCP server | P0 |
| **LanceDB** | Embedded vector store for semantic code search. Zero-config, local. | Library (Rust SDK) | P0 |
| **CodeRankEmbed / Qwen3-Embedding** | Code embedding models. Local inference via ONNX Runtime. | Rust `ort` crate | P0 |
| **Zoekt** | Trigram-indexed search for multi-repo scale. | HTTP API | P1 |
| **codedb** | Zig-based code intelligence. 5 in-memory indexes, sub-ms warm queries. 16 MCP tools. | MCP server | P2 |

**Embedding model strategy**: Default: **CodeRankEmbed** (137M params, MIT license, 8K context). This is the primary model for most users — CPU-friendly, 50-150 embeddings/sec, ~137MB with Int8 quantization, negligible accuracy loss. GPU upgrade path: **Qwen3-Embedding-8B** (Apache 2.0, 80.68 MTEB-Code, 32K context, Matryoshka dimension support 32–7168). Substantially better retrieval quality but requires GPU for reasonable throughput. Configured via `ucil.toml`: `embedding_model = "coderankembed"` (default) or `"qwen3-embedding"`. Inference via Rust `ort` crate (ONNX Runtime) for 3-5x faster than Python with 60-80% less memory.

### 4.3 Group 3 — Knowledge and memory

Persistent understanding across sessions. Facts about the codebase, decisions, temporal evolution.

| Tool | Role | Interface | Priority |
|------|------|-----------|----------|
| **Codebase-Memory MCP** | Code knowledge graph. 66 languages, 14 tools. Sub-ms structural queries. | MCP server | P0 |
| **Mem0** | Agent memory. Three-tier (user/session/agent). Self-editing conflict resolution. | Python SDK + MCP server | P0 |
| **Graphiti** | Bi-temporal knowledge graph. Tracks fact evolution with validity windows. P95 ~300ms. | Python SDK + MCP server | P1 |
| **Arc Memory** | Git-native knowledge graph. PRs, commits, issues, ADRs linked to code. | CLI + MCP server | P1 |
| **Cognee** | Local-first cognitive memory. Graph + vector hybrid. | Python SDK | P2 |
| **Letta** | Stateful agent framework. Self-editing memory blocks. | REST API | P2 |
| **ConPort** | Project-specific knowledge graphs with RAG. | MCP server | P2 |
| **mcp-memory-service** | Lightweight MCP-native memory. Inter-agent messaging via tags. | MCP server | P2 |

### 4.4 Group 4 — Architecture and dependencies

How components connect, what depends on what, what breaks when something changes.

| Tool | Role | Interface | Priority |
|------|------|-----------|----------|
| **CodeGraphContext** | Dependency graph, blast radius analysis. MIT license. 14 languages. | MCP server | P0 |
| **GitNexus** | Blast radius, git-diff impact mapping. (PolyForm NC license) | MCP server | P0 (alt) |
| **dependency-cruiser** | JS/TS dependency validation. Rule-based. | CLI | P0 (JS/TS) |
| **LSP diagnostics bridge** | Call hierarchy and type hierarchy across all languages. Complements Serena. | Internal | P0 |
| **Axon** | Knowledge graphs with dependency analysis, call chains, clusters. | MCP server | P1 |
| **Semgrep** | Static analysis. 5000+ security rules, 30+ languages. | CLI + MCP server | P1 |
| **deptry** | Python dependency hygiene. | CLI | P1 (Python) |
| **skott** | JS/TS dependency graph. 7x faster than Madge. | CLI + library | P2 |
| **Nx** | Monorepo dependency graph. `nx affected` for change impact. | CLI | P2 |
| **Bazel MCP** | Build/test/query for Bazel-based projects. | MCP server | P2 |

### 4.5 Group 5 — Context and documentation

Packing codebase context for LLMs, library docs, API docs, project documentation.

| Tool | Role | Interface | Priority |
|------|------|-----------|----------|
| **Repomix** | Repo-to-context packing. tree-sitter compression. 70% token reduction. | CLI + MCP server | P0 |
| **Context7** | Real-time library docs for 9000+ libraries. Eliminates API hallucination. | MCP server | P0 |
| **Aider repo-map** (reimplemented) | PageRank-based symbol selection within token budgets. | Library (internal, Rust) | P0 |
| **Open Context** | High-perf docs for Go, npm, Python, Rust, Docker, K8s, Terraform. | MCP server (Go binary) | P1 |
| **AWS OpenAPI MCP** | Dynamic MCP tools from any OpenAPI v2/v3 spec. | MCP server | P1 |
| **mcp-graphql-enhanced** | Filtered GraphQL introspection for large schemas. | MCP server | P1 |
| **Code2Prompt** | Rust alternative for context packing. TUI + template system. | CLI | P2 |
| **Outline** | Team knowledge base with built-in MCP server. | MCP server | P2 |

### 4.6 Group 6 — Platform and git

Integration with development platforms.

| Tool | Role | Interface | Priority |
|------|------|-----------|----------|
| **GitHub MCP Server** | 80+ tools for full GitHub platform including Actions. ~25K stars. | MCP server | P0 |
| **Git MCP** | Repository operations, search, diff, log, blame. | MCP server | P0 |
| **Filesystem MCP** | Secure file read/write with access controls. | MCP server | P0 |
| **Playwright MCP** | Browser automation via accessibility tree. | MCP server | P1 |
| **Docker MCP** | Container management. 33 tools with three-tier safety. | MCP server | P1 |
| **Terraform MCP** | IaC provider docs, module search. Official HashiCorp. | MCP server | P2 |
| **kubectl MCP** | 253 Kubernetes tools. CNCF Landscape. | MCP server | P2 |

### 4.7 Group 7 — Quality and security (NEW)

Linting, type checking, static analysis, security scanning, secrets detection, SBOM.

This is the most significant addition to UCIL v2. The research revealed a rich ecosystem of quality tools with MCP servers, and agents that write code need constant quality feedback.

| Tool | Role | Interface | Priority |
|------|------|-----------|----------|
| **LSP diagnostics bridge** | Universal type errors + lint warnings for all languages via LSP. Complements Serena. | Internal | P0 |
| **ESLint** (built-in MCP) | JS/TS linting. `eslint --mcp` or `npx @eslint/mcp`. | Built-in MCP | P0 (JS/TS) |
| **Ruff MCP** | Python linting + formatting. Extremely fast (Rust). | MCP server | P0 (Python) |
| **Semgrep MCP** | Multi-language SAST. 5000+ rules. | MCP server | P0 |
| **SonarQube MCP** | Enterprise code quality. 19+ languages. Official. ~600 stars. | MCP server | P1 |
| **Snyk MCP** | 11 tools: SAST, SCA, IaC scan, container scan, SBOM, AIBOM. | MCP server | P1 |
| **RuboCop** (built-in MCP) | Ruby linting. `rubocop --mcp`. | Built-in MCP | P1 (Ruby) |
| **OSV MCP** | Free CVE database. npm, PyPI, Go, Maven, NuGet. | MCP server | P1 |
| **Trivy** | Container + filesystem vulnerability scanning. | CLI (wrappable) | P1 |
| **TruffleHog** | Secrets detection. 800+ types. Live API verification. | CLI | P1 |
| **Gitleaks** | Fast secrets scanning. MIT license. | CLI | P1 |
| **cargo clippy** (via LSP) | Rust linting via rust-analyzer diagnostics. | Diagnostics bridge | P0 (Rust) |
| **Biome** | JS/TS/JSON linting + formatting. MCP RFC active. | CLI (MCP pending) | P2 |
| **DeepSource / Codacy** | AI-powered code quality platforms with MCP. | MCP server | P2 |

**How G7 integrates with UCIL's core tools**:
- `check_quality` routes to G7 tools based on language and check type
- `get_context_for_edit` proactively includes lint/type warnings in bonus context when editing a file with known issues
- `review_changes` uses G7 to validate diffs against quality standards
- The feedback loop (Section 8) tracks whether agents fix quality issues UCIL surfaces

### 4.8 Group 8 — Testing and CI (NEW)

Test execution, coverage analysis, test generation, CI/CD integration.

| Tool | Role | Interface | Priority |
|------|------|-----------|----------|
| **test-runner-mcp** | Unified test execution: Pytest, Jest, Go, Rust, Bats, Flutter. | MCP server | P0 |
| **mcp-pytest-runner** | Deep Pytest: hierarchical discovery, selective re-runs, node IDs. | MCP server | P0 (Python) |
| **GitHub Actions** (via GitHub MCP) | Trigger workflows, check build status, fetch logs. | MCP server | P0 |
| **pytest-mcp** | Pytest plugin that exposes test results as MCP resources. | Pytest plugin | P1 |
| **Coverage analysis** | Parse coverage reports (lcov, cobertura) for changed files. | Internal | P1 |
| **Mutation testing** | AST-based mutation engine for test quality validation. | Internal + Stryker/PIT | P2 |

**How G8 integrates with UCIL's core tools**:
- `check_quality` routes to `run_tests` when checking code that has associated tests
- `review_changes` proactively identifies untested changed functions
- `get_context_for_edit` includes relevant test files in the context when editing source code
- The `tests_to_update` bonus context field draws from G8's test discovery

---

## 5. Intra-group fusion strategy

**Philosophy**: Every tool in a group runs on every relevant query. Results are fused, not cascaded. A tool is skipped only if it is genuinely unavailable (not installed, crashed, health check failed). The goal is maximum information density — let the fusion layer reconcile and enrich, not the tool selection layer filter.

### 5.1 G1: Structural — All tools parallel, fuse everything

```
Query → ALL of the following run in parallel:
        ├─ tree-sitter parse (<1ms, always available)
        ├─ Serena: find_symbol, go_to_definition, find_references, get_hover_info
        ├─ ast-grep: structural pattern match for the query target
        ├─ LSP diagnostics bridge: type errors, call hierarchy, type hierarchy
        ├─ SCIP index lookup (if available): compiler-accurate cross-repo symbols
        └─ Joern data flow analysis (if available): inter-procedural taint paths
      → Fusion: merge all results by location, union unique information
        - Serena provides precise definition + hover docs
        - tree-sitter provides AST structure + scope info
        - ast-grep provides pattern context + surrounding code
        - Diagnostics bridge provides type errors + call chain
        - SCIP provides cross-repo references
        - Joern provides data flow paths
      → Agent enrichment: Interpreter agent synthesizes a unified understanding
        from all tool outputs — not just "here's the definition" but
        "here's what this code does, how it connects, and what to watch out for"
      → Output: {symbol, definition, signature, type_info, hover_doc, ast_context,
                 pattern_matches[], diagnostics[], call_chain, data_flow_paths[],
                 cross_repo_refs[], agent_narrative}
```

**No cascade, no fallback ordering.** Serena, tree-sitter, ast-grep, diagnostics bridge, SCIP, and Joern all run simultaneously. If Serena is unavailable, the other 5 tools still produce rich results. If only tree-sitter is available, that's the degraded mode — but it's never the preferred path.

### 5.2 G2: Search — All engines parallel → weighted RRF

```
Query → ALL search engines run in parallel:
        ├─ ripgrep (text search, always available)
        ├─ Probe (AST-aware search with function bodies)
        ├─ LanceDB (semantic vector search via CodeRankEmbed)
        ├─ Zoekt (trigram index, if available)
        └─ codedb (5 in-memory indexes, if available)
      → Normalize to (file, start_line, end_line, snippet, score)
      → Weighted RRF: Probe 2.0, ripgrep 1.5, LanceDB 1.5, Zoekt 1.0, codedb 1.0
      → Dedup: overlapping line ranges (Jaccard > 0.7) merged, highest-ranked kept
      → Session dedup: don't return same code block twice in a session
      → Output: ranked results with provenance (which engines found each result)
```

### 5.3 G3: Knowledge — All knowledge stores queried → temporal merge

```
Query → ALL knowledge stores queried in parallel:
        ├─ Codebase-Memory MCP (code knowledge graph)
        ├─ Mem0 (agent memory store)
        ├─ Graphiti (temporal knowledge graph)
        ├─ Arc Memory (git-based knowledge, ADRs)
        ├─ Cognee (document understanding)
        └─ ConPort (conversation-persisted context)
      → Merge by entity/topic:
        - Same entity, same fact → highest-confidence version
        - Same entity, conflicting facts → Graphiti temporal validity wins,
          else more recent fact wins, else surface conflict to Conflict agent
        - Different entities → union all
      → Agent enrichment: Memory Curator agent synthesizes narrative context
        from all stores — not just raw facts but "here's the story of this code"
      → Output: knowledge entries with provenance, confidence, temporal metadata
```

### 5.4 G4: Architecture — All analysis tools → union + agent synthesis

```
Query → ALL architecture tools run in parallel:
        ├─ CodeGraphContext / GitNexus (code graph analysis)
        ├─ dependency-cruiser / deptry (dependency extraction)
        ├─ Semgrep (architectural pattern matching)
        ├─ LSP diagnostics bridge (call hierarchy, type hierarchy)
        ├─ Nx / Bazel query (monorepo module boundaries, if available)
        └─ KG architectural relations (from G3)
      → Union all dependency edges from all sources
      → Conflict: parse actual imports as ground truth, merge inferred relations
      → Blast radius: BFS from changed nodes, weight by coupling strength
      → Agent enrichment: Architecture Narrator produces "here's how this fits
        in the system and what changing it would ripple through"
      → Output: dependency subgraph + blast radius + module boundaries +
                architectural narrative + static analysis findings
```

### 5.5 G5: Context — All context sources → quality-maximalist assembly

```
Query → ALL context sources run in parallel:
        ├─ Aider-style repo-map (PageRank, 50x bias toward relevant files)
        ├─ Context7 (library docs for imported external libraries)
        ├─ Open Context (platform docs for Docker/K8s/Terraform references)
        ├─ Repomix (full file packing for comprehensive context)
        ├─ OpenAPI MCP (API specs for referenced endpoints)
        └─ GraphQL MCP (schema for referenced GraphQL types)
      → Assembly: rank all context by relevance using PageRank scores
      → Include relevant content at full detail — no artificial compression
      → Exclude irrelevant content entirely — don't pad with unrelated modules
      → For complex tasks: include more examples, full function bodies,
        test files, related modules, documentation
      → Session dedup: skip content the agent already has (files_in_context)
      → Output: comprehensive but focused context package
      → _meta.token_count included so host adapter can trim if needed
```

**No artificial budget caps forcing compression. But also no bloat.** Relevance-ranked content at full detail, irrelevant content excluded. The host adapter layer (Section 9) handles any further trimming for constrained hosts like Codex CLI.

### 5.6 G6: Platform — All platform tools → aggregation

```
Query → ALL relevant platform tools run in parallel:
        ├─ Git MCP (commit, blame, diff, log)
        ├─ GitHub MCP (PR, issue, Actions, reviews, comments)
        ├─ Filesystem MCP (raw file content, directory structure)
        ├─ Docker MCP (container state, Dockerfile analysis)
        ├─ Terraform MCP (infrastructure state, plan output)
        ├─ kubectl MCP (K8s resource state)
        └─ Playwright MCP (browser testing context)
      → Simple aggregation — no complex fusion needed
      → Output: platform context with full detail
```

### 5.7 G7: Quality — All quality tools → severity-weighted merge

```
Query → ALL quality/security tools run in parallel:
        ├─ LSP diagnostics bridge (type errors, lint warnings) [always]
        ├─ ESLint MCP / Ruff / RuboCop / clippy (language linters) [all that apply]
        ├─ Semgrep (SAST, 5000+ rules)
        ├─ Snyk (SCA, dependency vulnerabilities)
        ├─ SonarQube (code quality metrics)
        ├─ Trivy (container + dependency scan)
        ├─ OSV (open source vulnerability DB)
        ├─ TruffleHog / Gitleaks (secrets detection)
        └─ All other installed quality plugins
      → ALL tools run, not conditionally — don't skip Snyk because
        "no dependency changes detected". Run it anyway. Let the results
        speak for themselves.
      → Merge by severity: critical > high > medium > low
      → Dedup: same file+line+category = keep highest-severity with merged details
      → Agent enrichment: quality agent narrativizes findings into actionable guidance
      → Output: issues[] with severity, category, file, line, fix_suggestion, narrative
```

### 5.8 G8: Testing — All test tools → comprehensive test intelligence

```
Query → ALL test tools run in parallel:
        ├─ test-runner-mcp (discover + execute tests)
        ├─ pytest-runner (Python-specific test intelligence)
        ├─ GitHub Actions MCP (CI status, recent failures)
        ├─ Coverage tools (line/branch coverage for changed code)
        ├─ Mutation testing (if configured, run on changed functions)
        └─ KG tested_by relations (which tests cover which code)
      → Discover ALL relevant tests via ALL methods:
        1. Convention-based: src/foo.rs → tests/test_foo.rs
        2. Import-based: which test files import the changed module
        3. KG-based: tested_by relations in knowledge graph
        4. Coverage-based: which tests historically cover the changed lines
      → Execute selected tests, parse results + coverage
      → Output: test_results[], coverage_for_changed_lines, untested_functions[],
                ci_status, mutation_score (if available)
```

---

## 6. Cross-group fusion and the unified pipeline

### 6.1 Complete query pipeline

```
Agent query arrives via MCP (with CEQP context: reason, current_task, files_in_context)
    │
    ├─ [OpenTelemetry] Start root span: ucil.query.{tool_name}
    │
    ├─ [Session State] Update session with call record, infer task/domain
    │
    ├─ [Query Interpreter Agent] (DEFAULT — runs on every query when agents enabled)
    │      Analyzes reason + target + session context
    │      Produces rich QueryPlan: intent, domains, sub-queries, knowledge gaps
    │      Deterministic classifier used as FALLBACK when no LLM configured
    │
    ├─ [Clarification Agent] If query is genuinely ambiguous and no session
    │      context resolves it → MCP Elicitation to ask the user
    │
    ├─ [Cache Check] L0 in-memory (content-addressed hash of query + file state)
    │      Hit? → Return cached result (span: ucil.cache.hit)
    │
    ├─ [Parallel Executor] Fan out to ALL relevant groups concurrently
    │      Per-group timeout: 5s default. Failed groups → empty results + _meta.degraded_groups.
    │      ALL tools within each group run in parallel.
    │      Each group call traced: ucil.group.{group_name}
    │
    ├─ [Cross-Group RRF Fusion] Apply query-type-specific weights (see 6.2)
    │      Normalize to unified ranked list. Cross-group dedup.
    │      Weighted RRF: Σ w_g × 1/(k + rank_g(d)), k tunable (default 60)
    │
    ├─ [Conflict Resolution Agent] (DEFAULT — not just when confidence < 0.6)
    │      Reasons about disagreements between groups with full semantic understanding.
    │      Source authority as soft guidance: LSP/AST → SCIP → Dep tools → KG → Text
    │      Deterministic source-authority ranking as FALLBACK when no LLM configured.
    │      Unresolvable: include both with confidence metadata
    │
    ├─ [Synthesis Agent] (DEFAULT — runs on every query when agents enabled)
    │      Produces coherent, rich narrative from all fused results.
    │      Not just "here's the data" but "here's what this means, what to watch for,
    │      and how it connects to your task."
    │      In host-passthrough mode: embeds synthesis instructions in _synthesis_hint.
    │
    ├─ [Bonus Context] Include ALL relevant bonus context:
    │      conventions, pitfalls, quality issues, related code, tests to update,
    │      blast radius, historical PR context, security findings, examples.
    │      For complex tasks: MORE context, MORE examples, full function bodies.
    │      No token budget cap — include everything relevant.
    │      _meta.token_count provided so host adapter can trim if needed.
    │
    ├─ [Adaptive Guidance] (CEQP _guidance section)
    │      Context quality score, hints for improvement, suggested next calls.
    │
    ├─ [Feedback Analyzer] Compare with previous response's bonus context
    │      Track whether agent used pitfalls, followed conventions, read related code
    │      Boost/decay importance scores accordingly
    │
    └─ [Response + OTel span end] Full quality-maximalist response returned via MCP
```

### 6.2 Query-type weight matrix

Weights multiplied into RRF formula: `w_g × 1/(k + rank)`, k = 60 (tunable).

| Query type | G1 | G2 | G3 | G4 | G5 | G6 | G7 | G8 |
|-----------|:---:|:---:|:---:|:---:|:---:|:---:|:---:|:---:|
| `understand_code` | 2.0 | 1.0 | **2.5** | 1.5 | 2.0 | 0.5 | 1.0 | 0.5 |
| `find_definition` | **3.0** | 1.5 | 1.0 | 0.5 | 0.5 | 0.0 | 0.5 | 0.0 |
| `find_references` | **3.0** | 2.0 | 0.5 | 1.0 | 0.5 | 0.0 | 0.5 | 0.0 |
| `search_code` | 1.5 | **3.0** | 0.5 | 0.5 | 1.0 | 0.0 | 0.0 | 0.0 |
| `get_context_for_edit` | 2.0 | 2.0 | 1.5 | 1.5 | **2.5** | 0.5 | 1.5 | 1.0 |
| `trace_dependencies` | 1.5 | 0.5 | 1.5 | **3.0** | 0.5 | 0.0 | 0.5 | 0.0 |
| `blast_radius` | 1.5 | 0.5 | 1.5 | **3.0** | 0.5 | 0.5 | 1.0 | 1.0 |
| `review_changes` | 1.5 | 1.0 | 1.5 | 1.5 | 0.5 | 1.0 | **3.0** | **2.5** |
| `check_quality` | 1.0 | 0.5 | 1.0 | 1.5 | 0.5 | 0.5 | **3.0** | **2.5** |
| `remember` | 0.0 | 0.0 | **3.0** | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 |

### 6.3 Response assembly — full detail for relevant content, nothing wasted

```
Input: ranked_results[], bonus_context, agent_narratives
Output: enriched response — every token deserved

1. Session dedup: remove results the agent already has (files_in_context)
2. Relevance filtering: exclude results below relevance threshold (score < 0.1)
   — irrelevant content is excluded, not compressed
3. Include relevant results at FULL detail — full function bodies, full docs, full context
   — no artificial caps forcing signature-only or snippet-only
4. Append relevant bonus context: conventions that apply, pitfalls for this code,
   quality issues in these files, tests that cover this code, blast radius if editing
   — skip bonus categories with zero relevant entries
5. For complex tasks (inferred from reason): proportionally more context:
   - Full source of directly related modules
   - Code examples of the pattern in use
   - Complete test files for the changed code
   - Full PR descriptions for relevant historical changes
6. For simple lookups (find_definition with clear target): lean response:
   - Definition + signature + hover doc + immediate callers
   - Relevant conventions and pitfalls still included (they're cheap and high-value)
7. Agent synthesis: concise, actionable narrative — not verbose
8. Annotate with _meta.token_count for host adapter trimming
9. Format: Markdown with file paths as headers, full code blocks
```

**No artificial budget caps. No lossy compression of relevant content. But also no bloat — irrelevant results are excluded, session dedup avoids repetition, and simple queries get lean responses.** Every piece of information gets the tokens it deserves. The host adapter layer (Section 9) handles fitting into host-specific limits via trimming from the bottom of the relevance ranking.

---

## 7. Agent layer

### 7.1 Philosophy — agents by default

Agents are the primary intelligence layer, not a selective fallback. Every query benefits from semantic reasoning: richer interpretation of intent, deeper synthesis of cross-tool results, more nuanced conflict resolution, and contextual narratives that connect code to its history and purpose. The deterministic path exists only as a fallback when no LLM provider is configured (`provider = "none"` in config).

### 7.2 Where agents sit

Between the MCP server and the orchestration engine. Query-path agents run on **every query** (when agents enabled) — before (interpret), during (resolve conflicts), and after (synthesize). Background agents continuously enrich the knowledge graph without blocking queries.

### 7.3 LLM provider abstraction

```rust
trait LlmProvider {
    async fn complete(&self, request: LlmRequest) -> Result<LlmResponse>;
    fn is_available(&self) -> bool;
    fn cost_per_1k_tokens(&self) -> f64;
    fn max_context_tokens(&self) -> usize;
    fn name(&self) -> &str;
}
```

**Supported providers** (configured during `ucil init`):

| Provider | Use case | Latency | Cost |
|----------|----------|---------|------|
| **Ollama local** | Default. Fully offline. llama3, qwen2.5-coder, deepseek-coder. | 500ms–5s | Free |
| **Claude API** | Highest quality synthesis, complex reasoning. | 1–3s | ~$3/$15 per M tokens |
| **OpenAI API** | Alternative cloud. | 1–3s | ~$2.50/$10 per M tokens |
| **Host passthrough** | Delegates reasoning to the calling agent's LLM. Zero cost. | Variable | Free |
| **None** | Deterministic only. No agent features. Fastest but least rich. | 0ms | Free |

**Host passthrough**: When UCIL is called by Claude Code, instead of running its own LLM, it embeds synthesis instructions in the response: key facts, conflicts to address, narrative structure hints. The host agent naturally synthesizes this.

### 7.4 The seven internal agents

**Query-path agents** (DEFAULT — run on every query when agents enabled):

1. **Query Interpreter**: Runs on EVERY query. Analyzes reason + target + session context to produce a rich `QueryPlan` with intent, domains, sub-queries, knowledge gaps, and inferred context. This is far richer than keyword-based classification — the agent understands nuance, ambiguity, and implicit needs. When no LLM configured: falls back to deterministic keyword classifier.

2. **Synthesis Agent**: Runs on EVERY query. Takes the fused results from all groups and produces a coherent, rich narrative — not just data but understanding. "Here's what this code does, how it connects to your task, what patterns to follow, and what to watch out for." In host-passthrough mode: embeds synthesis instructions in `_synthesis_hint` field.

3. **Conflict Mediator**: Runs whenever ANY conflict is detected between group outputs. Reasons about disagreements with full semantic understanding rather than rigid source-authority rules. Source authority (LSP > SCIP > KG > text) serves as soft guidance to the agent, not as a deterministic rule.

4. **Clarification Agent**: Uses MCP Elicitation protocol to ask users for clarification when queries are genuinely too ambiguous for even the Interpreter agent to resolve. Progressive reduction: week 1 ~40% queries need clarification → month 3 <5% as knowledge graph grows.

**Background agents** (async, never block queries, use fallback LLM provider):

5. **Convention Extractor**: Runs every N commits (default: 10). Samples 50–100 files, feeds to LLM to discover coding patterns. Stores in `conventions` table with evidence counts and confidence scores.

6. **Memory Curator**: Runs after each agent session + every 24h. Deduplicates, merges, and promotes agent observations. Clusters by semantic similarity, resolves conflicts, prunes low-importance entries.

7. **Architecture Narrator**: Runs every 50 commits or on demand. Produces human-readable architectural narrative of the project. <2000 token overview.

### 7.5 Two-mode operation

**Mode 1 — Agent-rich (default when LLM provider configured)**:
All four query-path agents run on every query. This adds 200ms–5s latency but produces dramatically richer, more contextual results. The Interpreter agent's understanding propagates through the entire pipeline — better group selection, better fusion, better synthesis.

**Mode 2 — Deterministic fallback (when `provider = "none"`)**:
No agents run. Keyword-based classifier assigns query_type. Source-authority rules resolve conflicts. No synthesis narrative. Results are still fused via RRF but without semantic enrichment. This mode is functional but produces much less rich output.

**Background agents run in BOTH modes** (if any LLM provider is configured, even just Ollama).

---

## 8. Context-Enriched Query Protocol (CEQP)

### 8.1 The problem

Without context, agents make 6+ narrow tool calls per task. With CEQP, a single enriched call replaces all of them because UCIL understands WHY the agent is asking and proactively includes everything needed.

### 8.2 Universal parameters on every tool

Every UCIL MCP tool gains these parameters:

- **`reason`** (string, optional but strongly encouraged): WHY the agent is making this call. The richer the reason, the more UCIL proactively includes.
- **`current_task`** (string, optional): One-line summary of the user's overall task.
- **`files_in_context`** (string[], optional): Files the agent already has. UCIL avoids repeating them.
- **`token_budget`** (integer, optional, soft hint only): Advisory token limit. UCIL includes this in `_meta.token_count` so the host adapter can decide what to trim. UCIL itself does NOT enforce this limit — it always returns the fullest possible response.

### 8.3 How reason changes UCIL's behavior

The Query Interpreter Agent (or fallback keyword classifier) analyzes the reason to extract: `intent` (add_feature, fix_bug, refactor, understand, review), `domains` (keywords matching KG entities), `planned_action` (edit, read, explain), `knowledge_gaps`, `already_known`. These signals modify: which groups to invoke (weight boosting), bonus context selection (more conventions for refactoring, more security for new code), and synthesis narrative tone (explanatory for understand, actionable for fix_bug).

### 8.4 Enriched response — full detail for what's relevant

Every response includes relevant context at full detail. Irrelevant categories are omitted, not compressed:

```
primary_result: {direct answer to the tool call — full detail, never truncated}
bonus_context: {
  conventions: [{rule, examples, relevance, confidence, tier}]     // Only conventions that apply
  pitfalls: [{warning, source, severity, related_code, tier}]      // Only pitfalls for this code
  quality_issues: [{severity, file, line, message, category, fix}] // Only issues in relevant files
  related_code: [{file, full_snippet, why_relevant}]               // Full code bodies, not snippets
  tests_to_update: [{file, why, test_code}]                        // Include test code when relevant
  test_results: [{test_name, status, duration, output}]            // When tests were actually run
  blast_radius: {files_affected, dependency_chain, summary}        // When editing/refactoring
  history: [{event, date, author, full_description, pr_url}]       // When history is relevant
  security: [{vulnerability, severity, cve_id, fix_available, details}]  // When security findings exist
  examples: [{file, code, description}]                            // For complex/unfamiliar patterns
}
agent_synthesis: {concise, actionable narrative — what this means for the agent's task}
_meta: {token_count, groups_invoked, tools_invoked[], timing_per_group,
        degraded_tools[], indexing_status, otel_trace_id}
_guidance: {context_quality_score, hints[], suggested_tools[], session_stats}
```

**Scaling by task complexity**: Simple `find_definition` with clear target → lean response (definition + signature + conventions + pitfalls). Complex `get_context_for_edit` with rich reason → comprehensive response (full modules, examples, tests, history, architectural context). The `reason` parameter is the primary signal for how much context to include.

### 8.5 Token guidance for host adapters

UCIL does not enforce hard token limits — it provides relevant content at the detail level it deserves. The `_meta.token_count` field tells the host adapter the response size. Results are ranked by relevance, so hosts that need to trim can cut from the bottom without losing the most important content.

| Host | Limit | Host adapter strategy |
|------|-------|----------------------|
| Claude Code | 25K configurable | Usually no trimming needed |
| Codex CLI | 10KB / 256 lines | Trim from bottom of relevance ranking, paginate remainder |
| Cursor | Configurable | Trim low-relevance bonus context if over limit |
| Cline/Roo | Configurable | Usually no trimming needed |
| Aider | N/A | Adapter pre-compresses for prompt injection |
| Ollama | 4K–32K | Trim aggressively from bottom, signatures for low-ranked results |

The separation: UCIL's intelligence layer decides *what's relevant and at what detail*. The host adapter decides *what fits*. Intelligence is never compromised by presentation constraints.

### 8.6 Handling bare tool calls (no reason/context)

1. **Auto-infer from session history** (0ms): If same session had previous calls, infer task/domain.
2. **Agent still runs** (200ms–2s): Even without a reason, the Interpreter agent examines the target + session context to produce a QueryPlan. A bare `find_definition("process_order")` still gets conventions and pitfalls because the agent recognizes the payment domain.
3. **MCP Elicitation** (user interaction): Only for truly ambiguous calls where even the agent can't infer intent.
4. **Learn from subsequent calls** (0ms): Track what agent does next to infer task context.

### 8.7 Feedback loop — learning from agent behavior

UCIL tracks whether its bonus context was actually useful:

```
UCIL returns pitfall: "PaymentGateway not idempotent"
  → Agent's next call references idempotency_key in its reason
  → SIGNAL: pitfall was USEFUL → boost importance by 0.1

UCIL returns quality_issue: "Type error on line 42"
  → Agent's next edit fixes that line
  → SIGNAL: quality issue was ACTED ON → boost that lint rule's priority

UCIL returns convention: "Use thiserror + ModuleError"
  → Agent's next edit creates a ModuleError enum with thiserror
  → SIGNAL: convention was FOLLOWED → boost confidence by 0.05

UCIL returns related_code: "Retry utility in src/utils/retry.rs"
  → Agent reads that file (detected via files_in_context in next call)
  → SIGNAL: related code was USED → boost relevance score
```

---

## 9. Host adapters and Claude Code plugin

### 9.1 Claude Code Plugin (primary distribution)

UCIL ships as a Claude Code plugin — the richest integration surface available. Structure:

```
ucil-plugin/
├── .claude-plugin/
│   └── plugin.json                 # Plugin manifest
├── agents/
│   ├── ucil-lint.md                # Linting subagent (model: haiku, tools: [check_quality])
│   ├── ucil-security.md            # Security scan subagent (model: sonnet, tools: [security_scan])
│   ├── ucil-test.md                # Test runner subagent (model: haiku, tools: [check_quality, run_tests])
│   └── ucil-review.md              # Code review subagent (model: sonnet, tools: [review_changes])
├── skills/
│   ├── analyze/SKILL.md            # Auto-invoked code analysis skill
│   ├── lint/SKILL.md               # /ucil-lint command
│   ├── security-scan/SKILL.md      # /ucil-security command
│   ├── test/SKILL.md               # /ucil-test command
│   ├── review/SKILL.md             # /ucil-review command
│   └── usage-guide/SKILL.md        # How to use UCIL effectively (reason templates)
├── hooks/
│   ├── post-write-analyze.py       # PostToolUse: auto-analyze after Write/Edit/MultiEdit
│   ├── pre-tool-route.py           # PreToolUse: intercept search/grep and route through UCIL
│   └── stop-quality-check.py       # Stop: run quality checks before session ends
├── .mcp.json                       # UCIL MCP server config (project-scoped)
├── .claude/
│   └── rules/
│       └── ucil-conventions.md     # Rules for using UCIL tools effectively
└── README.md
```

**Plugin manifest** (`.claude-plugin/plugin.json`):
```json
{
  "name": "ucil",
  "version": "0.1.0",
  "description": "Unified Code Intelligence Layer — a project-specific brain for coding agents",
  "skills": ["skills/analyze", "skills/lint", "skills/security-scan", "skills/test", "skills/review", "skills/usage-guide"],
  "agents": ["agents/ucil-lint.md", "agents/ucil-security.md", "agents/ucil-test.md", "agents/ucil-review.md"],
  "hooks": "hooks/",
  "mcp": ".mcp.json",
  "rules": ".claude/rules/"
}
```

**Key hooks**:

**PostToolUse hook** (fires after Write/Edit/MultiEdit):
```python
#!/usr/bin/env python3
# hooks/post-write-analyze.py
# Notifies UCIL daemon of file changes for instant re-indexing
# Bypasses file watcher debounce for 0ms change propagation
import json, sys, os, urllib.request

tool_name = os.environ.get("CLAUDE_TOOL_NAME", "")
if tool_name in ("Write", "Edit", "MultiEdit"):
    input_file = os.environ.get("CLAUDE_TOOL_INPUT_FILE_PATH", "")
    if input_file:
        # Notify UCIL daemon directly via Unix socket
        # This triggers immediate AST re-parse and KG update
        try:
            sock_path = os.path.join(os.getcwd(), ".ucil", "daemon.sock")
            # ... notify daemon of changed file
        except:
            pass  # Fail silently — don't block the agent
```

**Skills follow the Agent Skills open standard** (agentskills.io), making them compatible with Claude Code, Codex CLI, Gemini CLI, Cursor, Aider, and 6+ other tools.

### 9.2 Host adapter interface

```rust
trait HostAdapter {
    fn detect() -> bool;
    fn transform_tools(&self, tools: &[ToolDefinition]) -> HostToolSet;
    fn transform_request(&self, raw: &HostRequest) -> UcilQuery;
    fn transform_response(&self, response: &UcilResponse, constraints: &HostConstraints) -> HostResponse;
    fn initialize(&self, project_path: &Path) -> Result<()>;
}
```

### 9.3 Host-specific details

| Host | Detection | Integration surface | Max tool output | Adapter-specific |
|------|-----------|-------------------|----------------|-----------------|
| **Claude Code** | `.claude/` dir or `CLAUDE.md` | Plugin: MCP + Skills + Hooks + Subagents + Rules | 25K tokens | Full plugin distribution. PostToolUse hooks for instant re-indexing. All 22 tools loaded. Subagents for parallel quality checks. |
| **Codex CLI** | `codex` in PATH or AGENTS.md | MCP server + AGENTS.md | 10KB / 256 lines | Aggressive compression. Summary-first responses. Pagination tokens. All 10 tier-1 tools. |
| **Cursor** | `.cursor/` dir | MCP server via `.cursor/mcp.json` | ~40 tool limit | All 22 tools loaded (well under 40 cap). Full enriched responses. |
| **Cline/Roo Code** | VS Code extension context | MCP server via McpHub | Configurable per-server | `alwaysAllow` for trusted tools. Mode-specific filtering (Architect vs Code). |
| **Aider** | `.aider*` config files | HTTP API bridge (no native MCP) + `.aider.conf.yml` | N/A (prompt-based) | Enhanced repo-map augmenting Aider's own. Skills via Agent Skills standard. |
| **Ollama/local** | `localhost:11434` responding | HTTP API (non-MCP agents) | Small (4K–32K) | Aggressive context compression, pre-computed cache, signature-only mode. |

---

## 10. Daemon architecture

### 10.1 Process model

```
ucild (main daemon process, single Rust binary)
├── File watcher thread (notify crate; Watchman if available for large repos)
├── Incremental engine thread (Salsa, processes change events)
├── MCP server thread (handles tool calls via stdio/HTTP)
├── LSP diagnostics bridge (taps into Serena's LSP servers for type errors + hierarchies)
├── Background indexer thread (initial + periodic re-indexing)
├── Background agent scheduler (convention/memory/architecture)
├── Warm processors (4 rule-based, 60s intervals)
├── Compaction agent thread (periodic KG maintenance)
├── Session manager (tracks all active agent sessions)
├── OpenTelemetry exporter (batched span export)
└── Plugin supervisor (manages child processes, HOT/COLD lifecycle)
    ├── serena (stdio MCP server — P0, LSP navigation + refactoring)
    ├── codebase-memory-mcp (stdio MCP server — P0, code knowledge graph)
    ├── probe (stdio MCP server — P0, AST-aware search)
    └── [additional plugins lazy-loaded on first relevant query]
```

### 10.2 Init and session verification — ensuring everything works

**On `ucil init`** (first-time setup for a project):

```
1. Language/framework detection (scan files, detect pyproject.toml, Cargo.toml, etc.)
2. Plugin resolution: map detected languages → required plugins (Serena, linters, etc.)
3. Plugin installation: download/verify all P0 plugins
4. Health check ALL plugins:
   ├─ Start each plugin process
   ├─ Send tools/list MCP request — verify it responds with expected tools
   ├─ Send a test query (e.g., Serena find_symbol) — verify it returns valid data
   ├─ Record: plugin_name, version, tools_available[], languages[], health=OK/DEGRADED/FAILED
   └─ Kill test processes (they'll be properly started on daemon launch)
5. Verify LSP server availability:
   ├─ For each detected language, check if LSP server binary exists
   ├─ Attempt to start and initialize each — verify ServerCapabilities
   └─ Record available LSP capabilities per language
6. Verify embedding model:
   ├─ Load ONNX model, run test embedding, verify dimensions match config
7. LLM provider verification:
   ├─ If Ollama: check ollama running, model pulled, test completion
   ├─ If Claude/OpenAI API: verify API key, test completion
   ├─ If None: skip (deterministic-only mode)
8. Generate .ucil/init_report.json with full health status
9. Print summary: "X/Y plugins healthy, Z languages supported, LLM: {provider}"
```

**On daemon startup** (`ucild`):

```
1. Load config, open/create databases (SQLite, LanceDB, LMDB)
2. Start file watcher
3. Start ALL P0 plugins — verify each responds to tools/list within 5s
4. Start Serena — verify MCP handshake
5. Start LSP diagnostics bridge — connect to Serena's LSP servers
6. If any P0 plugin fails: log error, mark as degraded, continue (don't block startup)
7. Start background indexer — begin initial or incremental index
8. Start MCP server — begin accepting queries
9. Record startup health in _meta.startup_health for first query's response
```

**On session start** (new agent connects):

```
1. Assign session ID, detect branch/worktree
2. Verify ALL plugins still healthy (quick tools/list ping, <100ms each)
3. If any plugin degraded since startup: attempt restart
4. Record session-level health status
5. First query response includes _meta.available_tools[] and _meta.degraded_tools[]
   so the agent knows what's operational
```

### 10.3 Change propagation — how fast the brain updates

**Two detection paths**:
- **Agent edits via PostToolUse hook** (fast path, 0ms): Hook directly notifies daemon via Unix socket. Bypasses file watcher debounce. Daemon starts re-parsing AST immediately.
- **Human edits / git operations** (normal path, 50–200ms): `notify` crate detects via inotify/FSEvents (10–50ms) + debounce window (100ms via `notify-debouncer-full`).

**Five update tiers after detection**:

| Tier | What updates | Latency | Blocks queries? |
|------|-------------|---------|----------------|
| **Tier 1: Instant** | tree-sitter AST re-parse (incremental), symbol extraction, tag cache update, cache invalidation, Serena + diagnostics bridge notification | ~5ms | No |
| **Tier 2: Fast** | Knowledge graph entity update, dependency edge update, hot staging write (raw observations captured immediately) | 50–200ms | No |
| **Tier 2.5: Warm** | Rule-based enrichment: dedup, entity linking, domain tagging, pattern counting. Runs every 60s. | 1–5 min | No |
| **Tier 3: Background** | Vector re-embedding (ONNX inference), Codebase-Memory reindex, full plugin refresh, LSP re-diagnostic | 1–10s | No |
| **Tier 4: Cold** | LLM-powered curation: convention validation, memory synthesis, architecture narration. Consumes warm data. | Minutes–hours | No |

**Stale data guarantee**: UCIL never serves stale data for a query's primary result. The Salsa incremental engine's dependency DAG ensures that if a query depends on a changed file, the dependent computation is lazily recomputed before returning.

---

## 11. Multi-agent, branches, and worktrees

### 11.1 Core design: shared brain, isolated lenses

One daemon per git repository serves all agents across all branches and worktrees. Knowledge flows through three isolation layers:

**Shared layer** (repository-wide, all agents read/write):
- Conventions, agent observations, architectural decisions
- PR/issue/ADR history, architecture narrative
- Pitfalls, patterns, project configuration

**Branch layer** (one per active branch, isolated):
- AST/symbol index, dependency graph, vector embeddings
- Entity facts about specific code (signatures, locations, call chains)
- LSP diagnostics (per-branch, since code differs)

**Session layer** (one per agent connection, fully isolated):
- Call history, task context, dedup tracking, CEQP inference

### 11.2 Storage layout

```
.ucil/
├── daemon.sock / daemon.pid
├── ucil.toml
├── shared/
│   ├── knowledge.db          # SQLite WAL — conventions, decisions, patterns,
│   │                         # + hot_* staging tables + warm_* enriched tables
│   ├── memory.db             # SQLite WAL — agent observations (cold/hardened)
│   └── history.db            # SQLite WAL — PR/issue/ADR links
├── branches/
│   ├── main/
│   │   ├── symbols.db        # SQLite — AST symbols, entities, relations
│   │   ├── vectors/           # LanceDB — code embeddings
│   │   ├── tags.lmdb          # LMDB — mtime-based tag cache
│   │   └── state.json         # Last indexed commit, file hashes
│   ├── feat-retry/
│   │   └── ...
│   └── fix-auth/
│       └── ...
├── sessions/
│   ├── sess_abc123.json       # Per-agent session state
│   └── ...
├── plugins/                   # Plugin-specific data
├── backups/                   # Auto-backups before compaction
├── otel/                      # OpenTelemetry export buffer
└── logs/
    └── daemon.log
```

### 11.3 Knowledge sharing through the shared brain

Knowledge sharing is **pull-based through relevance**, not push-based. No notifications, no inter-agent messaging.

```
FORGE discovers: "PaymentGateway.charge() is not idempotent"
  → Stored in shared/memory.db with domains: ["payments", "idempotency"]

WIRE queries about refund flow (domain: "payments")
  → PaymentGateway pitfall IS relevant → SURFACED as bonus context
  → WIRE benefits from FORGE's discovery without any direct communication
```

Relevance matching uses: domain overlap (0.3 weight), entity overlap (0.4), file proximity (0.2), recency boost (0.1). Threshold: only surface if relevance > 0.3.

### 11.4 Concurrent access

**SQLite WAL mode**: Benchmarked at ~70K ops/sec up to 64 concurrent threads on M1. For UCIL's 5-10 agent scenario, this is comfortably within limits. Critical configuration:

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA busy_timeout = 10000;
PRAGMA mmap_size = 268435456;
```

**All writes use `BEGIN IMMEDIATE`** — the default `BEGIN DEFERRED` causes SQLITE_BUSY errors when write lock upgrade fails, ignoring busy_timeout. This is the #1 cause of unexpected SQLite concurrency failures.

### 11.5 Git worktree support

The `.ucil/` directory lives in the primary worktree. All worktrees share it. Agent in worktree discovers daemon: reads `.git` file → follows `gitdir` pointer → finds primary worktree → connects to `.ucil/daemon.sock`.

---

## 12. Knowledge graph and persistent storage

### 12.1 SQLite knowledge graph schema (shared layer)

```sql
CREATE TABLE entities (
    id INTEGER PRIMARY KEY,
    kind TEXT NOT NULL,          -- 'function', 'class', 'module', 'file', 'type', 'variable'
    name TEXT NOT NULL,
    qualified_name TEXT,
    file_path TEXT NOT NULL,
    start_line INTEGER, end_line INTEGER,
    signature TEXT, doc_comment TEXT, language TEXT,
    -- Bi-temporal (valid time from git commits, transaction time from indexing)
    t_valid_from TEXT, t_valid_to TEXT,
    t_ingested_at TEXT NOT NULL DEFAULT (datetime('now')),
    t_last_verified TEXT,
    -- Importance
    importance REAL DEFAULT 0.5,
    access_count INTEGER DEFAULT 0, last_accessed TEXT,
    source_tool TEXT, source_hash TEXT,
    UNIQUE(qualified_name, file_path, t_valid_from)
);

CREATE TABLE relations (
    id INTEGER PRIMARY KEY,
    source_id INTEGER REFERENCES entities(id),
    target_id INTEGER REFERENCES entities(id),
    kind TEXT NOT NULL,          -- 'calls', 'imports', 'inherits', 'implements',
                                -- 'depends_on', 'tested_by', 'runtime_depends_on'
    weight REAL DEFAULT 1.0,
    t_valid_from TEXT, t_valid_to TEXT,
    t_ingested_at TEXT NOT NULL DEFAULT (datetime('now')),
    source_tool TEXT, source_evidence TEXT, confidence REAL DEFAULT 0.8
);

CREATE TABLE decisions (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL, description TEXT,
    decision_type TEXT,          -- 'adr', 'pr_decision', 'issue_resolution', 'convention'
    related_entities TEXT, source_url TEXT, author TEXT, decided_at TEXT,
    t_ingested_at TEXT NOT NULL DEFAULT (datetime('now')),
    importance REAL DEFAULT 0.7,
    is_superseded INTEGER DEFAULT 0,
    superseded_by INTEGER REFERENCES decisions(id)
);

CREATE TABLE conventions (
    id INTEGER PRIMARY KEY,
    category TEXT NOT NULL,      -- 'naming', 'structure', 'error_handling', 'testing', 'style', 'security'
    pattern TEXT NOT NULL,
    examples TEXT, counter_examples TEXT,
    confidence REAL DEFAULT 0.5,
    evidence_count INTEGER DEFAULT 1,
    t_ingested_at TEXT NOT NULL DEFAULT (datetime('now')),
    last_verified TEXT,
    scope TEXT DEFAULT 'project'  -- 'project' or 'group' (cross-project)
);

CREATE TABLE observations (
    id INTEGER PRIMARY KEY,
    observation TEXT NOT NULL,
    category TEXT,               -- 'bug', 'pattern', 'pitfall', 'optimization', 'context', 'quality'
    related_entities TEXT, domains TEXT,
    session_id TEXT,
    importance REAL DEFAULT 0.5,
    access_count INTEGER DEFAULT 0,
    t_created TEXT NOT NULL DEFAULT (datetime('now')),
    t_last_accessed TEXT
);

-- Quality issues tracking (NEW)
CREATE TABLE quality_issues (
    id INTEGER PRIMARY KEY,
    file_path TEXT NOT NULL,
    line_start INTEGER, line_end INTEGER,
    category TEXT NOT NULL,      -- 'type_error', 'lint', 'security', 'style', 'complexity'
    severity TEXT NOT NULL,      -- 'critical', 'high', 'medium', 'low', 'info'
    message TEXT NOT NULL,
    rule_id TEXT,                -- e.g., 'E0001', 'no-unused-vars', 'CVE-2024-xxxx'
    source_tool TEXT,            -- 'lsp:pyright', 'eslint', 'semgrep', 'snyk'
    fix_suggestion TEXT,
    first_seen TEXT NOT NULL DEFAULT (datetime('now')),
    last_seen TEXT,
    resolved INTEGER DEFAULT 0,
    resolved_by_session TEXT
);

-- Hot staging tables (unchanged from v1)
CREATE TABLE hot_observations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    raw_text TEXT NOT NULL,
    session_id TEXT,
    related_file TEXT,
    related_symbol TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    promoted_to_warm INTEGER DEFAULT 0
);

CREATE TABLE hot_convention_signals (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    pattern_hash TEXT NOT NULL,
    file_path TEXT NOT NULL,
    example_snippet TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    promoted INTEGER DEFAULT 0
);

CREATE TABLE hot_architecture_deltas (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    change_type TEXT NOT NULL,
    file_path TEXT NOT NULL,
    details TEXT,                -- JSON: imports, exports, etc.
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    promoted INTEGER DEFAULT 0
);

CREATE TABLE hot_decision_material (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_type TEXT NOT NULL,   -- 'pr', 'commit', 'issue', 'adr'
    source_url TEXT,
    title TEXT,
    description TEXT,
    affected_files TEXT,         -- JSON array
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    promoted INTEGER DEFAULT 0
);

-- Warm layer tables (unchanged from v1)
CREATE TABLE warm_observations (
    id INTEGER PRIMARY KEY,
    text TEXT NOT NULL,
    domains TEXT,
    related_entities TEXT,
    severity TEXT,
    evidence_count INTEGER DEFAULT 1,
    first_seen TEXT, last_seen TEXT,
    confidence REAL DEFAULT 0.6,
    promoted_to_cold INTEGER DEFAULT 0
);

CREATE TABLE warm_conventions (
    id INTEGER PRIMARY KEY,
    category TEXT NOT NULL,
    pattern_description TEXT NOT NULL,
    examples TEXT,
    evidence_count INTEGER DEFAULT 3,
    confidence REAL DEFAULT 0.5,
    promoted_to_cold INTEGER DEFAULT 0
);

CREATE TABLE warm_architecture_state (
    id INTEGER PRIMARY KEY,
    summary TEXT NOT NULL,
    deltas_incorporated INTEGER,
    last_updated TEXT,
    confidence REAL DEFAULT 0.5,
    promoted_to_cold INTEGER DEFAULT 0
);

CREATE TABLE warm_decisions (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    key_phrases TEXT,
    related_entities TEXT,
    source_material_ids TEXT,
    confidence REAL DEFAULT 0.5,
    promoted_to_cold INTEGER DEFAULT 0
);

-- Feedback tracking (NEW)
CREATE TABLE feedback_signals (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    bonus_type TEXT NOT NULL,    -- 'pitfall', 'convention', 'related_code', 'quality_issue', 'test'
    bonus_id INTEGER,           -- ID in the source table
    signal TEXT NOT NULL,        -- 'used', 'followed', 'ignored', 'fixed'
    timestamp TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Indexes
CREATE INDEX idx_entities_file ON entities(file_path);
CREATE INDEX idx_entities_name ON entities(name);
CREATE INDEX idx_entities_valid ON entities(t_valid_to) WHERE t_valid_to IS NULL;
CREATE INDEX idx_relations_source ON relations(source_id);
CREATE INDEX idx_relations_target ON relations(target_id);
CREATE INDEX idx_observations_category ON observations(category);
CREATE INDEX idx_conventions_category ON conventions(category);
CREATE INDEX idx_quality_file ON quality_issues(file_path) WHERE resolved = 0;
CREATE INDEX idx_quality_severity ON quality_issues(severity) WHERE resolved = 0;
CREATE INDEX idx_hot_obs_promoted ON hot_observations(promoted_to_warm);
CREATE INDEX idx_hot_conv_hash ON hot_convention_signals(pattern_hash);
CREATE INDEX idx_warm_obs_domains ON warm_observations(domains);
CREATE INDEX idx_feedback_session ON feedback_signals(session_id);
```

### 12.2 Vector store (LanceDB, per-branch)

```python
code_chunks_schema = {
    "id": "string",                    # {file_path}:{start_line}:{end_line}
    "file_path": "string",
    "start_line": "int32", "end_line": "int32",
    "content": "string",
    "language": "string",
    "symbol_name": "string",
    "symbol_kind": "string",
    "embedding": "vector[768]",       # CodeRankEmbed default (or 1024 for Qwen3-Embedding)
    "token_count": "int32",
    "file_hash": "string",
    "indexed_at": "timestamp"
}
```

**Chunking**: AST-aware via tree-sitter. Each chunk is a complete function/method/class. Never split mid-function. Max 512 tokens. Larger functions: signature + first-paragraph doc comment.

**Alternative for small projects (<100K vectors)**: sqlite-vec extension provides brute-force kNN (1M 128-dim vectors in 33ms) with zero additional dependencies. Configured via `ucil.toml`:

```toml
[vector_store]
backend = "lancedb"    # "lancedb" or "sqlite-vec"
```

### 12.3 Knowledge tiering — hot/warm/cold processing

Three tiers of progressively deeper processing. Queries merge across all three, so agents always see the latest knowledge — just at different confidence levels.

```
HOT (immediate, 0-5ms):     Raw append. Queryable immediately. Confidence 0.2-0.4.
WARM (1-5 minutes):          Rule-based enrichment. No LLM. Confidence 0.5-0.7.
COLD (hours):                LLM-powered curation. Confidence 0.8-1.0.
```

(Full tiering details as in v1 plan — observations, conventions, architecture, decisions all flow through the same hot→warm→cold pipeline.)

### 12.4 Compaction and decay rules

Runs every 6 hours. Applies to cold tier only:
- **Dead code entities** (t_valid_to set): importance × 0.9/day. Delete at < 0.05.
- **Live entities**: importance × 0.99/day since last_accessed. Flag at < 0.1.
- **Decisions**: importance × 0.999/day, floor at 0.3. Almost never decay.
- **Observations**: importance × 0.95/day. Delete at < 0.1 AND access_count < 3.
- **Quality issues**: resolved issues kept 7 days then deleted. Unresolved never decay.
- **Relations**: Delete if either endpoint deleted.
- **Vector store**: Remove embeddings for deleted/renamed files.
- **Feedback signals**: Aggregate per-bonus-type monthly, delete raw signals >30 days.

---

## 13. Serena + LSP diagnostics bridge — language intelligence

### 13.1 Why Serena plus a diagnostics bridge

Serena (17K+ stars, 40+ languages) is the most mature MCP server for LSP-powered code navigation. It handles LSP server lifecycle, workspace management, and cross-file resolution through well-tested MCP tools (`find_symbol`, `find_referencing_symbols`, `go_to_definition`, `replace_symbol_body`, `get_hover_info`, `search_symbol`). Building a custom replacement would take 3-4 weeks for a worse version.

However, Serena's MCP tools don't expose **diagnostics** (type errors, lint warnings) or **hierarchies** (call hierarchy, type hierarchy). These are valuable for UCIL's quality pipeline (G7) and architecture analysis (G4). The diagnostics bridge fills this gap by connecting to the same LSP servers Serena manages and pulling the missing data.

### 13.2 Architecture — Serena as primary, diagnostics bridge as complement

```
┌──────────────────────────────────────────────────────────┐
│  Serena (MCP plugin, stdio)                              │
│  ├─ find_symbol, find_references, go_to_definition       │
│  ├─ replace_symbol_body, get_hover_info, search_symbol   │
│  └─ Manages LSP server lifecycle for 40+ languages       │
│       pyright · rust-analyzer · gopls · typescript-ls … │
└──────────────────┬───────────────────────────────────────┘
                   │ Shares LSP server instances
┌──────────────────▼───────────────────────────────────────┐
│  LSP diagnostics bridge (daemon-internal)                │
│  ├─ textDocument/diagnostic → G7 quality_issues table    │
│  ├─ callHierarchy/incomingCalls → G4 call graph          │
│  ├─ typeHierarchy/supertypes → G4 type hierarchy         │
│  └─ Connects to Serena's LSP servers (no duplicate procs)│
└──────────────────────────────────────────────────────────┘
```

### 13.3 Diagnostics bridge implementation

```rust
struct LspDiagnosticsBridge {
    /// Connection to Serena's managed LSP servers (via shared socket/pipe)
    /// Falls back to spawning own servers if Serena unavailable
    servers: HashMap<Language, LspConnection>,
    diagnostics_cache: DashMap<PathBuf, Vec<Diagnostic>>,
}

impl LspDiagnosticsBridge {
    /// Pull diagnostics for a file — feeds G7 quality pipeline
    async fn diagnostics(&self, file: &Path) -> Result<Vec<Diagnostic>>;

    /// Get call hierarchy — feeds G4 architecture
    async fn call_hierarchy(&self, file: &Path, line: u32, col: u32) -> Result<CallHierarchy>;

    /// Get type hierarchy — feeds G4 architecture
    async fn type_hierarchy(&self, file: &Path, line: u32, col: u32) -> Result<TypeHierarchy>;
}
```

### 13.4 LSP server sharing with Serena

When Serena is active as a P0 plugin, it manages the LSP server processes. The diagnostics bridge connects to these same server instances — no duplicate processes. Detection:

1. On daemon startup, check if Serena plugin is ACTIVE
2. If active: query Serena for its managed LSP server PIDs/sockets, connect directly
3. If not active (degraded mode): spawn own LSP servers with the same lifecycle management (lazy start, grace period shutdown, health monitoring)

### 13.5 How it feeds other groups

```
Serena         → G1 (Structural):  definition, references, hover, symbol search, refactoring
Diag. bridge   → G4 (Architecture): call hierarchy, type hierarchy
Diag. bridge   → G7 (Quality):     diagnostics (type errors, lint warnings) → quality_issues table
Serena         → Refactoring tools: replace_symbol_body, rename
```

---

## 14. Plugin system

### 14.1 Plugin manifest (`plugin.toml`)

```toml
[plugin]
name = "semgrep"
version = "1.0.0"
description = "Static analysis with 5000+ security rules"
category = "quality"             # structural, search, knowledge, architecture, context, platform, quality, testing
license = "MIT"

[capabilities]
provides = ["sast_scan", "security_rules", "code_quality"]
languages = ["python", "typescript", "rust", "go", "java", "c", "cpp"]
activation.on_language = ["python", "typescript", "rust"]
activation.on_tool = ["check_quality", "security_scan", "review_changes"]
activation.eager = false

[transport]
type = "stdio"                   # stdio | http | library
command = "semgrep"
args = ["--mcp"]
health_check = "tools/list"
restart_on_crash = true
max_restarts = 3

[resources]
memory_mb = 200
startup_time_ms = 3000
typical_query_ms = 500

[lifecycle]
hot_cold = true                  # Enable HOT/COLD lifecycle management
idle_timeout_minutes = 10        # Shut down after 10 minutes idle
```

### 14.2 Plugin lifecycle

States: DISCOVERED → REGISTERED → LOADING → ACTIVE → IDLE → STOPPED → ERROR

**HOT/COLD lifecycle** (new): Plugins with `hot_cold = true` auto-transition to IDLE after `idle_timeout_minutes` of no calls. When the next relevant query arrives, the plugin restarts (IDLE → LOADING → ACTIVE). This prevents 10+ MCP server processes sitting idle and consuming 300-500MB RAM.

### 14.3 Adding a new plugin

1. Create `plugins/{name}/plugin.toml`
2. Ensure binary is in PATH or provide absolute path
3. Run `ucil plugin install {name}` — validates manifest, tests health
4. Daemon discovers on next startup or hot-reload via `ucil plugin reload`
5. No core code changes. Plugin manager auto-maps capabilities to tool groups.

---

## 15. Observability and telemetry

### 15.1 Why observability from day one

UCIL orchestrates 60+ tools with multi-stage fusion. Without observability, debugging why a query returned unexpected results is nearly impossible. OpenTelemetry spans trace every step.

### 15.2 Span hierarchy

```
ucil.query.find_definition                  # Root span (per tool call)
├── ucil.ceqp.parse_reason                  # Reason parsing
├── ucil.classifier.classify                # Intent classification
├── ucil.cache.check                        # Cache lookup (with hit/miss attribute)
├── ucil.executor.parallel                  # Parallel group execution
│   ├── ucil.group.structural               # G1 execution
│   │   ├── ucil.tool.treesitter.parse      # tree-sitter parse
│   │   ├── ucil.tool.serena.find_symbol    # Serena MCP call
│   │   ├── ucil.tool.astgrep.search        # ast-grep search
│   │   └── ucil.tool.lsp_diag.diagnostics  # Diagnostics bridge
│   ├── ucil.group.search                   # G2 execution
│   │   ├── ucil.tool.probe.search
│   │   ├── ucil.tool.ripgrep.search
│   │   └── ucil.tool.lancedb.vector_search
│   ├── ucil.group.quality                  # G7 execution
│   │   ├── ucil.tool.lsp.diagnostics
│   │   └── ucil.tool.semgrep.scan
│   └── ...
├── ucil.fusion.rrf                         # Cross-group fusion
├── ucil.conflict.resolve                   # Conflict resolution
├── ucil.context.compile                    # Context compilation
├── ucil.bonus.select                       # Bonus context selection
├── ucil.feedback.analyze                   # Feedback analysis
└── ucil.response.format                    # Response formatting
```

### 15.3 Key metrics

| Metric | Type | Description |
|--------|------|-------------|
| `ucil.query.duration` | Histogram | Total query time (p50/p95/p99) |
| `ucil.query.count` | Counter | Queries per tool type |
| `ucil.cache.hit_rate` | Gauge | L0/L1/L2 cache hit rates |
| `ucil.group.duration` | Histogram | Per-group execution time |
| `ucil.tool.duration` | Histogram | Per-tool execution time |
| `ucil.tool.error_rate` | Counter | Tool failures by type |
| `ucil.plugin.state` | Gauge | Plugin lifecycle state (HOT/COLD/ERROR) |
| `ucil.bonus.usage_rate` | Gauge | Fraction of bonus context used by agents |
| `ucil.lsp.diagnostic_count` | Gauge | Active diagnostics per language |
| `ucil.kg.entity_count` | Gauge | Knowledge graph entity count |
| `ucil.session.active` | Gauge | Active agent sessions |

### 15.4 Export targets

- **Development**: stdout (human-readable spans)
- **Local**: Jaeger (via OTLP gRPC to localhost:4317)
- **Production**: Any OTLP-compatible backend (Grafana Tempo, Datadog, Honeycomb)
- **MCPcat**: One-line integration for MCP-specific analytics

Configured via `ucil.toml`:
```toml
[telemetry]
enabled = true
exporter = "stdout"              # stdout | otlp | jaeger | none
otlp_endpoint = "http://localhost:4317"
sample_rate = 1.0                # 1.0 = trace everything (dev), 0.1 = 10% (prod)
```

---

## 16. CLI specification

```
ucil — Unified Code Intelligence Layer

COMMANDS:
  init [--path <dir>] [--detect-languages] [--install-plugins]
      Create .ucil/, detect languages, generate ucil.toml, configure LLM provider.
      Interactive: choose LLM provider, set up host adapter, install Claude Code plugin.

  daemon start [--foreground] [--log-level <level>]
  daemon stop
  daemon restart
  daemon logs [--follow] [--lines <n>]

  plugin list | install <n> | uninstall <n> | enable <n> | disable <n> | reload

  status
      Daemon status, index health, cache stats, plugin health (incl. Serena), session count,
      diagnostics bridge status, quality issue summary, OTel export status.

  compact [--aggressive] [--dry-run]
      Run knowledge graph compaction manually.

  query <tool-name> <query-string> [--token-budget <n>] [--format json|markdown]
      Direct query for testing. Bypasses MCP server.

  config show | set <key> <value>

  export [--format dot|json|csv] [--output <file>]
      Export knowledge graph for debugging/visualization.

  extract-conventions
      Manually trigger convention extraction.

  lsp status
      Show Serena + diagnostics bridge status: which servers running, diagnostic counts.

  quality [--file <path>] [--severity <level>]
      Show quality issues from G7. Filter by file and severity.

  group create <n> | join <n> | leave | list | sync
      Manage cross-project knowledge groups.

  convention promote <id> [--to-group]
      Promote a project convention to cross-project group scope.

  export-brain [--output <path>] [--include-branch-indexes]
      Export shared knowledge for team sharing.

  import-brain <path> [--merge | --replace]
      Import a previously exported brain.

  backup [--output <path>]
  restore <backup-path>
```

---

## 17. Directory structure

```
ucil/
├── Cargo.toml                    # Rust workspace root
├── README.md, LICENSE, ARCHITECTURE.md, CLAUDE.md
│
├── crates/
│   ├── ucil-core/                # Core: incremental engine, KG, cache, fusion, CEQP
│   │   └── src/ (lib.rs, incremental.rs, knowledge_graph.rs, vector_store.rs,
│   │            cache.rs, fusion.rs, context_compiler.rs, compaction.rs,
│   │            ceqp.rs, reason_parser.rs, bonus_selector.rs, types.rs,
│   │            hot_staging.rs, warm_processors.rs, tier_merger.rs,
│   │            feedback.rs, cross_project.rs, resilience.rs,
│   │            schema_migration.rs, language_detect.rs, quality_tracker.rs,
│   │            otel.rs)
│   ├── ucil-daemon/              # Daemon binary
│   │   └── src/ (main.rs, server.rs, watcher.rs, plugin_manager.rs,
│   │            query_router.rs, executor.rs, session_manager.rs,
│   │            branch_manager.rs, health.rs, agent_scheduler.rs)
│   ├── ucil-cli/                 # CLI binary
│   │   └── src/ (main.rs, commands/{init,daemon,plugin,status,compact,
│   │                               query,config,lsp,quality,group}.rs)
│   ├── ucil-treesitter/          # tree-sitter integration
│   │   └── src/ (lib.rs, parser.rs, symbols.rs, chunker.rs)
│   ├── ucil-lsp-diagnostics/      # LSP diagnostics bridge (complements Serena)
│   │   └── src/ (lib.rs, bridge.rs, server_sharing.rs, diagnostics.rs,
│   │            call_hierarchy.rs, type_hierarchy.rs)
│   ├── ucil-agents/              # Internal agent implementations
│   │   └── src/ (lib.rs, provider.rs, interpreter.rs, synthesis.rs,
│   │            conflict.rs, clarification.rs, convention.rs,
│   │            memory_curator.rs, architecture.rs)
│   └── ucil-embeddings/          # Embedding inference (NEW)
│       └── src/ (lib.rs, onnx_inference.rs, chunker.rs, models.rs)
│
├── plugin/                       # Claude Code plugin (NEW, distribution package)
│   ├── .claude-plugin/plugin.json
│   ├── agents/ (ucil-lint.md, ucil-security.md, ucil-test.md, ucil-review.md)
│   ├── skills/ (analyze/, lint/, security-scan/, test/, review/, usage-guide/)
│   ├── hooks/ (post-write-analyze.py, pre-tool-route.py, stop-quality-check.py)
│   ├── .mcp.json
│   └── .claude/rules/ucil-conventions.md
│
├── adapters/                     # Host adapters (TypeScript)
│   └── src/ (base.ts, claude-code.ts, codex.ts, aider.ts,
│             cline.ts, cursor.ts, ollama.ts, auto-detect.ts)
│   └── templates/ (claude-mcp.json.hbs, claude-md.hbs, skill.md.hbs,
│                    agents-md.hbs, codex-config.toml.hbs)
│
├── plugins/                      # Built-in plugin manifests
│   ├── structural/ (serena/, ast-grep/, scip/, joern/)
│   ├── search/ (probe/, ripgrep/, zoekt/, codedb/)
│   ├── knowledge/ (codebase-memory/, mem0/, graphiti/, arc-memory/, cognee/, conport/)
│   ├── architecture/ (codegraphcontext/, gitnexus/, dep-cruiser/, axon/, deptry/, nx/, bazel/)
│   ├── context/ (repomix/, context7/, open-context/, code2prompt/, outline/)
│   ├── platform/ (github-mcp/, git-mcp/, filesystem-mcp/, playwright-mcp/,
│   │              docker-mcp/, terraform-mcp/, kubectl-mcp/)
│   ├── quality/ (semgrep/, sonarqube/, snyk/, eslint/, ruff/, rubocop/,
│   │             osv/, trivy/, trufflehog/, gitleaks/, biome/)
│   └── testing/ (test-runner/, pytest-runner/)
│   └── (each contains plugin.toml)
│
├── ml/                           # ML pipelines (Python)
│   ├── requirements.txt, embed.py, chunker.py
│   └── models/                   # Local model weights cache
│
├── scripts/ (install.sh, install-plugins.sh, benchmark.sh, install-claude-plugin.sh)
├── tests/
│   ├── fixtures/ (rust-project/, python-project/, typescript-project/)
│   ├── integration/ (test_fusion.rs, test_incremental.rs, test_query_pipeline.rs,
│   │                 test_host_adapters.rs, test_multi_agent.rs, test_ceqp.rs,
│   │                 test_knowledge_tiering.rs, test_feedback_loop.rs,
│   │                 test_cross_project.rs, test_resilience.rs,
│   │                 test_progressive_startup.rs, test_review_changes.rs,
│   │                 test_lsp_bridge.rs, test_quality_pipeline.rs, test_otel.rs)
│   └── benchmarks/ (bench_indexing.rs, bench_query.rs, bench_token_efficiency.rs,
│                    bench_embedding.rs)
└── docs/ (architecture.md, plugin-development.md, host-adapter-guide.md,
           configuration.md, benchmarks.md, claude-code-integration.md,
           serena-diagnostics-guide.md, observability.md)
```

---

## 18. Phase-wise implementation plan

### Phase 0 — Project bootstrap (Week 1)

**Goal**: Repository skeleton, build system, core types, `ucil init`.

**Tasks**:
1. Create repository with full directory structure
2. Set up Cargo workspace, TypeScript project, Python environment
3. Write CLAUDE.md for Claude Code to work on UCIL itself
4. Define core type system in `ucil-core/src/types.rs`
5. Write `ucil init` command: create `.ucil/`, language/framework detection, generate `ucil.toml`, LLM provider selection, dependency check, **plugin health verification** (verify every P0 plugin starts and responds)
6. Write init verification report: `.ucil/init_report.json` with full health status
6. Implement schema version tracking
7. Set up CI: Rust build + test, TypeScript lint, Python lint
8. Initialize OpenTelemetry crate with stdout exporter

**Deliverable**: Compilable skeleton where `ucil init` works.

---

### Phase 1 — Daemon core + tree-sitter + Serena + diagnostics bridge (Weeks 2–5)

**Goal**: Running daemon with tree-sitter indexing, Serena integration, LSP diagnostics bridge, symbol cache, session management, branch detection, and first working MCP tools.

**Week 2 — tree-sitter integration**:
1. Implement `ucil-treesitter`: multi-language parser, symbol extraction, AST-aware chunking
2. Implement tag cache in LMDB: `file_path + mtime → [Symbol]`
3. Implement session manager: session creation, branch detection, worktree discovery
4. Implement two-tier storage layout (shared/ + branches/)

**Week 3 — Daemon core**:
1. Process lifecycle (daemonize, PID file, signal handling, crash recovery)
2. File watcher: `notify` crate with `notify-debouncer-full`. Two detection paths: PostToolUse hook (bypass debounce, 0ms) and notify events (100ms debounce). Auto-detect and use Watchman if present for repos >50K files.
3. Salsa incremental engine skeleton: dependency DAG, invalidation, lazy recompute, early cutoff. Position-independent intermediate representations.
4. Plugin manager skeleton: manifest scanning, process spawning, health checks, HOT/COLD lifecycle
5. Basic MCP server (stdio): register all 22 tools (stubs for unimplemented ones return `_meta.not_yet_implemented: true`)
6. Progressive startup: MCP server available immediately, priority indexing queue

**Week 4 — SQLite knowledge graph + basic queries**:
1. Create SQLite schema (shared + branch + hot staging + warm + quality_issues + feedback)
2. Implement knowledge_graph.rs: CRUD, bi-temporal queries, symbol resolution
3. Wire tree-sitter extraction → knowledge graph population
4. Implement first working tool: `find_definition` (tree-sitter lookup → Serena enrichment → KG population)
5. Implement CEQP universal parameters on all tool schemas
6. Implement session state tracking
7. Implement hot staging writes

**Week 5 — Serena integration + LSP diagnostics bridge**:
1. Write plugin manifest for Serena. Test installation and MCP client communication.
2. Wire Serena → G1 structural fusion (find_symbol, find_references, go_to_definition)
3. Implement `ucil-lsp-diagnostics` crate: lightweight bridge that connects to Serena's LSP servers
4. LSP diagnostics client (JSON-RPC): textDocument/diagnostic, callHierarchy/incomingCalls, typeHierarchy/supertypes
5. Wire diagnostics bridge → G7 quality (type errors, lint warnings as quality_issues)
6. Wire diagnostics bridge → G4 architecture (call/type hierarchy enrichment)
7. When Serena unavailable: bridge spawns own LSP servers (pyright, rust-analyzer, typescript-ls)

**Deliverable**: `ucild` runs, watches files, maintains symbol index, Serena provides LSP navigation for 40+ languages, diagnostics bridge pulls type errors into quality pipeline, serves `find_definition` and `search_code` via MCP with all 22 tools registered.

---

### Phase 2 — Plugins + G1/G2 + embeddings (Weeks 6–8)

**Goal**: Plugin system operational. G1 (Structural) and G2 (Search) fully working. Embedding pipeline operational.

**Week 6 — Plugin system**:
1. Plugin manifest parser, lifecycle manager, hot-reload
2. HOT/COLD lifecycle: idle timeout, on-demand restart, circuit breakers
3. Write manifests for ast-grep and Probe. Test installation.
4. `ucil plugin` CLI commands

**Week 7 — G1 (Structural) + G2 (Search) fusion**:
1. G1 parallel-all: tree-sitter + Serena + ast-grep + diagnostics bridge all run, fuse results
2. G2 intra-group RRF: Probe 2.0, ripgrep 1.5, LanceDB 1.5
3. Session deduplication tracking
4. Wire into `find_definition`, `find_references`, `search_code`

**Week 8 — Embedding pipeline (NEW)**:
1. `ucil-embeddings` crate: ONNX Runtime (`ort` crate) inference
2. CodeRankEmbed (137M, CPU) as default, Qwen3-Embedding (8B, GPU optional) as upgrade
3. LanceDB integration: background indexing, incremental updates, per-branch stores
4. Benchmark: embedding throughput, query latency, recall@10

**Deliverable**: Plugin system works. `find_definition`, `find_references`, `search_code` return fused results from Serena + tree-sitter + ast-grep. Vector search works.

---

### Phase 3 — Orchestration + all groups + warm processors (Weeks 9–11)

**Goal**: Full query pipeline. All 8 tool groups operational. RRF fusion, conflict resolution, context compiler, warm processors.

**Week 9 — Query router + parallel executor + G3/G4**:
1. Deterministic query classifier
2. CEQP reason parser
3. Parallel executor with per-group timeouts
4. RRF fusion engine with query-type weight matrix
5. G3 (Knowledge): Install Codebase-Memory MCP + Mem0
6. G4 (Architecture): Install CodeGraphContext + LSP call hierarchy

**Week 10 — G5/G6 + response assembly + bonus context**:
1. G5 (Context): Aider repo-map reimplementation in Rust + Context7 + Repomix
2. G6 (Platform): GitHub MCP, Git MCP, Filesystem MCP
3. Quality-maximalist response assembly (no budget fitting — include everything)
4. Conflict resolution: agent-based with source authority as soft guidance
5. Bonus context selector — include ALL relevant conventions, pitfalls, quality, tests
6. Multi-tier query merging (hot + warm + cold)
7. Warm processors (4 rule-based, 60s/120s intervals)

**Week 11 — G7/G8 (Quality + Testing) (NEW)**:
1. G7: Wire LSP diagnostics bridge + install ESLint MCP + Ruff MCP + Semgrep MCP
2. G7: Severity-weighted merge fusion
3. G7: quality_issues table tracking (first_seen, last_seen, resolved)
4. G8: Install test-runner-mcp + mcp-pytest-runner
5. G8: Test discovery (convention-based + import-based + KG tested_by relations)
6. Wire G7/G8 into `check_quality` and `review_changes`
7. Feedback loop: post-hoc analyzer tracking bonus context usage

**Deliverable**: All 8 groups operational. Full pipeline from query → fusion → response with quality and testing.

---

### Phase 3.5 — Agent layer (Weeks 12–13)

**Goal**: Internal agents operational. LLM providers configured. Agents run on every query by default. Deterministic fallback for provider=none.

**Week 12 — Agent infrastructure + query-path agents**:
1. LlmProvider trait + implementations (Ollama, Claude API, OpenAI, host passthrough, none)
2. Query Interpreter agent
3. Synthesis Agent (LLM + host-passthrough modes)

**Week 13 — Remaining agents + background enrichment**:
1. Conflict Mediator agent
2. Clarification Agent with MCP Elicitation
3. Background agent scheduler: Convention Extractor, Memory Curator, Architecture Narrator
4. Integration tests for all agents

**Deliverable**: 7 agents operational. All queries get Interpreter + Synthesis agent enrichment by default (~200ms–3s). Deterministic path as fallback when provider=none. Background agents enrich KG continuously.

---

### Phase 4 — Host adapters + Claude Code plugin (Weeks 14–15)

**Goal**: UCIL works seamlessly with Claude Code (via plugin), Codex CLI, Cursor, and 3+ other hosts.

**Week 14 — Claude Code plugin + auto-detect**:
1. Build Claude Code plugin package (skills, hooks, subagents, rules, .mcp.json)
2. PostToolUse hook: auto-notify daemon on file changes
3. Skills: /ucil-lint, /ucil-security, /ucil-test, /ucil-review, usage-guide
4. Subagents: ucil-lint.md (haiku), ucil-security.md (sonnet), ucil-test.md, ucil-review.md
5. End-to-end test: install plugin, use Claude Code, verify quality
6. `ucil init --install-claude-plugin`

**Week 15 — Additional adapters**:
1. Codex CLI adapter: AGENTS.md, aggressive compression for 10KB limit, pagination
2. Cursor adapter: core 5 tools only, .cursor/mcp.json
3. Cline/Roo Code adapter: mode-specific tool filtering, alwaysAllow
4. Aider adapter: HTTP bridge, enhanced repo-map
5. Ollama adapter: aggressive compression, signature-only mode
6. Adapter installation guides

**Deliverable**: Claude Code plugin installs in one command. All supported hosts can use UCIL.

---

### Phase 5 — Knowledge evolution + compaction + security (Weeks 16–18)

**Goal**: Knowledge graph evolves with commits. Security scanning integrated. Compaction prevents unbounded growth.

**Week 16 — Knowledge evolution + security**:
1. Git hook integration: post-commit re-index, PR context extraction
2. `remember` tool full implementation
3. Install Snyk MCP (P1), OSV MCP, TruffleHog/Gitleaks
4. Wire security tools into G7 fusion
5. `security_scan` tool full implementation

**Week 17 — Compaction + temporal queries + tiered GC**:
1. Importance decay rules for cold tier
2. Periodic compaction: staleness, relation validation, quality issue cleanup
3. Tiered GC: hot >1h → delete; warm >7d unpromoted → delete
4. Background agents consume warm data (faster, cheaper)
5. Arc Memory integration for `explain_history`

**Week 18 — Cross-project + review + docs**:
1. Cross-project knowledge groups: `~/.ucil/groups/` storage, CLI commands
2. Convention promotion pipeline (project → group when seen in ≥3 projects)
3. `review_changes` tool: diff analysis against conventions, quality, tests, blast radius, security
4. `generate_docs` tool: architecture/module/API doc generation
5. Tests for knowledge graph evolution

**Deliverable**: Knowledge evolves organically. Security scanning works. Compaction keeps growth bounded.

---

### Phase 6 — Performance + observability (Weeks 19–20)

**Goal**: P95 query latency < 500ms. Full observability. Cache hit rates documented.

**Week 19 — Tiered cache + optimization + OTel**:
1. Full tiered cache: L0 (in-memory LRU), L1 (LMDB), L2 (full invocation)
2. Cache invalidation tied to Salsa incremental engine
3. OpenTelemetry: full span hierarchy, metrics, Jaeger export
4. MCPcat integration option
5. Resource limits: RSS monitoring, disk usage tracking, LRU eviction

**Week 20 — Benchmarking**:
1. Full benchmark suite: indexing, query latency, token efficiency, memory, cache hit rates, multi-agent contention, embedding throughput, Serena + diagnostics bridge latency
2. `docs/benchmarks.md`
3. Regression benchmark in CI

**Deliverable**: Published benchmarks. P95 < 500ms. Full observability.

---

### Phase 7 — Database + infrastructure integration (Week 21) (NEW)

**Goal**: Database intelligence and infrastructure context available.

**Tasks**:
1. Install DBHub MCP (universal database schema access)
2. Install Prisma MCP (migration management for Prisma projects)
3. Wire database tools into G5 (Context) and G4 (Architecture)
4. `query_database` tool full implementation
5. Install Docker MCP, Terraform MCP for infrastructure context
6. Install Sentry MCP for runtime error intelligence
7. `check_runtime` tool full implementation

**Deliverable**: Agents can query database schemas, check migrations, inspect infrastructure, and access runtime error data.

---

### Phase 8 — Documentation + release (Weeks 22–24)

**Goal**: Public v0.1.0 release.

**Tasks**:
1. Full documentation suite: architecture, plugin-dev, host-adapter, config, observability, Serena + diagnostics bridge guide, Claude Code integration
2. Schema migration system: versioned runner, auto-backup
3. Import/export: `ucil export-brain`, `ucil import-brain`, `ucil backup`, `ucil restore`
4. Install script (`scripts/install.sh`) — one-line install
5. Claude Code plugin publish to GitHub
6. Demo video/GIF: UCIL in action with Claude Code + multi-agent
7. Publish to GitHub with badges, screenshots, benchmarks
8. Submit to awesome-mcp-servers lists

**Deliverable**: Public v0.1.0 release on GitHub.

---

## 19. Testing and validation strategy

### 19.1 Test categories

| Category | What | Approach |
|----------|------|----------|
| Unit | Individual functions in core/treesitter/agents/lsp-bridge/embeddings | Rust `#[test]` |
| Integration | Full pipeline: query → route → fuse → respond | Against fixture projects |
| Plugin | Lifecycle, HOT/COLD, communication, crash recovery | Real plugins vs fixtures |
| LSP / Serena | Serena plugin lifecycle, diagnostics bridge, server sharing, cross-language coverage | Real LSP servers vs fixtures |
| Multi-agent | Concurrent sessions, branch isolation, shared knowledge | Simulated 5-agent workload |
| CEQP | Reason parsing, bonus context selection, quality scoring | Golden queries |
| Knowledge tiering | Hot→warm→cold promotion, multi-tier merging, GC | Timed assertions |
| Quality pipeline | G7 fusion, severity merge, quality_issues tracking | Fixture projects with known issues |
| Testing pipeline | G8 test discovery, execution, coverage tracking | Fixture projects with tests |
| Feedback loop | Bonus usage tracking, importance boost/decay | Simulated agent sequences |
| Cross-project | Group sync, convention promotion | Multi-project fixture |
| Resilience | Plugin crash, DB corruption, missing deps, progressive startup | Fault injection |
| Observability | OTel spans, metrics, export | Span assertion library |
| Adapter | Host-specific transformation correctness | Mock host environments |
| Benchmark | Performance regression | Criterion, tracked in CI |
| Quality | Context quality vs baselines | Compare UCIL vs Aider/Cursor |

### 19.2 Fixture projects

Four in `tests/fixtures/`: rust-project (~5K LOC), python-project (~5K LOC), typescript-project (~5K LOC), mixed-project (~3K LOC with intentional lint errors, type errors, security issues, and test failures).

---

## 20. Configuration reference

```toml
[project]
name = "my-project"
languages = ["rust", "typescript"]
root = "."
exclude = ["node_modules", "target", ".git", "dist", "build"]

[daemon]
log_level = "info"
pid_file = ".ucil/daemon.pid"
socket = ".ucil/daemon.sock"
mcp_transport = "stdio"              # stdio | http
http_port = 9742

[indexing]
max_file_size_kb = 1024
watch_debounce_ms = 100
post_tool_use_bypass_debounce = true
background_threads = 2
eager_reparse = true

[knowledge_graph]
compaction_interval_hours = 6
entity_decay_rate = 0.99
observation_decay_rate = 0.95
decision_decay_rate = 0.999
min_importance_threshold = 0.05

[knowledge_tiering]
observation_processor_interval_sec = 60
convention_signal_processor_interval_sec = 60
architecture_delta_processor_interval_sec = 120
decision_linker_interval_sec = 60
convention_min_evidence = 3
hot_max_age_minutes = 60
warm_max_age_days = 7
warm_promoted_keep_hours = 24
observation_dedup_threshold = 0.9

[vector_store]
backend = "lancedb"                  # "lancedb" or "sqlite-vec"
embedding_model = "coderankembed"     # "coderankembed" (137M, CPU default) or "qwen3-embedding" (8B, GPU upgrade)
embedding_dimensions = 768           # 768 for CodeRankEmbed (default), 1024 for Qwen3
chunk_max_tokens = 512
reindex_on_startup = false

[cache]
l0_max_entries = 1000
l1_max_size_mb = 100
query_ttl_seconds = 300

[context]
soft_token_hint = 25000              # Advisory only, not enforced by UCIL — host adapter decides
max_token_budget = 25000
# No max_budget_expansion — quality maximalist, host adapter trims if needed
format = "markdown"
include_confidence = true
include_timing = false

[plugins]
auto_install = true
plugin_timeout_ms = 5000
max_concurrent_plugins = 10
hot_cold_lifecycle = true            # Enable HOT/COLD for idle plugins
idle_timeout_minutes = 10

[host]
auto_detect = true
preferred = "claude-code"

[tools]
all_tools_always_loaded = true       # All 22 tools registered with every host
run_all_tools_in_group = true        # All tools in a group run on every query (no cascade)

[agents]
enabled = true                       # Agents run on every query (default). Set false for deterministic-only.
query_interpreter = true             # Interpreter agent on every query
synthesis = true                     # Synthesis agent on every query
conflict_mediator = true             # Conflict agent when disagreements detected
clarification = true                 # Clarification via MCP Elicitation when ambiguous

[response]
full_detail_for_relevant = true      # Relevant content at full detail, irrelevant excluded
soft_token_hint = 25000              # Advisory — included in _meta.token_count for host adapters
complex_task_extra_context = true    # Rich reasons get more context: modules, examples, tests
session_dedup = true                 # Don't repeat content agent already has

[lsp_diagnostics]
enabled = true
share_serena_servers = true          # Connect to Serena's LSP servers when available
fallback_spawn_own = true            # Spawn own LSP servers when Serena unavailable
grace_period_minutes = 5             # Keep standalone LSP server alive after last session
auto_detect_servers = true
max_concurrent_servers = 5

[llm]
primary_provider = "host_passthrough"
fallback_provider = "ollama"
monthly_budget_usd = 10.0

[llm.ollama]
endpoint = "http://localhost:11434"
model = "qwen2.5-coder:7b"
timeout_ms = 10000
max_concurrent = 2

[llm.claude]
model = "claude-sonnet-4-20250514"
timeout_ms = 15000

[llm.routing]
query_interpretation = "primary"
synthesis = "primary"
conflict_resolution = "primary"
convention_extraction = "fallback"
memory_curation = "fallback"
architecture_narration = "fallback"

[agents]
enabled = true
fast_path_confidence = 0.7
query_agent_timeout_ms = 5000
elicitation_enabled = true
max_clarifications_per_query = 2

[agents.background]
convention_extract_every_n_commits = 10
convention_sample_files = 100
memory_curate_interval_hours = 24
memory_curate_after_session = true
architecture_narrate_every_n_commits = 50
architecture_max_tokens = 2000

[multi_agent]
max_sessions = 10
session_timeout_minutes = 30
prune_inactive_branch_days = 7
archive_inactive_branch_days = 30

[worktrees]
auto_discover = true
extra_paths = []

[ceqp]
enable_bonus_context = true
enable_adaptive_guidance = true
context_quality_hint_threshold = 0.7
enable_feedback_loop = true

[quality]                            # NEW
enable_quality_tracking = true
auto_lint_on_change = true           # Run LSP diagnostics on every file change
security_scan_on_pr = true           # Auto-scan security on review_changes
severity_threshold = "medium"        # Minimum severity to include in bonus context

[testing]                            # NEW
enable_test_discovery = true
auto_run_on_change = false           # Don't auto-run tests (expensive); agent calls explicitly
coverage_format = "lcov"             # lcov | cobertura

[cross_project]
group = ""
sync_interval_minutes = 30
auto_promote_threshold = 3
allow_group_override_local = false

[telemetry]
enabled = true
exporter = "stdout"                  # stdout | otlp | jaeger | none
otlp_endpoint = "http://localhost:4317"
sample_rate = 1.0

[resilience]
auto_backup_before_compaction = true
max_backups = 3
db_integrity_check_on_startup = true
plugin_max_restarts = 3
plugin_restart_backoff_base_sec = 1
wal_max_size_mb = 100

[resource_limits]
max_daemon_rss_mb = 512
max_ucil_dir_size_mb = 2048
disk_warning_threshold_percent = 80
disk_critical_threshold_percent = 95
```

---

## 21. Production readiness

### 21.1 Error handling and resilience

(All error handling from v1 plan applies, plus:)

**Serena failures**:
- Serena crash: Plugin supervisor restarts with exponential backoff. During restart, G1 operates in degraded mode — tree-sitter + ast-grep + SCIP still run and fuse. Quality reduced but functional. Diagnostics bridge switches to spawning its own LSP servers.
- Serena LSP server crash (e.g., pyright OOMs): Serena handles internal restart. If persistent, diagnostics bridge detects stale diagnostics and logs warning.
- Serena not installed: G1 operates via tree-sitter + ast-grep + SCIP (all still run in parallel). Diagnostics bridge spawns own LSP servers. `_meta.degraded_plugins: ["serena"]` in responses. Navigation quality reduced but all other tools still contribute.

**LSP diagnostics bridge failures**:
- Bridge can't connect to Serena's servers: Falls back to spawning own LSP servers.
- LSP server unresponsive: 5s timeout per operation. On timeout, mark language as degraded, include `_meta.degraded_languages: ["python"]`.
- LSP server not available for a language: Skip diagnostics for that language. G7 still operates via linters (ESLint, Ruff, etc.).

**Quality tool failures**:
- Linter/scanner not installed: Skip that tool. G7 operates with available tools.
- Security scanner timeout: Return partial results with `_meta.partial_security: true`.

### 21.2 Progressive startup

```
Phase 1 (0-2 seconds): MCP server is LIVE, 5 core tools accept queries
Phase 2 (2-30 seconds): Priority indexing of queried files
Phase 3 (30s-5 minutes): Background full index, LSP servers warm up
Phase 4 (5-30 minutes): Vector embeddings, deep indexing, first convention extraction
Phase 5 (ongoing): Fully operational, incremental updates
```

### 21.3 Import/export for team onboarding

```
ucil export-brain --output ~/ucil-brain.tar.gz
    Export: knowledge.db, memory.db, history.db, ucil.toml, plugin manifests
    Exclude: branch indexes (rebuilt), sessions (per-agent), backups

ucil import-brain ~/ucil-brain.tar.gz --merge
    Import: all shared knowledge merged by entity/observation dedup
    Result: immediate access to colleague's 3 months of accumulated knowledge
```

### 21.4 Language and framework detection

`ucil init` auto-detects languages, frameworks, build systems, and test frameworks. Maps detections to recommended plugins. Interactive install with P0/P1/P2 priority tiers.

---

## Summary

| Phase | Duration | What ships |
|-------|----------|-----------|
| 0: Bootstrap | 1 week | Repo skeleton, types, `ucil init` |
| 1: Daemon core + Serena | 4 weeks | Daemon, tree-sitter, Serena, diagnostics bridge, sessions, branches, all 22 tools registered |
| 2: Plugins + G1/G2 + embeddings | 3 weeks | Plugin system, ast-grep + Probe, vector search, embedding pipeline |
| 3: Orchestration + all groups | 3 weeks | Full pipeline, 8 groups, RRF fusion, CEQP, G7 quality, G8 testing, warm processors |
| 3.5: Agent layer | 2 weeks | 7 agents, LLM providers, elicitation, background enrichment |
| 4: Host adapters + Claude Code plugin | 2 weeks | Plugin distribution, Claude Code + Codex + Cursor + Cline + Aider adapters |
| 5: Knowledge evolution + security | 3 weeks | Git hooks, security scanning, compaction, cross-project, review_changes |
| 6: Performance + observability | 2 weeks | Tiered cache, OpenTelemetry, benchmarks, resource limits |
| 7: Database + infrastructure | 1 week | DBHub, Prisma, Docker, Terraform, Sentry integration |
| 8: Release | 3 weeks | Docs, migration, import/export, public v0.1.0 |
| **Total** | **24 weeks** | |

The system orchestrates 60+ tools across 8 groups — running ALL tools in each group on every query and fusing their outputs via weighted RRF. LLM agents (Interpreter, Synthesis, Conflict Mediator) run on every query by default, producing rich semantic understanding rather than mechanical data retrieval. Serena (40+ languages) provides LSP navigation complemented by an LSP diagnostics bridge (type errors, call hierarchies). A bi-temporal knowledge graph with hot/warm/cold tiering evolves continuously. Token usage is smart — full detail for relevant content, nothing for irrelevant content, no artificial caps forcing lossy compression. All 22 tools are always available (~22K tokens). Init and session start verify every tool is operational. 3–5 concurrent agents share a brain across branches and worktrees. Distribution via Claude Code plugin with skills/hooks/subagents. OpenTelemetry instrumented from day one. 7 internal agents enrich every query and continuously grow the knowledge graph in the background. No cloud dependencies. Fully extensible via plugins. Every token deserved — nothing less, nothing more.
