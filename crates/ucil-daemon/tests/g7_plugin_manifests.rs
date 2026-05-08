//! End-to-end integration tests for the on-disk ESLint + Semgrep G7
//! (Quality) plugin manifests (P3-W11-F02 / P3-W11-F04).
//!
//! Each test loads the on-disk manifest at
//! `plugins/quality/<name>/plugin.toml`, drives the manifest's
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
//! end against the real `npx -y <pinned-pkg>` (ESLint) or
//! `uvx <pinned-pkg>` (Semgrep) invocation.
//!
//! Set `UCIL_SKIP_EXTERNAL_PLUGIN_TESTS=1` only on truly offline CI
//! builds (skips ALL external plugin-manifest suites including this
//! G7 suite); set the G7-specific `UCIL_SKIP_QUALITY_PLUGIN_E2E=1`
//! to skip ONLY this suite (so an operator can keep the WO-0044 /
//! WO-0069 / WO-0072 / WO-0074 / WO-0075 regression coverage without
//! paying the additional ~30-second-cold-cache npx + uvx fetches for
//! the G7 plugins). The verifier MUST NOT set EITHER opt-out, per
//! WO-0076 `scope_in` #12 carried by this WO.
//!
//! Semgrep CLI dependency (per WO-0076 plugin.toml top-of-file
//! rustdoc): `semgrep-mcp@0.8.1` requires the Semgrep CLI binary on
//! PATH (or located via the `SEMGREP_PATH` env var) — the upstream
//! lifespan handler `semgrep_mcp.semgrep.mk_context` calls
//! `semgrep --pro --version` BEFORE the MCP server reaches its
//! initialize handshake. Operator-state per the WO-0069 §planner
//! Mem0 API-key short-circuit precedent (applied here to a CLI
//! binary instead of an env var). When `semgrep` is missing the
//! Semgrep test short-circuits with an informational `[SKIP]` log —
//! NOT a failure — mirroring the API-key gating approach for
//! environments without operator-state. The ESLint test has no
//! analogous external dep — `npx -y @eslint/mcp@0.3.5` runs entirely
//! self-contained.
//!
//! Tests are wrapped in `mod g7_plugin_manifests` so nextest reports
//! them as `g7_plugin_manifests::eslint_manifest_health_check` and
//! `g7_plugin_manifests::semgrep_manifest_health_check` matching the
//! WO-0076 acceptance selectors. Same wrapper pattern as the existing
//! `mod g6_plugin_manifests` block in
//! `tests/g6_plugin_manifests.rs:67` (DEC-0007 frozen-selector
//! module-root placement; carried per WO-0068 lessons §"For planner"
//! frozen-test selector substring-match REQUIRES module-root
//! placement). NO `mod tests { ... }` nesting; the test functions
//! live at `mod g7_plugin_manifests` ROOT per WO-0073 lessons §"For
//! planner".
//!
//! This file is a peer of `tests/g3_plugin_manifests.rs` (G3 suite),
//! `tests/g4_plugin_manifests.rs` (G4 suite),
//! `tests/g5_plugin_manifests.rs` (G5 suite),
//! `tests/g6_plugin_manifests.rs` (G6 suite), and
//! `tests/plugin_manifests.rs` (WO-0044 G2 regression guard) — six
//! group-isolated suites kept distinct so each group's
//! `UCIL_SKIP_<GROUP>_PLUGIN_E2E` opt-out is scoped distinctly. Per
//! WO-0069 lessons §executor #2 ("write a SEPARATE integration test
//! file per phase/group" — single-file-per-group keeps the
//! architecture / knowledge / search / context / platform / quality
//! test surfaces isolated and avoids cross-group flake propagation).
//!
//! All fixture-init / tmpdir-mutation helpers in async test bodies
//! use the tokio variant of the process-spawn API (NOT the blocking
//! standard-library variant) per WO-0075 lesson §executor W1 — pre-
//! emptively applied here to avoid the WO-0075 W1 critic warning.
//! Rule reference: `.claude/rules/rust-style.md` §Async line 23
//! mandates the tokio process variant in async paths.

