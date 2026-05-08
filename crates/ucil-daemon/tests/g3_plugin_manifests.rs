//! End-to-end integration tests for the on-disk codebase-memory + mem0
//! G3 (Knowledge) plugin manifests (P3-W9-F05 / P3-W9-F06).
//!
//! Each test loads the on-disk manifest at
//! `plugins/knowledge/<name>/plugin.toml`, drives the manifest's
//! `transport.command` as a real subprocess via
//! [`ucil_daemon::PluginManager::health_check_with_timeout`], and
//! asserts the live `tools/list` reply contains an expected tool name.
//!
//! Mocking `tokio::process::Command`, the spawned MCP server, or the
//! JSON-RPC dialogue is forbidden — the WO-0069 contract is precisely
//! that real MCP-server subprocesses speak real JSON-RPC over stdio
//! exactly the same way a Claude Code / Cursor / Cline client would
//! consume them at runtime. The test exercises the full handshake
//! [`ucil_daemon::PluginManager::health_check`] performs (`initialize`
//! → `notifications/initialized` → `tools/list`) end-to-end against the
//! real `npx -y codebase-memory-mcp@0.6.1` and
//! `uvx mem0-mcp-server@0.2.1` invocations.
//!
//! Workspace fixtures exercised by the partner verify scripts
//! (`scripts/verify/P3-W9-F05.sh`, `scripts/verify/P3-W9-F06.sh`):
//! `tests/fixtures/rust-project` for codebase-memory symbol-lookup
//! smoke; an ephemeral `mktemp -d`-rooted store for mem0's CRUD
//! round-trip smoke. The integration tests themselves do not touch
//! those fixtures — they exercise only the manifest spawn + MCP
//! handshake, which is the load-bearing daemon-side surface.
//!
//! Set `UCIL_SKIP_EXTERNAL_PLUGIN_TESTS=1` only on truly offline CI
//! builds (skips both WO-0044 and WO-0069 plugin-manifest suites);
//! set the G3-specific `UCIL_SKIP_KNOWLEDGE_PLUGIN_E2E=1` to skip
//! ONLY these two tests (so an operator can keep the WO-0044
//! ast-grep + probe regression coverage without paying the additional
//! 90-second-cold-cache npx/uvx fetches for the G3 plugins). The
//! verifier MUST NOT set EITHER opt-out, per WO-0069 `scope_in` #5.
//!
//! Tests are wrapped in `mod g3_plugin_manifests` so nextest reports
//! them as
//! `g3_plugin_manifests::codebase_memory_manifest_health_check` and
//! `g3_plugin_manifests::mem0_manifest_health_check`, matching the
//! WO-0069 acceptance selectors. Same wrapper pattern as the existing
//! `mod plugin_manifests` block in `tests/plugin_manifests.rs:36`
//! (DEC-0007 frozen-selector module-root placement; carried per
//! WO-0068 lessons §"For planner" frozen-test selector substring-
//! match REQUIRES module-root placement).
//!
//! This file is ADDITIVE — `tests/plugin_manifests.rs` (WO-0044
//! P2-W6-F05/F06 regression guard) is intentionally NOT modified to
//! keep the two regression guards isolated and to scope the
//! G3-specific `UCIL_SKIP_KNOWLEDGE_PLUGIN_E2E` opt-out distinctly.

mod g3_plugin_manifests {
    use std::path::PathBuf;

    use ucil_daemon::{HealthStatus, PluginManager, PluginManifest};

    /// Generous first-run npx/uvx download budget — `npx -y <pkg>` may
    /// fetch a tarball on a cold cache and `uvx <pkg>` may resolve
    /// dozens of transitive Python deps (mem0ai pulls openai, mcp,
    /// pydantic, sqlalchemy, etc. — typically ~70 packages on a fresh
    /// uv cache). Subsequent runs hit the cache and complete in well
    /// under a second; the production-default `HEALTH_CHECK_TIMEOUT_MS`
    /// (5 s) is therefore fine for steady-state daemon ticks but
    /// inadequate for the very first post-install integration-test
    /// run on a fresh workstation. Mirror the WO-0044 budget exactly.
    const FIRST_RUN_TIMEOUT_MS: u64 = 90_000;

    /// Walks up from this crate's manifest dir (`crates/ucil-daemon`) to
    /// the workspace root so the on-disk plugin manifests can be loaded
    /// regardless of the directory `cargo test` is invoked from.
    fn repo_root() -> PathBuf {
        // `CARGO_MANIFEST_DIR` for ucil-daemon is `<repo>/crates/ucil-daemon`;
        // two parents up is the workspace root.
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("workspace root above crates/ucil-daemon")
            .to_path_buf()
    }

