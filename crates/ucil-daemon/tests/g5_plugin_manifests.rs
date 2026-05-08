//! End-to-end integration tests for the on-disk Context7 + Repomix G5
//! (Context) plugin manifests (P3-W10-F02 / P3-W10-F03).
//!
//! Each test loads the on-disk manifest at
//! `plugins/context/<name>/plugin.toml`, drives the manifest's
//! `transport.command` as a real subprocess via
//! [`ucil_daemon::PluginManager::health_check_with_timeout`], and
//! asserts the live `tools/list` reply contains an expected canonical
//! tool name.
//!
//! Mocking `tokio::process::Command`, the spawned MCP server, or the
//! JSON-RPC dialogue is forbidden — the WO-0069 contract carried by
//! this WO is precisely that real MCP-server subprocesses speak real
//! JSON-RPC over stdio exactly the same way a Claude Code / Cursor /
//! Cline client would consume them at runtime. Each test exercises the
//! full handshake [`ucil_daemon::PluginManager::health_check`] performs
//! (`initialize` → `notifications/initialized` → `tools/list`) end-to-
//! end against the real `npx -y <pinned-pkg>` invocation.
//!
//! Set `UCIL_SKIP_EXTERNAL_PLUGIN_TESTS=1` only on truly offline CI
//! builds (skips ALL external plugin-manifest suites including this
//! G5 suite); set the G5-specific `UCIL_SKIP_CONTEXT_PLUGIN_E2E=1` to
//! skip ONLY this suite (so an operator can keep the WO-0044 / WO-0069
//! / WO-0071 regression coverage without paying the additional
//! ~30-second-cold-cache npx fetches for the G5 plugins). The verifier
//! MUST NOT set EITHER opt-out, per WO-0069 `scope_in` #5 carried by
//! this WO.
//!
//! Tests are wrapped in `mod g5_plugin_manifests` so nextest reports
//! them as `g5_plugin_manifests::context7_manifest_health_check` and
//! `g5_plugin_manifests::repomix_manifest_health_check` matching the
//! WO-0074 acceptance selectors. Same wrapper pattern as the existing
//! `mod g4_plugin_manifests` block in `tests/g4_plugin_manifests.rs:46`
//! (DEC-0007 frozen-selector module-root placement; carried per
//! WO-0068 lessons §"For planner" frozen-test selector substring-
//! match REQUIRES module-root placement). NO `mod tests { ... }`
//! nesting; the test functions live at `mod g5_plugin_manifests` ROOT
//! per WO-0073 lessons §"For planner".
//!
//! This file is a peer of `tests/g3_plugin_manifests.rs` (G3 suite),
//! `tests/g4_plugin_manifests.rs` (G4 suite), and
//! `tests/plugin_manifests.rs` (WO-0044 G2 regression guard) — four
//! group-isolated suites kept distinct so each group's
//! `UCIL_SKIP_<GROUP>_PLUGIN_E2E` opt-out is scoped distinctly. Per
//! WO-0069 lessons §executor #2 ("write a SEPARATE integration test
//! file per phase/group" — single-file-per-group keeps the
//! architecture / knowledge / search / context test surfaces isolated
//! and avoids cross-group flake propagation).

mod g5_plugin_manifests {
    use std::path::PathBuf;

    use ucil_daemon::{HealthStatus, PluginManager, PluginManifest};

    /// Generous first-run npx download budget — `npx -y <pkg>` may
    /// fetch a tarball + transitive deps on a cold cache (Repomix in
    /// particular pulls Tree-sitter + globby + clipanion + ~80 small
    /// deps). Subsequent runs hit the cache and complete in well under
    /// a second; the production-default `HEALTH_CHECK_TIMEOUT_MS` (5 s)
    /// is therefore fine for steady-state daemon ticks but inadequate
    /// for the very first post-install integration-test run on a fresh
    /// workstation. Mirror the WO-0044 / WO-0069 / WO-0072 budget
    /// exactly.
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
    /// air-gapped CI runners that cannot reach npm at all (this is the
    /// same global opt-out honoured by the WO-0044
    /// `tests/plugin_manifests.rs`, WO-0069 `tests/g3_plugin_manifests.rs`,
    /// and WO-0072 `tests/g4_plugin_manifests.rs` suites) AND the
    /// G5-specific `UCIL_SKIP_CONTEXT_PLUGIN_E2E` opt-out for operators
    /// that want to keep the existing G2/G3/G4 regression coverage but
    /// skip the additional Context7 + Repomix cold-cache budget. Either
    /// env set means "skip this test"; the verifier MUST NOT set
    /// either, per WO-0069 `scope_in` #5 carried by WO-0074
    /// `scope_in` #5.
    fn skip_via_env() -> bool {
        std::env::var("UCIL_SKIP_EXTERNAL_PLUGIN_TESTS").is_ok()
            || std::env::var("UCIL_SKIP_CONTEXT_PLUGIN_E2E").is_ok()
    }