mod g7_plugin_manifests {
    use std::path::{Path, PathBuf};

    use tokio::process::Command;
    use ucil_daemon::{HealthStatus, PluginManager, PluginManifest};

    /// Generous first-run npx + uvx download budget — `npx -y <pkg>`
    /// and `uvx <pkg>` may fetch tarballs + transitive deps on a cold
    /// cache (the ESLint MCP package pulls a small Node deps set;
    /// semgrep-mcp pulls a Python venv plus mcp + uv-resolve transitive
    /// deps and triggers a `semgrep --pro --version` lifespan call).
    /// Subsequent runs hit the cache and complete in well under a
    /// second; the production-default `HEALTH_CHECK_TIMEOUT_MS` (5 s)
    /// is therefore fine for steady-state daemon ticks but inadequate
    /// for the very first post-install integration-test run on a
    /// fresh workstation. Mirror the WO-0044 / WO-0069 / WO-0072 /
    /// WO-0074 / WO-0075 budget exactly.
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
    /// air-gapped CI runners that cannot reach npm + pypi at all (this
    /// is the same global opt-out honoured by the WO-0044
    /// `tests/plugin_manifests.rs`, WO-0069 `tests/g3_plugin_manifests.rs`,
    /// WO-0072 `tests/g4_plugin_manifests.rs`, WO-0074
    /// `tests/g5_plugin_manifests.rs`, and WO-0075
    /// `tests/g6_plugin_manifests.rs` suites) AND the G7-specific
    /// `UCIL_SKIP_QUALITY_PLUGIN_E2E` opt-out for operators that want
    /// to keep the existing G2/G3/G4/G5/G6 regression coverage but
    /// skip the additional ESLint + Semgrep cold-cache budget. Either
    /// env set means "skip this test"; the verifier MUST NOT set
    /// either, per WO-0076 `scope_in` #12.
    fn skip_via_env() -> bool {
        std::env::var("UCIL_SKIP_EXTERNAL_PLUGIN_TESTS").is_ok()
            || std::env::var("UCIL_SKIP_QUALITY_PLUGIN_E2E").is_ok()
    }

    /// Resolves a working Semgrep CLI binary path. Returns the
    /// absolute path to the working binary on success, or `None` if
    /// no working CLI is reachable via `SEMGREP_PATH` or `which`
    /// resolution against the parent's `PATH`. The returned path is
    /// guaranteed to respond to `--version` with exit-code 0 — this
    /// matters because the bundled `semgrep` shipped inside the
    /// `uvx semgrep-mcp@0.8.1` venv has a transitive
    /// `opentelemetry-instrumentation-requests` import-time crash
    /// (the venv-semgrep returns code 1 on `--version`). Mirrors
    /// the upstream `semgrep_mcp.utilities.utils.find_semgrep_info`
    /// resolution order with one addition: the resolved path is
    /// returned to the caller so the caller can export it back as
    /// `SEMGREP_PATH` for the spawned uvx subprocess (whose
    /// PATH-prepended venv-semgrep would otherwise win the lookup
    /// inside `find_semgrep_info` and crash on import).
    ///
    /// Used by the Semgrep test only — when the CLI is absent,
    /// `uvx semgrep-mcp@0.8.1` exits at lifespan with an `McpError`
    /// before any tools/list reply can arrive. Skipping is the
    /// correct behaviour per the WO-0069 §planner Mem0 API-key
    /// short-circuit precedent (applied here to a CLI binary instead
    /// of an env var).
    async fn resolve_working_semgrep() -> Option<String> {
        // Honour an operator-set SEMGREP_PATH first.
        if let Ok(path) = std::env::var("SEMGREP_PATH") {
            if !path.is_empty() && version_check(&path).await {
                return Some(path);
            }
        }
        // Fall back to `which semgrep` against the parent's PATH.
        // `which` is POSIX-required so this is portable; tokio::process
        // is the async-correct form per .claude/rules/rust-style.md
        // §Async line 23 + WO-0075 lesson §executor W1.
        if let Ok(out) = Command::new("which").arg("semgrep").output().await {
            if out.status.success() {
                let resolved = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !resolved.is_empty() && version_check(&resolved).await {
                    return Some(resolved);
                }
            }
        }
        None
    }