    /// Honours the `UCIL_SKIP_EXTERNAL_PLUGIN_TESTS` opt-out for
    /// air-gapped CI runners that cannot reach npm/pypi at all (this
    /// is the same global opt-out honoured by the WO-0044
    /// `tests/plugin_manifests.rs` suite) AND the G3-specific
    /// `UCIL_SKIP_KNOWLEDGE_PLUGIN_E2E` opt-out for operators that
    /// want to keep the WO-0044 ast-grep + probe regression coverage
    /// but skip the additional codebase-memory + mem0 cold-cache
    /// budget. Either env set means "skip these two tests"; the
    /// verifier MUST NOT set either, per WO-0069 `scope_in` #5.
    fn skip_via_env() -> bool {
        std::env::var("UCIL_SKIP_EXTERNAL_PLUGIN_TESTS").is_ok()
            || std::env::var("UCIL_SKIP_KNOWLEDGE_PLUGIN_E2E").is_ok()
    }

    #[tokio::test]
    async fn codebase_memory_manifest_health_check() {
        if skip_via_env() {
            return;
        }
        let manifest_path = repo_root().join("plugins/knowledge/codebase-memory/plugin.toml");
        let manifest =
            PluginManifest::from_path(&manifest_path).expect("parse codebase-memory plugin.toml");

        // Manifest sanity (cheap pre-flight before paying the npx cost).
        assert_eq!(manifest.plugin.name, "codebase-memory");
        assert_eq!(manifest.transport.kind, "stdio");
        assert!(
            !manifest.capabilities.provides.is_empty(),
            "codebase-memory manifest must declare at least one provided capability",
        );

        let health = PluginManager::health_check_with_timeout(&manifest, FIRST_RUN_TIMEOUT_MS)
            .await
            .expect("health-check codebase-memory MCP server");

        assert_eq!(health.name, "codebase-memory");
        assert_eq!(
            health.status,
            HealthStatus::Ok,
            "codebase-memory health-check returned non-Ok status: {:?}",
            health.status,
        );
        assert!(
            !health.tools.is_empty(),
            "codebase-memory advertised zero tools",
        );
        // `search_graph` is the canonical symbol-lookup tool advertised
        // by `codebase-memory-mcp@0.6.1` (alongside `index_repository`,
        // `query_graph`, `trace_path`, `get_code_snippet`,
        // `get_graph_schema`, `get_architecture`, `search_code`,
        // `list_projects`, `delete_project`, `index_status`,
        // `detect_changes`, `manage_adr`, `ingest_traces` — 14 total
        // matches master-plan §3.1 line 311 claim). Pinning on the
        // existence of this exact name surfaces upstream tool-surface
        // drift loudly; pinning on the count would force an update on
        // every benign upstream tool addition.
        assert!(
            health.tools.iter().any(|t| t == "search_graph"),
            "expected `search_graph` tool in advertised set, got: {:?}",
            health.tools,
        );
    }

    #[tokio::test]
    async fn mem0_manifest_health_check() {
        if skip_via_env() {
            return;
        }
        let manifest_path = repo_root().join("plugins/knowledge/mem0/plugin.toml");
        let manifest = PluginManifest::from_path(&manifest_path).expect("parse mem0 plugin.toml");

        // Manifest sanity (cheap pre-flight before paying the uvx cost).
        assert_eq!(manifest.plugin.name, "mem0");
        assert_eq!(manifest.transport.kind, "stdio");
        assert!(
            !manifest.capabilities.provides.is_empty(),
            "mem0 manifest must declare at least one provided capability",
        );

        let health = PluginManager::health_check_with_timeout(&manifest, FIRST_RUN_TIMEOUT_MS)
            .await
            .expect("health-check mem0 MCP server");

        assert_eq!(health.name, "mem0");
        assert_eq!(
            health.status,
            HealthStatus::Ok,
            "mem0 health-check returned non-Ok status: {:?}",
            health.status,
        );
        assert!(!health.tools.is_empty(), "mem0 advertised zero tools",);
        // `add_memory` is the canonical CRUD store tool advertised by
        // `mem0-mcp-server@0.2.1` (alongside `search_memories`,
        // `get_memories`, `get_memory`, `update_memory`,
        // `delete_memory`, `delete_all_memories`, `list_entities`,
        // `delete_entities` — 9 total). The master-plan §3.1 line 312
        // store/retrieve/list vocabulary maps to:
        //   store    → add_memory
        //   retrieve → get_memory / search_memories
        //   list     → get_memories / list_entities
        // Pinning on `add_memory` (the store side) gives the strongest
        // detection signal for upstream renames of the canonical
        // CRUD-write surface.
        assert!(
            health.tools.iter().any(|t| t == "add_memory"),
            "expected `add_memory` tool in advertised set, got: {:?}",
            health.tools,
        );
    }

