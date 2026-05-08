//! End-to-end integration tests for the on-disk mcp-pytest-runner +
//! test-runner G8 (Testing+CI) plugin manifests (P3-W11-F07 +
//! P3-W11-F08).
//!
//! Each test loads an on-disk manifest under `plugins/testing/...`,
//! drives the manifest's `transport.command` as a real subprocess via
//! [`ucil_daemon::PluginManager::health_check_with_timeout`], and
//! asserts the live `tools/list` reply contains an expected canonical
//! tool name.
//!
//! Mocking `tokio::process::Command`, the spawned MCP server, or the
//! JSON-RPC dialogue is forbidden — the WO-0069 contract carried by
//! this WO is precisely that real MCP-server subprocesses speak real
//! JSON-RPC over stdio exactly the same way a Claude Code / Cursor /
//! Cline client would consume them at runtime. The tests exercise the
//! full handshake [`ucil_daemon::PluginManager::health_check`] performs
//! (`initialize` → `notifications/initialized` → `tools/list`) end-to-
//! end against the real `uvx mcp-pytest-runner@0.2.1` and
//! `npx -y @iflow-mcp/mcp-test-runner@0.2.1` invocations.
//!
//! Set `UCIL_SKIP_EXTERNAL_PLUGIN_TESTS=1` only on truly offline CI
//! builds (skips ALL external plugin-manifest suites including this
//! G8 suite); set the G8-specific `UCIL_SKIP_TESTING_PLUGIN_E2E=1`
//! to skip ONLY this suite (so an operator can keep the WO-0044 /
//! WO-0069 / WO-0072 / WO-0074 / WO-0075 / WO-0076 regression
//! coverage without paying the additional ~30-second-cold-cache
//! uvx fetch for the mcp-pytest-runner pypi package and pytest
//! transitive deps OR the cold-cache npx fetch for the
//! @iflow-mcp/mcp-test-runner npm package and
//! @modelcontextprotocol/sdk transitive deps). Both tests in this
//! file honour both env vars; the verifier MUST NOT set EITHER
//! opt-out, per WO-0077 `scope_in` #10 carry-over and WO-0082
//! `scope_in` #25 (and WO-0076 verifier protocol).
//!
//! pyproject.toml-side note (per the manifest's top-of-file rustdoc
//! at `plugins/testing/mcp-pytest-runner/plugin.toml`): the upstream
//! `discover_tests` tool drives a pytest collection pass against the
//! spawned binary's cwd. Pytest collection of the
//! `tests/fixtures/python-project` fixture requires the
//! `python_project` package on `sys.path` (the fixture's
//! `pyproject.toml` declares it under `src/python_project/` but does
//! NOT carry an installed-mode `pip install -e .` artifact). The
//! integration test does NOT exercise `discover_tests` itself — only
//! the `tools/list` handshake — so no fixture-side `conftest.py` is
//! required for this test. The verify script (`scripts/verify/
//! P3-W11-F08.sh`) does fabricate a `conftest.py` in its tmpdir copy
//! for the tool-level smoke that exercises `discover_tests` +
//! `execute_tests` end-to-end.
//!
//! Tests are wrapped in `mod g8_plugin_manifests` so nextest reports
//! them as `g8_plugin_manifests::mcp_pytest_runner_manifest_health_check`
//! and `g8_plugin_manifests::test_runner_manifest_health_check`
//! matching the WO-0077 + WO-0082 acceptance selectors. Same wrapper
//! pattern as the existing `mod g7_plugin_manifests` block in
//! `tests/g7_plugin_manifests.rs:75` (DEC-0007 frozen-selector
//! module-root placement; carried per WO-0068 lessons §"For planner"
//! frozen-test selector substring-match REQUIRES module-root
//! placement). NO `mod tests { ... }` nesting; the test functions
//! live at `mod g8_plugin_manifests` ROOT per WO-0073 lessons §"For
//! planner".
//!
//! This file is a peer of `tests/g3_plugin_manifests.rs` (G3 suite),
//! `tests/g4_plugin_manifests.rs` (G4 suite),
//! `tests/g5_plugin_manifests.rs` (G5 suite),
//! `tests/g6_plugin_manifests.rs` (G6 suite),
//! `tests/g7_plugin_manifests.rs` (G7 suite), and
//! `tests/plugin_manifests.rs` (WO-0044 G2 regression guard) — seven
//! group-isolated suites kept distinct so each group's
//! `UCIL_SKIP_<GROUP>_PLUGIN_E2E` opt-out is scoped distinctly. Per
//! WO-0069 lessons §executor #2 ("write a SEPARATE integration test
//! file per phase/group" — single-file-per-group keeps the
//! architecture / knowledge / search / context / platform / quality
//! / testing test surfaces isolated and avoids cross-group flake
//! propagation).
//!
//! All fixture-init / tmpdir-mutation helpers in async test bodies
//! use the tokio variant of the process-spawn API (NOT the blocking
//! standard-library variant) per WO-0075 lesson §executor W1 — pre-
//! emptively applied here to avoid the WO-0075 W1 critic warning.
//! Rule reference: `.claude/rules/rust-style.md` §Async line 23
//! mandates the tokio process variant in async paths. The
//! pre-emptive grep-AC for the standard-library spawn API on this
//! source file enforces the discipline at the file level (per
//! WO-0077 acceptance_criteria).
//!
//! test-runner-mcp (P3-W11-F07) revived per DEC-0024 + DEC-0025 —
//! single-tool dispatcher, `npx -y @iflow-mcp/mcp-test-runner@0.2.1`.
//! The deferral chain DEC-0019 → DEC-0020 → DEC-0021 (graphiti, ruff,
//! test-runner-mcp) is fully discharged by the matching revival
//! chain DEC-0022 (graphiti) → DEC-0023 (ruff) →
//! DEC-0024+DEC-0025 (test-runner-mcp). The chain-of-corrections is
//! documented in detail in
//! `plugins/testing/test-runner/plugin.toml`'s top-of-file rustdoc
//! §Provenance section: the bare `test-runner-mcp` name is NOT on
//! npm (404); the only published surface is the third-party scoped
//! mirror `@iflow-mcp/mcp-test-runner@0.2.1`; upstream advertises
//! ONE `run_tests` tool with a framework enum (NOT 6 separate
//! tools as DEC-0024 incorrectly described). DEC-0025 §Decision
//! point 3 amended DEC-0024 §Decision point 3 — the canonical
//! assertion is `health.tools.len() == 1` AND `health.tools[0] ==
//! "run_tests"` (the framework-enum-coverage assertion lives in
//! the verify script's independent JSON-RPC capture which inspects
//! the inputSchema; `health.tools` from
//! `PluginManager::health_check_with_timeout` exposes tool names
//! only).

