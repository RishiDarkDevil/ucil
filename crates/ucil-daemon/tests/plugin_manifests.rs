//! End-to-end integration tests for the on-disk ast-grep + probe plugin
//! manifests (P2-W6-F05 / P2-W6-F06).
//!
//! Each test loads the on-disk manifest at
//! `plugins/<group>/<name>/plugin.toml`, drives the manifest's
//! `transport.command` as a real subprocess via
//! [`ucil_daemon::PluginManager::health_check_with_timeout`], and
//! asserts the live `tools/list` reply contains an expected tool name.
//!
//! Mocking `tokio::process::Command`, the spawned MCP server, or the
//! JSON-RPC dialogue is forbidden — the WO-0044 contract is precisely
//! that real MCP-server subprocesses speak real JSON-RPC over stdio
//! exactly the same way a Claude Code / Cursor / Cline client would
//! consume them at runtime. The test exercises the full handshake
//! [`ucil_daemon::PluginManager::health_check`] performs (`initialize`
//! → `notifications/initialized` → `tools/list`) end-to-end against the
//! real `npx -y @notprolands/ast-grep-mcp@1.1.1` /
//! `npx -y @probelabs/probe@0.6.0-rc315 mcp` invocations.
//!
//! Workspace fixtures exercised by the partner verify scripts
//! (`scripts/verify/P2-W6-F05.sh`, `scripts/verify/P2-W6-F06.sh`):
//! `tests/fixtures/typescript-project` for ast-grep structural search,
//! `tests/fixtures/rust-project` for probe function-body extraction.
//!
//! Set `UCIL_SKIP_EXTERNAL_PLUGIN_TESTS=1` only on truly offline CI
//! builds — the verifier MUST NOT set the skip env, per WO-0044
//! `scope_in`.
//!
//! Tests are wrapped in `mod plugin_manifests` so nextest reports them
//! as `plugin_manifests::ast_grep_manifest_health_check`,
//! `plugin_manifests::probe_manifest_health_check`,
//! `plugin_manifests::ripgrep_manifest_parses` (parse-only per DEC-0009),
//! and `plugin_manifests::zoekt_manifest_parses` (parse-only per DEC-0009;
//! WO-0086 / P3-W10-F15), matching the WO-0044 / WO-0051 / WO-0086
//! acceptance selectors. Same wrapper pattern as the existing
//! `mod plugin_manager` block in `tests/plugin_manager.rs:21`
//! (DEC-0007 frozen-selector module-root placement).

mod plugin_manifests {
    use std::path::PathBuf;

    use ucil_daemon::{HealthStatus, PluginManager, PluginManifest};

    /// Generous first-run npx download budget — `npx -y <pkg>` may
    /// fetch dozens of megabytes on a cold cache. Subsequent runs hit
    /// the npx cache and complete in well under a second; the
    /// production-default `HEALTH_CHECK_TIMEOUT_MS` (5 s) is therefore
    /// fine for steady-state daemon ticks but inadequate for the very
    /// first post-install integration-test run on a fresh workstation.
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
    /// air-gapped CI runners that cannot reach the npm registry. The
    /// verifier MUST NOT set this env var.
    fn skip_via_env() -> bool {
        std::env::var("UCIL_SKIP_EXTERNAL_PLUGIN_TESTS").is_ok()
    }

    #[tokio::test]
    async fn ast_grep_manifest_health_check() {
        if skip_via_env() {
            return;
        }
        let manifest_path = repo_root().join("plugins/structural/ast-grep/plugin.toml");
        let manifest =
            PluginManifest::from_path(&manifest_path).expect("parse ast-grep plugin.toml");

        // Manifest sanity (cheap pre-flight before paying the npx cost).
        assert_eq!(manifest.plugin.name, "ast-grep");
        assert_eq!(manifest.transport.kind, "stdio");
        assert!(
            !manifest.capabilities.provides.is_empty(),
            "ast-grep manifest must declare at least one provided capability",
        );

        let health = PluginManager::health_check_with_timeout(&manifest, FIRST_RUN_TIMEOUT_MS)
            .await
            .expect("health-check ast-grep MCP server");

        assert_eq!(health.name, "ast-grep");
        assert_eq!(
            health.status,
            HealthStatus::Ok,
            "ast-grep health-check returned non-Ok status: {:?}",
            health.status,
        );
        assert!(!health.tools.is_empty(), "ast-grep advertised zero tools",);
        // `find_code` is the canonical structural-search tool advertised
        // by `@notprolands/ast-grep-mcp@1.1.1` (alongside
        // `dump_syntax_tree`, `rewrite_code`, …); pinning on the
        // existence of this exact name makes the test fail loudly if
        // the upstream package's tool surface drifts.
        assert!(
            health.tools.iter().any(|t| t == "find_code"),
            "expected `find_code` tool in advertised set, got: {:?}",
            health.tools,
        );
    }

