//! End-to-end integration tests for the on-disk codegraphcontext G4
//! (Architecture) plugin manifest (P3-W9-F08).
//!
//! Each test loads the on-disk manifest at
//! `plugins/architecture/<name>/plugin.toml`, drives the manifest's
//! `transport.command` as a real subprocess via
//! [`ucil_daemon::PluginManager::health_check_with_timeout`], and
//! asserts the live `tools/list` reply contains an expected tool name.
//!
//! Mocking `tokio::process::Command`, the spawned MCP server, or the
//! JSON-RPC dialogue is forbidden — the WO-0069 contract carried by
//! this WO is precisely that real MCP-server subprocesses speak real
//! JSON-RPC over stdio exactly the same way a Claude Code / Cursor /
//! Cline client would consume them at runtime. The test exercises the
//! full handshake [`ucil_daemon::PluginManager::health_check`] performs
//! (`initialize` → `notifications/initialized` → `tools/list`) end-to-
//! end against the real `uvx --with falkordblite codegraphcontext@0.4.7
//! mcp start` invocation.
//!
//! Set `UCIL_SKIP_EXTERNAL_PLUGIN_TESTS=1` only on truly offline CI
//! builds (skips both WO-0044 and WO-0069 plugin-manifest suites and
//! this G4 suite); set the G4-specific
//! `UCIL_SKIP_ARCHITECTURE_PLUGIN_E2E=1` to skip ONLY this suite (so an
//! operator can keep the WO-0044 / WO-0069 regression coverage without
//! paying the additional ~90-second-cold-cache uvx fetches for the G4
//! plugin). The verifier MUST NOT set EITHER opt-out, per WO-0069
//! `scope_in` #5 carried by this WO.
//!
//! Tests are wrapped in `mod g4_plugin_manifests` so nextest reports
//! them as `g4_plugin_manifests::codegraphcontext_manifest_health_check`
//! matching the WO-0071 acceptance selector. Same wrapper pattern as
//! the existing `mod g3_plugin_manifests` block in
//! `tests/g3_plugin_manifests.rs:51` (DEC-0007 frozen-selector module-
//! root placement; carried per WO-0068 lessons §"For planner" frozen-
//! test selector substring-match REQUIRES module-root placement).
//!
//! This file is a peer of `tests/g3_plugin_manifests.rs` (G3 suite) and
//! `tests/plugin_manifests.rs` (WO-0044 G2 regression guard) — three
//! group-isolated suites kept distinct so each group's
//! `UCIL_SKIP_<GROUP>_PLUGIN_E2E` opt-out is scoped distinctly. Per
//! WO-0069 lessons §executor #2 ("write a SEPARATE integration test
//! file per phase/group" — single-file-per-group keeps the
//! architecture / knowledge / search test surfaces isolated and
//! avoids cross-group flake propagation).

mod g4_plugin_manifests {
    use std::path::PathBuf;

    use ucil_daemon::{HealthStatus, PluginManager, PluginManifest};

    /// Generous first-run uvx download budget — `uvx <pkg>` may resolve
    /// dozens of transitive Python deps (codegraphcontext pulls
    /// falkordblite, mcp, networkx, pydantic, etc. — typically ~60
    /// packages on a fresh uv cache). Subsequent runs hit the cache and
    /// complete in well under a second; the production-default
    /// `HEALTH_CHECK_TIMEOUT_MS` (5 s) is therefore fine for steady-
    /// state daemon ticks but inadequate for the very first post-
    /// install integration-test run on a fresh workstation. Mirror the
    /// WO-0044 / WO-0069 budget exactly.
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
    /// air-gapped CI runners that cannot reach pypi at all (this is the
    /// same global opt-out honoured by the WO-0044
    /// `tests/plugin_manifests.rs` and WO-0069
    /// `tests/g3_plugin_manifests.rs` suites) AND the G4-specific
    /// `UCIL_SKIP_ARCHITECTURE_PLUGIN_E2E` opt-out for operators that
    /// want to keep the existing G2/G3 regression coverage but skip the
    /// additional codegraphcontext cold-cache budget. Either env set
    /// means "skip this test"; the verifier MUST NOT set either, per
    /// WO-0069 `scope_in` #5 carried by WO-0071 `scope_in` #28.
    fn skip_via_env() -> bool {
        std::env::var("UCIL_SKIP_EXTERNAL_PLUGIN_TESTS").is_ok()
            || std::env::var("UCIL_SKIP_ARCHITECTURE_PLUGIN_E2E").is_ok()
    }

    #[tokio::test]
    async fn codegraphcontext_manifest_health_check() {
        if skip_via_env() {
            return;
        }
        let manifest_path = repo_root().join("plugins/architecture/codegraphcontext/plugin.toml");
        let manifest =
            PluginManifest::from_path(&manifest_path).expect("parse codegraphcontext plugin.toml");

        // Manifest sanity (cheap pre-flight before paying the uvx cost).
        assert_eq!(manifest.plugin.name, "codegraphcontext");
        assert_eq!(manifest.transport.kind, "stdio");
        assert!(
            !manifest.capabilities.provides.is_empty(),
            "codegraphcontext manifest must declare at least one provided capability",
        );
        assert!(
            manifest
                .capabilities
                .provides
                .iter()
                .all(|c| c.starts_with("architecture.")),
            "codegraphcontext manifest must declare its capabilities under the architecture.* namespace, got: {:?}",
            manifest.capabilities.provides,
        );

        let health = PluginManager::health_check_with_timeout(&manifest, FIRST_RUN_TIMEOUT_MS)
            .await
            .expect("health-check codegraphcontext MCP server");

        assert_eq!(health.name, "codegraphcontext");
        assert_eq!(
            health.status,
            HealthStatus::Ok,
            "codegraphcontext health-check returned non-Ok status: {:?}",
            health.status,
        );
        assert!(
            !health.tools.is_empty(),
            "codegraphcontext advertised zero tools",
        );
        // `analyze_code_relationships` is the canonical blast-radius
        // tool advertised by `codegraphcontext@0.4.7` (alongside
        // `add_code_to_graph`, `find_code`, `execute_cypher_query`,
        // `find_dead_code`, `calculate_cyclomatic_complexity`,
        // `find_most_complex_functions`, etc. — 25 total). The master-
        // plan §4.4 line 326 vocabulary maps to:
        //   dependency graph → add_code_to_graph (build) +
        //                      analyze_code_relationships (query)
        //   blast radius     → analyze_code_relationships (impact)
        //   search           → find_code
        // Pinning on `analyze_code_relationships` (the blast-radius
        // surface) gives the strongest detection signal for upstream
        // renames of the canonical impact-analysis surface, mirroring
        // the codebase-memory `search_graph` and mem0 `add_memory`
        // rationale (count drifts on benign upstream additions; tool-
        // name pin surfaces real surface drift loudly).
        assert!(
            health
                .tools
                .iter()
                .any(|t| t == "analyze_code_relationships"),
            "expected `analyze_code_relationships` tool in advertised set, got: {:?}",
            health.tools,
        );
    }
}