    /// Runs `<path> --version` via the tokio process variant and
    /// returns `true` if the process exits with status 0. Used by
    /// `resolve_working_semgrep` to filter out broken binaries (the
    /// uvx-venv-bundled semgrep that crashes on import).
    async fn version_check(path: &str) -> bool {
        match Command::new(path).arg("--version").output().await {
            Ok(out) => out.status.success(),
            Err(_) => false,
        }
    }

    /// Copies the read-only `tests/fixtures/<fixture>` directory into
    /// a freshly-created tmpdir, returning the absolute path to the
    /// copy. Per WO-0074 §executor #5 / WO-0075 §executor #5: the
    /// fixture must be copied into a tmpdir BEFORE invoking the
    /// upstream binary so any side-files the MCP server writes do
    /// NOT pollute the read-only fixture tree (`forbidden_paths` in
    /// WO-0076 — `tests/fixtures/**` is immutable).
    ///
    /// Uses `tokio::process::Command::new("cp").arg("-r")` instead of
    /// a hand-rolled async recursive copy because (a) the host already
    /// guarantees `cp` is available (POSIX requirement), and (b) per
    /// WO-0075 lesson §executor W1 / .claude/rules/rust-style.md
    /// §Async line 23 the async paths must use the tokio process
    /// variant rather than the blocking standard-library one.
    async fn copy_fixture_to_tmpdir(prefix: &str, fixture: &str) -> PathBuf {
        let tmp = tempfile::Builder::new()
            .prefix(prefix)
            .tempdir()
            .expect("create tmpdir for fixture copy");
        let dst = tmp.path().join("fixture-copy");
        let src = repo_root().join(fixture);
        let out = Command::new("cp")
            .arg("-r")
            .arg(&src)
            .arg(&dst)
            .output()
            .await
            .expect("invoke cp -r");
        assert!(
            out.status.success(),
            "cp -r {src:?} {dst:?} failed: stderr={}",
            String::from_utf8_lossy(&out.stderr),
        );
        let path = dst.clone();
        // Leak the TempDir so the directory persists for the duration
        // of the test (the spawned MCP server keeps a handle on it).
        // tokio runtime tear-down at test-end reclaims the tmpfs space
        // when the OS unlinks the orphaned dir.
        std::mem::forget(tmp);
        path
    }

    /// Asserts the directory at `path` exists. Sanity check used after
    /// a tmpdir copy to give a readable failure message if the cp
    /// pipeline somehow elided the destination.
    fn assert_dir_exists(path: &Path) {
        assert!(
            path.is_dir(),
            "(SA0) tmpdir copy at {path:?} is not a directory; \
             tmpdir-population failed silently",
        );
    }