    /// `graphiti_manifest_health_check` (P3-W9-F10) extends the
    /// skip-via-env pattern with two additional gates per WO-0079
    /// `scope_in` #5(iii) / #6: graphiti is operator-state-DUAL-
    /// DEPENDENT — the upstream `mcp_server` lifespan handler eagerly
    /// connects to (a) a graph DB (FalkorDB by default; Neo4j as
    /// alternative) AND (b) instantiates an LLM client whose
    /// constructor reads the API key from env.
    ///
    /// Verified at the pinned commit-sha (mcp-v1.0.2 / SHA
    /// 19e44a97a929ebf121294f97f26966f0379d8e30) via
    /// `/tmp/wo-0079-capture.py`:
    ///   * Without `FALKORDB_URI` / `NEO4J_URI`, startup raises
    ///     `RuntimeError: Database Connection Error: FalkorDB is
    ///     not running` and `tools/list` never responds.
    ///   * Without `OPENAI_API_KEY` (or another LLM-provider key),
    ///     startup raises `openai.OpenAIError: Missing credentials`
    ///     during `OpenAIClient.__init__` and `tools/list` never
    ///     responds.
    ///
    /// When either gate is missing, the test honours the skip-via-
    /// env early-return per WO-0069 `tests/g3_plugin_manifests.rs:34`
    /// precedent — keeping the cargo-test selectable on a clean
    /// developer workstation that cannot satisfy the dual operator-
    /// state requirements without the verifier setting opt-outs
    /// (forbidden per WO-0069 `scope_in` #5 / WO-0079 `scope_in`
    /// #25).
    fn graphiti_skip_via_env() -> bool {
        if skip_via_env() {
            return true;
        }
        let graph_db_set =
            std::env::var("FALKORDB_URI").is_ok() || std::env::var("NEO4J_URI").is_ok();
        let llm_key_set = std::env::var("OPENAI_API_KEY").is_ok()
            || std::env::var("ANTHROPIC_API_KEY").is_ok()
            || std::env::var("GROQ_API_KEY").is_ok()
            || std::env::var("GEMINI_API_KEY").is_ok()
            || std::env::var("AZURE_OPENAI_API_KEY").is_ok();
        !(graph_db_set && llm_key_set)
    }

    #[tokio::test]
    async fn graphiti_manifest_health_check() {
        if graphiti_skip_via_env() {
            return;
        }
        let manifest_path = repo_root().join("plugins/knowledge/graphiti/plugin.toml");
        let manifest =
            PluginManifest::from_path(&manifest_path).expect("parse graphiti plugin.toml");

        // Manifest sanity (cheap pre-flight before paying the uvx
        // git-fetch + dependency-resolution cost — the upstream's
        // ~50 transitive Python deps include graphiti-core, mcp,
        // openai, pydantic, falkordb, etc.).
        assert_eq!(manifest.plugin.name, "graphiti");
        assert_eq!(manifest.transport.kind, "stdio");
        assert!(
            !manifest.capabilities.provides.is_empty(),
            "graphiti manifest must declare at least one provided capability",
        );

        let health = PluginManager::health_check_with_timeout(&manifest, FIRST_RUN_TIMEOUT_MS)
            .await
            .expect("health-check graphiti MCP server");

        assert_eq!(health.name, "graphiti");
        assert_eq!(
            health.status,
            HealthStatus::Ok,
            "graphiti health-check returned non-Ok status: {:?}",
            health.status,
        );
        assert!(!health.tools.is_empty(), "graphiti advertised zero tools",);
        // `add_memory` is the canonical episode-ingest store tool
        // advertised by `graphiti mcp-v1.0.2` (alongside
        // `search_nodes`, `search_memory_facts`, `delete_entity_edge`,
        // `delete_episode`, `get_entity_edge`, `get_episodes`,
        // `clear_graph`, `get_status` — 9 total at the pinned
        // commit-sha 19e44a97a929ebf121294f97f26966f0379d8e30,
        // captured live via /tmp/wo-0079-capture.py during WO-0079
        // execution). The master-plan §3.1 line 313 / §17.2 store/
        // retrieve/list vocabulary maps to:
        //   store    → add_memory       (episode-backed knowledge ingest)
        //   retrieve → search_memory_facts / search_nodes
        //   list     → get_episodes / get_entity_edge
        // Pinning on `add_memory` (the store side) gives the
        // strongest detection signal for upstream renames of the
        // canonical episode-ingest write surface and mirrors the
        // mem0 `add_memory` pin + codebase-memory `search_graph`
        // pin (rationale at WO-0069 § / WO-0079 scope_in #7).
        // Pinning on a count-based assertion (`tools.len() >= 9`)
        // would tolerate benign upstream additions but mask
        // tool-rename / tool-removal regressions; per WO-0079
        // scope_out #21 we deliberately reject that shape.
        assert!(
            health.tools.iter().any(|t| t == "add_memory"),
            "expected `add_memory` tool in advertised set, got: {:?}",
            health.tools,
        );
    }
}