    #[tokio::test]
    async fn probe_manifest_health_check() {
        if skip_via_env() {
            return;
        }
        let manifest_path = repo_root().join("plugins/search/probe/plugin.toml");
        let manifest = PluginManifest::from_path(&manifest_path).expect("parse probe plugin.toml");

        // Manifest sanity (cheap pre-flight before paying the npx cost).
        assert_eq!(manifest.plugin.name, "probe");
        assert_eq!(manifest.transport.kind, "stdio");
        assert!(
            !manifest.capabilities.provides.is_empty(),
            "probe manifest must declare at least one provided capability",
        );

        let health = PluginManager::health_check_with_timeout(&manifest, FIRST_RUN_TIMEOUT_MS)
            .await
            .expect("health-check probe MCP server");

        assert_eq!(health.name, "probe");
        assert_eq!(
            health.status,
            HealthStatus::Ok,
            "probe health-check returned non-Ok status: {:?}",
            health.status,
        );
        assert!(!health.tools.is_empty(), "probe advertised zero tools",);
        // `search_code` is the canonical semantic-search tool advertised
        // by `probe mcp` from `@probelabs/probe@0.6.0-rc315` (alongside
        // `extract_code`, `grep`); pinning on this exact name surfaces
        // upstream tool-surface drift loudly.
        assert!(
            health.tools.iter().any(|t| t == "search_code"),
            "expected `search_code` tool in advertised set, got: {:?}",
            health.tools,
        );
    }

    // DEC-0009 (search-code-in-process-ripgrep): ripgrep runs in-process
    // via `crates/ucil-daemon/src/text_search.rs` from WO-0035, NOT as a
    // long-lived MCP server. The manifest's `[transport]` table is a
    // declarative sentinel that satisfies the `PluginManifest` schema but
    // is never spawned. This test is therefore PARSE-ONLY — calling
    // `PluginManager::health_check` here would spawn `rg --version`,
    // which exits without speaking JSON-RPC and would (correctly) fail.
    #[test]
    fn ripgrep_manifest_parses() {
        let manifest_path = repo_root().join("plugins/search/ripgrep/plugin.toml");
        let manifest =
            PluginManifest::from_path(&manifest_path).expect("ripgrep manifest must parse cleanly");

        assert_eq!(
            manifest.plugin.name, "ripgrep",
            "plugin.name must be exactly `ripgrep`; observed `{}`",
            manifest.plugin.name,
        );
        assert!(
            manifest
                .capabilities
                .provides
                .contains(&"search.text".to_string()),
            "capabilities.provides must include `search.text`; observed {:?}",
            manifest.capabilities.provides,
        );
        assert!(
            manifest.capabilities.languages.is_empty(),
            "capabilities.languages must be empty (ripgrep is language-agnostic); observed {:?}",
            manifest.capabilities.languages,
        );
        assert_eq!(
            manifest.transport.kind, "stdio",
            "transport.type must be `stdio`; observed `{}`",
            manifest.transport.kind,
        );
        let lifecycle = manifest
            .lifecycle
            .as_ref()
            .expect("ripgrep manifest must declare a [lifecycle] section");
        assert!(
            !lifecycle.hot_cold,
            "lifecycle.hot_cold must be false (ripgrep is per-query spawn-and-exit); observed true",
        );
    }

    // DEC-0009 (search-code-in-process-ripgrep, generalised): Zoekt is an
    // external-service / CLI tool, NOT an MCP server. The manifest's
    // [transport] table is a declarative sentinel that satisfies the
    // `PluginManifest` schema but is never spawned. This test is therefore
    // PARSE-ONLY — calling `PluginManager::health_check` here would spawn
    // `zoekt --help`, which exits without speaking JSON-RPC and would
    // (correctly) fail. The runtime path for `search.text` queries against
    // Zoekt is `zoekt-index` (offline indexer) + `zoekt` CLI directly
    // (see scripts/verify/P3-W10-F15.sh for the smoke harness).
    #[test]
    fn zoekt_manifest_parses() {
        let manifest =
            PluginManifest::from_path(repo_root().join("plugins/search/zoekt/plugin.toml"))
                .expect("zoekt manifest must parse cleanly");

        assert_eq!(
            manifest.plugin.name, "zoekt",
            "(SA1) plugin.name must equal zoekt; observed {:?}",
            manifest.plugin.name,
        );
        assert!(
            manifest
                .capabilities
                .provides
                .contains(&"search.text".to_string()),
            "(SA2) capabilities.provides must include search.text; observed {:?}",
            manifest.capabilities.provides,
        );
        assert!(
            manifest.capabilities.languages.is_empty(),
            "(SA3) capabilities.languages must be empty (zoekt is language-agnostic trigram); observed {:?}",
            manifest.capabilities.languages,
        );
        assert_eq!(
            manifest.transport.kind, "stdio",
            "(SA4) transport.type must be stdio (declarative sentinel per DEC-0009); observed {:?}",
            manifest.transport.kind,
        );
        let lifecycle = manifest
            .lifecycle
            .as_ref()
            .expect("(SA5) zoekt manifest must declare a [lifecycle] section");
        assert!(
            !lifecycle.hot_cold,
            "(SA6) lifecycle.hot_cold must be false (zoekt index queries are per-call from UCIL's planner); observed true",
        );
    }
}