mod g8_plugin_manifests {
    use std::path::PathBuf;

    use ucil_daemon::{HealthStatus, PluginManager, PluginManifest};

    /// Generous first-run package-manager download budget. `uvx
    /// <pkg>` (mcp-pytest-runner) may fetch the pypi tarball +
    /// transitive deps (pytest, pluggy, anyio, etc.) on a cold cache.
    /// `npx -y <pkg>` (test-runner) may fetch the npm tarball +
    /// transitive deps (@modelcontextprotocol/sdk, etc.) on a cold
    /// cache. Subsequent runs hit the local package-manager cache and
    /// complete in well under a second; the production-default
    /// `HEALTH_CHECK_TIMEOUT_MS` (5 s) is therefore fine for steady-
    /// state daemon ticks but inadequate for the very first post-
    /// install integration-test run on a fresh workstation. Mirror
    /// the WO-0044 / WO-0069 / WO-0072 / WO-0074 / WO-0075 / WO-0076
    /// budget exactly.
    const FIRST_RUN_TIMEOUT_MS: u64 = 120_000;

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
    /// air-gapped CI runners that cannot reach pypi at all (this is
    /// the same global opt-out honoured by the WO-0044
    /// `tests/plugin_manifests.rs`, WO-0069 `tests/g3_plugin_manifests.rs`,
    /// WO-0072 `tests/g4_plugin_manifests.rs`, WO-0074
    /// `tests/g5_plugin_manifests.rs`, WO-0075
    /// `tests/g6_plugin_manifests.rs`, and WO-0076
    /// `tests/g7_plugin_manifests.rs` suites) AND the G8-specific
    /// `UCIL_SKIP_TESTING_PLUGIN_E2E` opt-out for operators that want
    /// to keep the existing G2/G3/G4/G5/G6/G7 regression coverage
    /// but skip the additional mcp-pytest-runner cold-cache budget.
    /// Either env set means "skip this test"; the verifier MUST NOT
    /// set either, per WO-0077 `scope_in` #10.
    fn skip_via_env() -> bool {
        std::env::var("UCIL_SKIP_EXTERNAL_PLUGIN_TESTS").is_ok()
            || std::env::var("UCIL_SKIP_TESTING_PLUGIN_E2E").is_ok()
    }