    #[tokio::test]
    async fn eslint_manifest_health_check() {
        if skip_via_env() {
            return;
        }
        let manifest_path = repo_root().join("plugins/quality/eslint/plugin.toml");
        let manifest = PluginManifest::from_path(&manifest_path).expect("parse eslint plugin.toml");

        // Manifest sanity (cheap pre-flight before paying the npx cost).
        assert_eq!(manifest.plugin.name, "eslint");
        assert_eq!(manifest.transport.kind, "stdio");
        assert!(
            !manifest.capabilities.provides.is_empty(),
            "eslint manifest must declare at least one provided capability",
        );
        assert!(
            manifest
                .capabilities
                .provides
                .iter()
                .all(|c| c.starts_with("quality.")),
            "eslint manifest must declare its capabilities under the quality.* namespace, got: {:?}",
            manifest.capabilities.provides,
        );

        // Copy the typescript-project fixture into a tmpdir BEFORE
        // invoking the upstream binary (per WO-0074 §executor #5 +
        // WO-0076 scope_in #5/#16). The `lint-files` tool accepts
        // absolute file paths via the `filePaths` argument so the
        // spawned binary's cwd does not affect the cargo-test
        // tools/list path — the tmpdir copy is here for parity with
        // the verify script's tool-level smoke + to keep the fixture
        // tree pristine when later helpers add config files.
        let project_dir =
            copy_fixture_to_tmpdir("ucil-wo-0076-eslint-", "tests/fixtures/typescript-project")
                .await;
        assert_dir_exists(&project_dir);

        let health = PluginManager::health_check_with_timeout(&manifest, FIRST_RUN_TIMEOUT_MS)
            .await
            .expect("health-check eslint MCP server");

        assert_eq!(health.name, "eslint");
        assert_eq!(
            health.status,
            HealthStatus::Ok,
            "eslint health-check returned non-Ok status: {:?}",
            health.status,
        );
        assert!(!health.tools.is_empty(), "eslint advertised zero tools");
        // `lint-files` is the canonical lint tool advertised by
        // `@eslint/mcp@0.3.5` (single tool — full live list captured
        // in the manifest's top-of-file rustdoc comment). The
        // master-plan §4.7 line 374 vocabulary
        //   ESLint MCP — JS/TS lint
        // describes the capability category independent of the
        // upstream literal tool name. Pinning on `lint-files` (the
        // upstream literal that matches our declared
        // `quality.eslint.lint` capability verbatim) gives the
        // strongest detection signal for upstream renames mirroring
        // the WO-0072 / WO-0074 / WO-0075 rationale. Note:
        // kebab-case `lint-files` is the upstream literal as emitted
        // by `tools/list` — preferred over snake-case translation
        // per WO-0074 scope_in #1 lesson.
        assert!(
            health.tools.iter().any(|t| t == "lint-files"),
            "(SA1) expected `lint-files` tool in advertised set; got: {:?}",
            health.tools,
        );
    }