    #[tokio::test]
    async fn context7_manifest_health_check() {
        if skip_via_env() {
            return;
        }
        let manifest_path = repo_root().join("plugins/context/context7/plugin.toml");
        let manifest =
            PluginManifest::from_path(&manifest_path).expect("parse context7 plugin.toml");

        // Manifest sanity (cheap pre-flight before paying the npx cost).
        assert_eq!(manifest.plugin.name, "context7");
        assert_eq!(manifest.transport.kind, "stdio");
        assert!(
            !manifest.capabilities.provides.is_empty(),
            "context7 manifest must declare at least one provided capability",
        );
        assert!(
            manifest
                .capabilities
                .provides
                .iter()
                .all(|c| c.starts_with("context.")),
            "context7 manifest must declare its capabilities under the context.* namespace, got: {:?}",
            manifest.capabilities.provides,
        );

        let health = PluginManager::health_check_with_timeout(&manifest, FIRST_RUN_TIMEOUT_MS)
            .await
            .expect("health-check context7 MCP server");

        assert_eq!(health.name, "context7");
        assert_eq!(
            health.status,
            HealthStatus::Ok,
            "context7 health-check returned non-Ok status: {:?}",
            health.status,
        );
        assert!(!health.tools.is_empty(), "context7 advertised zero tools");
        // `resolve-library-id` is the canonical disambiguation tool
        // advertised by `@upstash/context7-mcp@2.2.4` (alongside the
        // `query-docs` library-doc-fetch tool — 2 total). The master-
        // plan §3.1 line 311 + §4.5 line 343 vocabulary maps to:
        //   library docs → resolve-library-id (find) + query-docs (read)
        // Pinning on `resolve-library-id` (the upstream entry-point
        // surface that the README documents as MUST-call-first) gives
        // the strongest detection signal for upstream renames of the
        // canonical disambiguation surface, mirroring the WO-0072
        // codegraphcontext `analyze_code_relationships` rationale (count
        // / language drifts on benign upstream additions; tool-name pin
        // surfaces real surface drift loudly). Note: kebab-case
        // `resolve-library-id` is the upstream literal as emitted by
        // `tools/list` — preferred over snake-case translation per
        // WO-0074 scope_in #1.
        assert!(
            health.tools.iter().any(|t| t == "resolve-library-id"),
            "(SA1) expected `resolve-library-id` tool in advertised set; got: {:?}",
            health.tools,
        );
    }

    #[tokio::test]
    async fn repomix_manifest_health_check() {
        if skip_via_env() {
            return;
        }
        let manifest_path = repo_root().join("plugins/context/repomix/plugin.toml");
        let manifest =
            PluginManifest::from_path(&manifest_path).expect("parse repomix plugin.toml");

        // Manifest sanity (cheap pre-flight before paying the npx cost).
        assert_eq!(manifest.plugin.name, "repomix");
        assert_eq!(manifest.transport.kind, "stdio");
        assert!(
            !manifest.capabilities.provides.is_empty(),
            "repomix manifest must declare at least one provided capability",
        );
        assert!(
            manifest
                .capabilities
                .provides
                .iter()
                .all(|c| c.starts_with("context.")),
            "repomix manifest must declare its capabilities under the context.* namespace, got: {:?}",
            manifest.capabilities.provides,
        );

        let health = PluginManager::health_check_with_timeout(&manifest, FIRST_RUN_TIMEOUT_MS)
            .await
            .expect("health-check repomix MCP server");

        assert_eq!(health.name, "repomix");
        assert_eq!(
            health.status,
            HealthStatus::Ok,
            "repomix health-check returned non-Ok status: {:?}",
            health.status,
        );
        assert!(!health.tools.is_empty(), "repomix advertised zero tools");
        // `pack_codebase` is the canonical local-repo packing tool
        // advertised by `repomix@1.14.0` (alongside
        // `pack_remote_repository`, `generate_skill`,
        // `attach_packed_output`, `read_repomix_output`,
        // `grep_repomix_output`, `file_system_read_file`,
        // `file_system_read_directory` — 8 total). The master-plan
        // §3.1 line 311 + §4.5 line 344 vocabulary maps directly:
        //   repository pack → pack_codebase (local) +
        //                     pack_remote_repository (remote clone)
        //   token reduction → pack_codebase + Tree-sitter compress
        //   read packed     → read_repomix_output / grep_repomix_output
        // Pinning on `pack_codebase` (the load-bearing surface for the
        // F03 token-reduction acceptance line) gives the strongest
        // detection signal for upstream renames of the canonical
        // local-repo-pack surface. Note: snake_case `pack_codebase` is
        // the upstream literal as emitted by `tools/list` — preferred
        // over kebab-case translation per WO-0074 scope_in #2.
        assert!(
            health.tools.iter().any(|t| t == "pack_codebase"),
            "(SA2) expected `pack_codebase` tool in advertised set; got: {:?}",
            health.tools,
        );
    }
}