    #[tokio::test]
    async fn mcp_pytest_runner_manifest_health_check() {
        if skip_via_env() {
            return;
        }
        let manifest_path = repo_root().join("plugins/testing/mcp-pytest-runner/plugin.toml");
        let manifest =
            PluginManifest::from_path(&manifest_path).expect("parse mcp-pytest-runner plugin.toml");

        // Manifest sanity (cheap pre-flight before paying the uvx cost).
        assert_eq!(manifest.plugin.name, "mcp-pytest-runner");
        assert_eq!(manifest.transport.kind, "stdio");
        assert!(
            !manifest.capabilities.provides.is_empty(),
            "mcp-pytest-runner manifest must declare at least one provided capability",
        );
        assert!(
            manifest
                .capabilities
                .provides
                .iter()
                .all(|c| c.starts_with("testing.")),
            "mcp-pytest-runner manifest must declare its capabilities under the testing.* \
             namespace, got: {:?}",
            manifest.capabilities.provides,
        );

        let health = PluginManager::health_check_with_timeout(&manifest, FIRST_RUN_TIMEOUT_MS)
            .await
            .expect("health-check mcp-pytest-runner MCP server");

        assert_eq!(health.name, "mcp-pytest-runner");
        assert_eq!(
            health.status,
            HealthStatus::Ok,
            "mcp-pytest-runner health-check returned non-Ok status: {:?}",
            health.status,
        );
        assert!(
            !health.tools.is_empty(),
            "mcp-pytest-runner advertised zero tools",
        );
        // `discover_tests` is the canonical pytest-discovery tool
        // advertised by `mcp-pytest-runner@0.2.1` (alongside
        // `execute_tests` — full live list captured in the manifest's
        // top-of-file rustdoc comment). The master-plan §4.8 line 405
        // vocabulary
        //   pytest-runner — pytest hierarchical test discovery and
        //   selective re-run by node ID
        // describes the capability category independent of the
        // upstream literal tool name. Pinning on `discover_tests`
        // (the upstream literal that maps verbatim to our declared
        // `testing.pytest.discover` capability) gives the strongest
        // detection signal for upstream renames mirroring the
        // WO-0072 / WO-0074 / WO-0075 / WO-0076 rationale; pytest
        // discovery is the canonical pytest entry-point per F08
        // spec. Note: snake_case `discover_tests` is the upstream
        // literal as emitted by `tools/list` — preferred over
        // kebab-case translation per WO-0074 scope_in #1 lesson +
        // WO-0076 scope_in §11 lesson.
        assert!(
            health.tools.iter().any(|t| t == "discover_tests"),
            "(SA1) expected `discover_tests` tool in advertised set; got: {:?}; want: \
             \"discover_tests\"",
            health.tools,
        );
    }