    #[tokio::test]
    async fn semgrep_manifest_health_check() {
        if skip_via_env() {
            return;
        }
        // Semgrep CLI binary is required by the upstream lifespan
        // handler — see file-level rustdoc. Skip gracefully when it
        // is absent (WO-0069 §planner Mem0 API-key short-circuit
        // precedent applied to a CLI binary).
        let Some(semgrep_path) = resolve_working_semgrep().await else {
            eprintln!(
                "[SKIP] g7_plugin_manifests::semgrep_manifest_health_check: \
                 no working semgrep CLI found via SEMGREP_PATH or PATH. \
                 Install semgrep via `pip install --user semgrep` or \
                 `uv tool install semgrep` and re-run. Note: the uvx-bundled \
                 semgrep inside semgrep-mcp's venv has a known \
                 opentelemetry-instrumentation-requests import-time crash \
                 and is filtered out by the `--version` exit-code check."
            );
            return;
        };
        // Export SEMGREP_PATH so the spawned `uvx semgrep-mcp@0.8.1`
        // subprocess inherits it. The upstream's `find_semgrep_info`
        // probes SEMGREP_PATH AFTER the bare `semgrep` PATH lookup —
        // but the uvx venv prepends a broken bundled-semgrep onto
        // PATH, so without the explicit SEMGREP_PATH override the
        // upstream finds the broken binary first and lifespan crashes
        // before tools/list can arrive. tokio::process inherits env
        // by default (no .env_clear() in PluginManager::spawn), so
        // setting it on the parent propagates verbatim. Safe even if
        // an outer SEMGREP_PATH was already set: this is the same
        // value the resolve_working_semgrep helper just verified.
        // SAFETY: cargo test runs each #[tokio::test] in its own
        // tokio runtime but the Rust process is single-threaded
        // w.r.t. environment manipulation per `--test-threads=1`
        // (WO-0076 verifier protocol — single-threaded tests). The
        // env mutation is process-global; the eslint test does not
        // read SEMGREP_PATH so cross-test contamination is harmless.
        // SAFETY: see set_var docs — single-threaded test.
        unsafe {
            std::env::set_var("SEMGREP_PATH", &semgrep_path);
        }

        let manifest_path = repo_root().join("plugins/quality/semgrep/plugin.toml");
        let manifest =
            PluginManifest::from_path(&manifest_path).expect("parse semgrep plugin.toml");

        // Manifest sanity (cheap pre-flight before paying the uvx cost).
        assert_eq!(manifest.plugin.name, "semgrep");
        assert_eq!(manifest.transport.kind, "stdio");
        assert!(
            !manifest.capabilities.provides.is_empty(),
            "semgrep manifest must declare at least one provided capability",
        );
        assert!(
            manifest
                .capabilities
                .provides
                .iter()
                .all(|c| c.starts_with("quality.")),
            "semgrep manifest must declare its capabilities under the quality.* namespace, \
             got: {:?}",
            manifest.capabilities.provides,
        );

        // Copy the mixed-project fixture into a tmpdir BEFORE
        // invoking the upstream binary (per WO-0074 §executor #5 +
        // WO-0076 scope_in #5). The `semgrep_scan` tool consumes
        // inline `code_files` content rather than scanning the
        // spawned binary's cwd — the tmpdir copy is here for parity
        // with the verify script's tool-level smoke + to keep the
        // fixture tree pristine when later helpers add scratch
        // outputs.
        let project_dir =
            copy_fixture_to_tmpdir("ucil-wo-0076-semgrep-", "tests/fixtures/mixed-project").await;
        assert_dir_exists(&project_dir);

        let health = PluginManager::health_check_with_timeout(&manifest, FIRST_RUN_TIMEOUT_MS)
            .await
            .expect("health-check semgrep MCP server");

        assert_eq!(health.name, "semgrep");
        assert_eq!(
            health.status,
            HealthStatus::Ok,
            "semgrep health-check returned non-Ok status: {:?}",
            health.status,
        );
        assert!(!health.tools.is_empty(), "semgrep advertised zero tools");
        // `semgrep_scan` is the canonical scan tool advertised by
        // `semgrep-mcp@0.8.1` (alongside 7 other tools — full live
        // list captured in the manifest's top-of-file rustdoc
        // comment: semgrep_rule_schema, get_supported_languages,
        // semgrep_findings, semgrep_scan_with_custom_rule,
        // semgrep_scan, semgrep_scan_local, security_check,
        // get_abstract_syntax_tree). The master-plan §4.7 line 377
        // vocabulary
        //   Semgrep MCP — multi-language SAST
        // describes the capability category independent of the
        // upstream literal tool name. Pinning on `semgrep_scan`
        // (the upstream literal that matches our declared
        // `quality.semgrep.scan` capability verbatim) gives the
        // strongest detection signal for upstream renames mirroring
        // the WO-0072 / WO-0074 / WO-0075 rationale. Note:
        // snake_case `semgrep_scan` is the upstream literal as
        // emitted by `tools/list` — preferred over kebab-case
        // translation per WO-0074 scope_in #1 lesson.
        //
        // Disclosed Deviation lineage: this is v0.8.1 (NOT WO-0076
        // scope_in #2's `0.9.0`). v0.9.0 ships only a
        // `deprecation_notice` tool upstream. See
        // plugins/quality/semgrep/plugin.toml top-of-file rustdoc
        // for the full pivot rationale. The v0.8.1 8-tool surface
        // satisfies the WO-0076 F04 `≥1 security finding using the
        // OWASP rule set` acceptance criterion via `semgrep_scan`'s
        // `config: "p/owasp-top-ten"` argument shape.
        assert!(
            health.tools.iter().any(|t| t == "semgrep_scan"),
            "(SA2) expected `semgrep_scan` tool in advertised set; got: {:?}",
            health.tools,
        );
    }
}
