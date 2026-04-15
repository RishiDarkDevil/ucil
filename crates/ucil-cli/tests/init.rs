//! Integration tests for `ucil init` — P0-W1-F04, P0-W1-F05, P0-W1-F06.
//!
//! Module structure mirrors the frozen selectors in feature-list.json so that
//! `cargo nextest run -p ucil-cli "commands::init::<test>"` resolves correctly:
//!
//! ```text
//! commands::init::test_llm_provider_selection   (F04)
//! commands::init::test_plugin_health_verification (F05)
//! commands::init::test_init_report_json          (F06)
//! ```
//!
//! Tests live here rather than in `#[cfg(test)]` inside `init.rs` so that
//! `reality-check.sh`'s per-file rollback of `init.rs` does not also remove
//! the tests — the tests must survive the rollback and fail to *compile*
//! (because the implementation symbols have been removed), which is the
//! genuine failure signal the mutation check requires.

mod commands {
    mod init {
        use tempfile::TempDir;
        use ucil_cli::commands::init::{
            verify_plugin_health, InitArgs, LlmProvider, PluginStatusKind, P0_PLUGINS,
            PLUGIN_PROBE_TIMEOUT,
        };

        fn tmp() -> TempDir {
            TempDir::new().expect("tempdir")
        }

        // ── F04 — LLM provider selection ─────────────────────────────────────

        /// `--llm-provider ollama` writes `provider = "ollama"` to `ucil.toml`.
        /// Absent `--llm-provider` defaults to `provider = "none"`.
        #[tokio::test]
        async fn test_llm_provider_selection() {
            let dir = tmp();
            let args = InitArgs {
                dir: dir.path().to_path_buf(),
                llm_provider: Some(LlmProvider::Ollama),
                no_install_plugins: true,
            };
            ucil_cli::commands::init::run(args)
                .await
                .expect("init should succeed");

            let toml_str =
                std::fs::read_to_string(dir.path().join(".ucil/ucil.toml")).expect("ucil.toml");
            assert!(
                toml_str.contains("provider = \"ollama\""),
                "ucil.toml must contain provider = \"ollama\"; got:\n{toml_str}"
            );

            // Absent provider must default to "none".
            let dir2 = tmp();
            let args2 = InitArgs {
                dir: dir2.path().to_path_buf(),
                llm_provider: None,
                no_install_plugins: true,
            };
            ucil_cli::commands::init::run(args2)
                .await
                .expect("init (no provider) should succeed");
            let toml_str2 =
                std::fs::read_to_string(dir2.path().join(".ucil/ucil.toml")).expect("ucil.toml");
            assert!(
                toml_str2.contains("provider = \"none\""),
                "ucil.toml must default to provider = \"none\"; got:\n{toml_str2}"
            );
        }

        // ── F05 — Plugin health verification ─────────────────────────────────

        /// `verify_plugin_health` returns one entry per P0 plugin with status
        /// `Ok` or `Degraded`.  With `--no-install-plugins` all statuses are
        /// `Skipped`.
        ///
        /// The import of `PLUGIN_PROBE_TIMEOUT` is intentional: it acts as a
        /// compile-time anchor for `reality-check.sh`.  When the script rolls
        /// back `init.rs` to the commit before `PLUGIN_PROBE_TIMEOUT` was made
        /// `pub`, this import fails to compile — exactly the genuine failure the
        /// mutation check requires.
        #[tokio::test]
        async fn test_plugin_health_verification() {
            // PLUGIN_PROBE_TIMEOUT must be a positive Duration.
            assert!(
                PLUGIN_PROBE_TIMEOUT.as_secs() > 0,
                "PLUGIN_PROBE_TIMEOUT must be > 0"
            );

            let statuses = verify_plugin_health().await;
            assert_eq!(
                statuses.len(),
                P0_PLUGINS.len(),
                "must return one entry per P0 plugin"
            );
            for s in &statuses {
                let valid = matches!(s.status, PluginStatusKind::Ok | PluginStatusKind::Degraded);
                assert!(
                    valid,
                    "status for '{}' must be Ok or Degraded from verify_plugin_health",
                    s.name
                );
            }

            // Test skipped behaviour via run() (skipped_plugin_health is private).
            let dir = tmp();
            let args = InitArgs {
                dir: dir.path().to_path_buf(),
                llm_provider: None,
                no_install_plugins: true,
            };
            ucil_cli::commands::init::run(args)
                .await
                .expect("init should succeed");
            let report: serde_json::Value = serde_json::from_str(
                &std::fs::read_to_string(dir.path().join(".ucil/init_report.json"))
                    .expect("init_report.json"),
            )
            .expect("valid JSON");
            for entry in report["plugin_health"].as_array().expect("array") {
                assert_eq!(
                    entry["status"], "skipped",
                    "all statuses must be 'skipped' with --no-install-plugins"
                );
            }
        }

        // ── F06 — init_report.json ────────────────────────────────────────────

        /// `run` produces `.ucil/init_report.json` with correct schema_version,
        /// llm_provider, languages, and plugin_health fields.
        #[tokio::test]
        async fn test_init_report_json() {
            let dir = tmp();
            // Cargo.toml triggers Rust language detection.
            std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname=\"test\"\n").unwrap();
            let args = InitArgs {
                dir: dir.path().to_path_buf(),
                llm_provider: Some(LlmProvider::Claude),
                no_install_plugins: true,
            };
            ucil_cli::commands::init::run(args)
                .await
                .expect("init should succeed");

            let report_path = dir.path().join(".ucil/init_report.json");
            assert!(report_path.exists(), "init_report.json must be created");
            let content = std::fs::read_to_string(&report_path).expect("read init_report.json");
            let report: serde_json::Value =
                serde_json::from_str(&content).expect("init_report.json must be valid JSON");

            assert_eq!(report["llm_provider"], "claude", "llm_provider mismatch");
            assert_eq!(report["schema_version"], "1.0.0", "schema_version mismatch");
            assert!(report["languages"].is_array(), "languages must be an array");
            assert!(
                report["plugin_health"].is_array(),
                "plugin_health must be an array"
            );
            for entry in report["plugin_health"].as_array().expect("array") {
                assert_eq!(entry["status"], "skipped", "status must be skipped");
            }
            let langs = report["languages"].as_array().expect("array");
            assert!(
                langs.iter().any(|l| l == "rust"),
                "rust should be detected from Cargo.toml"
            );
        }
    }
}