    /// End-to-end health check for the test-runner G8 manifest pinned
    /// to `npx -y @iflow-mcp/mcp-test-runner@0.2.1` per DEC-0025
    /// §Decision point 1. Mirrors the
    /// `mcp_pytest_runner_manifest_health_check` shape exactly:
    /// load manifest, manifest sanity, real-subprocess
    /// `tools/list` handshake, assertions on the live capture.
    ///
    /// DEC-0025 §Decision point 3 (amends DEC-0024) — the canonical
    /// upstream advertises ONE tool `run_tests` with a framework
    /// enum, NOT six separate tools. The load-bearing assertion is
    /// `health.tools.len() == 1` AND `health.tools[0] ==
    /// "run_tests"` (the framework-enum-coverage assertion lives
    /// in the verify script's independent JSON-RPC capture which
    /// inspects the inputSchema; `health.tools` from
    /// `PluginManager::health_check_with_timeout` exposes tool
    /// names only — same shape as the
    /// `mcp_pytest_runner_manifest_health_check` test for F08).
    ///
    /// `health.name` reflects the live `serverInfo.name` per the
    /// `initialize` reply, NOT the manifest's `[plugin] name`
    /// (these happen to coincide for test-runner — both are
    /// `"test-runner"` — but tracking the upstream literal verbatim
    /// guards against silent upstream renames).
    #[tokio::test]
    async fn test_runner_manifest_health_check() {
        if skip_via_env() {
            return;
        }
        let manifest_path = repo_root().join("plugins/testing/test-runner/plugin.toml");
        let manifest =
            PluginManifest::from_path(&manifest_path).expect("parse test-runner plugin.toml");

        // Manifest sanity (cheap pre-flight before paying the npx cost).
        assert_eq!(manifest.plugin.name, "test-runner");
        assert_eq!(manifest.transport.kind, "stdio");
        assert!(
            !manifest.capabilities.provides.is_empty(),
            "test-runner manifest must declare at least one provided capability",
        );
        assert!(
            manifest
                .capabilities
                .provides
                .iter()
                .all(|c| c.starts_with("testing.")),
            "test-runner manifest must declare its capabilities under the testing.* \
             namespace, got: {:?}",
            manifest.capabilities.provides,
        );

        let health = PluginManager::health_check_with_timeout(&manifest, FIRST_RUN_TIMEOUT_MS)
            .await
            .expect("health-check test-runner MCP server");

        // `health.name` reflects the live serverInfo.name from the
        // `initialize` reply per DEC-0025 §Context — both
        // serverInfo.name and the manifest's [plugin] name field
        // resolve to the literal "test-runner".
        assert_eq!(health.name, "test-runner");
        assert_eq!(
            health.status,
            HealthStatus::Ok,
            "test-runner health-check returned non-Ok status: {:?}",
            health.status,
        );
        // DEC-0025 §Decision point 3: upstream advertises EXACTLY 1
        // tool — single-tool dispatcher pattern. This load-bearing
        // assertion catches a regression in either direction
        // (tool dropped → 0; new sibling tool added → 2+) since
        // either drift would invalidate the manifest's pinned
        // tool-surface assumptions and would warrant a fresh ADR
        // bumping the pin.
        assert_eq!(
            health.tools.len(),
            1,
            "(SA1) expected exactly 1 tool from test-runner (single-tool dispatcher per \
             DEC-0025 §Decision point 3); got: {:?}",
            health.tools,
        );
        // The canonical upstream tool name per DEC-0025 §Context.
        // Snake_case as emitted by tools/list — preferred over
        // kebab-case translation per WO-0074 scope_in #1 lesson +
        // WO-0076 scope_in §11 lesson + WO-0077 §planner lesson #1.
        // M2 mutation target — see WO-0082 RFR §Mutation contract.
        assert!(
            health.tools.iter().any(|t| t == "run_tests"),
            "(SA2) expected `run_tests` tool in advertised set; got: {:?}; want: \
             \"run_tests\"",
            health.tools,
        );
    }
}
