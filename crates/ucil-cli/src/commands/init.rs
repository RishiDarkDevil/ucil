//! `ucil init` — detect project languages, create `.ucil/`, write `ucil.toml`
//! and `init_report.json`, and optionally probe P0 plugin binaries.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use clap::Args;
use serde::Serialize;

use ucil_core::schema_migration::SCHEMA_VERSION;

// ── LLM provider ──────────────────────────────────────────────────────────────

/// Supported LLM providers for `ucil init --llm-provider`.
///
/// When absent the provider defaults to `none` (no provider configured).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum LlmProvider {
    Ollama,
    Claude,
    Openai,
    Passthrough,
    None,
}

impl std::fmt::Display for LlmProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ollama => write!(f, "ollama"),
            Self::Claude => write!(f, "claude"),
            Self::Openai => write!(f, "openai"),
            Self::Passthrough => write!(f, "passthrough"),
            Self::None => write!(f, "none"),
        }
    }
}

// ── Arguments ─────────────────────────────────────────────────────────────────

/// Arguments for `ucil init`.
#[derive(Args, Debug)]
pub struct InitArgs {
    /// Directory to initialise (defaults to current directory).
    #[arg(long, default_value = ".")]
    pub dir: PathBuf,

    /// LLM provider to configure in the `[llm]` section of `ucil.toml`.
    ///
    /// Writes `provider = "<value>"`.  If absent, writes `provider = "none"`.
    #[arg(long)]
    pub llm_provider: Option<LlmProvider>,

    /// Skip P0 plugin binary health verification.
    ///
    /// When set, all plugin statuses are recorded as `"skipped"` in
    /// `init_report.json`.  Use in CI / gate smoke tests where host tools are
    /// not installed.
    #[arg(long)]
    pub no_install_plugins: bool,
}

// ── Language detection ────────────────────────────────────────────────────────

/// Languages that UCIL can detect and index.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Rust,
    TypeScript,
    Python,
    Go,
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rust => write!(f, "rust"),
            Self::TypeScript => write!(f, "typescript"),
            Self::Python => write!(f, "python"),
            Self::Go => write!(f, "go"),
        }
    }
}

/// Scans `dir` for language marker files and returns detected languages.
///
/// Detection rules:
/// - Rust: `Cargo.toml` present OR any `*.rs` file
/// - TypeScript: `package.json` present OR any `*.ts` / `*.tsx` file
/// - Python: `pyproject.toml` present OR any `*.py` file
/// - Go: `go.mod` present OR any `*.go` file
#[must_use]
pub fn detect_languages(dir: &Path) -> Vec<Language> {
    let mut langs = std::collections::BTreeSet::new();
    walk_for_langs(dir, 2, &mut langs);
    langs.into_iter().collect()
}

fn walk_for_langs(dir: &Path, depth: u32, langs: &mut std::collections::BTreeSet<Language>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();

        if name.starts_with('.') || name == "node_modules" || name == "target" || name == ".git" {
            continue;
        }

        if path.is_file() {
            classify_file(&name, langs);
        } else if path.is_dir() && depth > 0 {
            walk_for_langs(&path, depth - 1, langs);
        }
    }
}

fn ext_eq(name: &str, ext: &str) -> bool {
    std::path::Path::new(name)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(ext))
}

fn classify_file(name: &str, langs: &mut std::collections::BTreeSet<Language>) {
    if name == "Cargo.toml" || ext_eq(name, "rs") {
        langs.insert(Language::Rust);
    }
    if name == "package.json" || ext_eq(name, "ts") || ext_eq(name, "tsx") {
        langs.insert(Language::TypeScript);
    }
    if name == "pyproject.toml" || ext_eq(name, "py") {
        langs.insert(Language::Python);
    }
    if name == "go.mod" || ext_eq(name, "go") {
        langs.insert(Language::Go);
    }
}

// ── Plugin health verification ────────────────────────────────────────────────

/// Outcome of probing a single plugin binary.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginStatusKind {
    /// Binary responded to `--version` with exit 0.
    Ok,
    /// Binary not found or exited non-zero — init continues regardless.
    Degraded,
    /// Probe was skipped (`--no-install-plugins` was passed).
    Skipped,
}

/// Health probe result for one plugin binary.
#[derive(Debug, Clone, Serialize)]
pub struct PluginStatus {
    /// Binary name as passed to `Command::new`.
    pub name: String,
    /// Result of the probe.
    pub status: PluginStatusKind,
}

/// P0-priority binaries to probe during `ucil init`.
pub const P0_PLUGINS: &[&str] = &[
    "serena",
    "rust-analyzer",
    "pyright",
    "ruff",
    "eslint",
    "shellcheck",
];

/// Maximum time to wait for a single plugin binary to respond to `--version`.
const PLUGIN_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Probes each P0 plugin binary by running `<bin> --version`.
///
/// Returns `PluginStatusKind::Ok` when the binary exits 0, and
/// `PluginStatusKind::Degraded` when the binary is not found, fails, or times
/// out after [`PLUGIN_PROBE_TIMEOUT`].  Never returns an error — missing or
/// unresponsive tools are graceful degradation.
pub async fn verify_plugin_health() -> Vec<PluginStatus> {
    let mut statuses = Vec::with_capacity(P0_PLUGINS.len());
    for &bin in P0_PLUGINS {
        let output_result = tokio::time::timeout(
            PLUGIN_PROBE_TIMEOUT,
            tokio::process::Command::new(bin).arg("--version").output(),
        )
        .await;
        let kind = match output_result {
            Ok(Ok(out)) if out.status.success() => PluginStatusKind::Ok,
            _ => PluginStatusKind::Degraded, // timeout, binary not found, or non-zero exit
        };
        statuses.push(PluginStatus {
            name: bin.to_owned(),
            status: kind,
        });
    }
    statuses
}

/// Returns a list of `Skipped` statuses without probing any binary.
fn skipped_plugin_health() -> Vec<PluginStatus> {
    P0_PLUGINS
        .iter()
        .map(|&name| PluginStatus {
            name: name.to_owned(),
            status: PluginStatusKind::Skipped,
        })
        .collect()
}

// ── Init report ───────────────────────────────────────────────────────────────

/// Serialised to `.ucil/init_report.json` at the end of every `ucil init` run.
#[derive(Debug, Serialize)]
pub struct InitReport {
    /// Schema version at the time of init.
    pub schema_version: String,
    /// Basename of the initialised project directory.
    pub project_name: String,
    /// Detected languages (lowercase strings).
    pub languages: Vec<String>,
    /// Plugin binary probe results.
    pub plugin_health: Vec<PluginStatus>,
    /// Configured LLM provider string (e.g. `"ollama"`, `"none"`).
    pub llm_provider: String,
}

// ── ucil.toml serialisation ───────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct UcilConfig {
    project: ProjectSection,
    llm: LlmSection,
}

#[derive(Debug, Serialize)]
struct ProjectSection {
    name: String,
    languages: Vec<String>,
    schema_version: String,
}

#[derive(Debug, Serialize)]
struct LlmSection {
    provider: String,
}

// ── Command runner ────────────────────────────────────────────────────────────

/// Run `ucil init`.
///
/// # Errors
///
/// Returns an error if the `.ucil/` directory, `ucil.toml`, or
/// `init_report.json` cannot be written.
pub async fn run(args: InitArgs) -> Result<()> {
    let dir = args.dir.canonicalize().unwrap_or(args.dir);

    let name = dir.file_name().map_or_else(
        || "project".to_owned(),
        |n| n.to_string_lossy().into_owned(),
    );

    let languages = detect_languages(&dir);
    let lang_strings: Vec<String> = languages.iter().map(ToString::to_string).collect();

    let provider = args
        .llm_provider
        .as_ref()
        .map_or_else(|| "none".to_owned(), LlmProvider::to_string);

    // Create .ucil/ (idempotent).
    let ucil_dir = dir.join(".ucil");
    if !ucil_dir.exists() {
        fs::create_dir_all(&ucil_dir)
            .with_context(|| format!("failed to create {}", ucil_dir.display()))?;
    }

    // Write ucil.toml with [project] and [llm] sections.
    let config = UcilConfig {
        project: ProjectSection {
            name: name.clone(),
            languages: lang_strings.clone(),
            schema_version: SCHEMA_VERSION.to_owned(),
        },
        llm: LlmSection {
            provider: provider.clone(),
        },
    };
    let toml_content = toml::to_string_pretty(&config).context("failed to serialise ucil.toml")?;
    let toml_path = ucil_dir.join("ucil.toml");
    fs::write(&toml_path, &toml_content)
        .with_context(|| format!("failed to write {}", toml_path.display()))?;

    // Probe plugin binaries (or skip).
    let plugin_health = if args.no_install_plugins {
        skipped_plugin_health()
    } else {
        verify_plugin_health().await
    };

    // Write init_report.json.
    let report = InitReport {
        schema_version: SCHEMA_VERSION.to_owned(),
        project_name: name.clone(),
        languages: lang_strings.clone(),
        plugin_health,
        llm_provider: provider,
    };
    let report_json =
        serde_json::to_string_pretty(&report).context("failed to serialise init_report.json")?;
    let report_path = ucil_dir.join("init_report.json");
    fs::write(&report_path, &report_json)
        .with_context(|| format!("failed to write {}", report_path.display()))?;

    let langs_display = if lang_strings.is_empty() {
        "none detected".to_owned()
    } else {
        lang_strings.join(", ")
    };
    println!("ucil init: created .ucil/ for {name} [{langs_display}]");

    Ok(())
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::{
        detect_languages, run, skipped_plugin_health, verify_plugin_health, InitArgs, Language,
        LlmProvider, PluginStatusKind, P0_PLUGINS,
    };

    fn tmp() -> TempDir {
        TempDir::new().expect("temp dir")
    }

    // ── Language detection ────────────────────────────────────────────────────

    #[test]
    fn detects_rust_from_cargo_toml() {
        let dir = tmp();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
        let langs = detect_languages(dir.path());
        assert!(langs.contains(&Language::Rust));
    }

    #[test]
    fn detects_python_from_pyproject() {
        let dir = tmp();
        std::fs::write(dir.path().join("pyproject.toml"), "[project]").unwrap();
        let langs = detect_languages(dir.path());
        assert!(langs.contains(&Language::Python));
    }

    #[test]
    fn detects_typescript_from_package_json() {
        let dir = tmp();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        let langs = detect_languages(dir.path());
        assert!(langs.contains(&Language::TypeScript));
    }

    #[test]
    fn detects_go_from_go_mod() {
        let dir = tmp();
        std::fs::write(dir.path().join("go.mod"), "module example.com/m\ngo 1.21\n").unwrap();
        let langs = detect_languages(dir.path());
        assert!(langs.contains(&Language::Go));
    }

    #[test]
    fn empty_dir_detects_nothing() {
        let dir = tmp();
        let langs = detect_languages(dir.path());
        assert!(langs.is_empty());
    }

    // ── F04 — LLM provider selection ─────────────────────────────────────────

    /// `--llm-provider ollama` writes `provider = "ollama"` to ucil.toml.
    /// Absent provider defaults to `"none"`.
    #[tokio::test]
    async fn test_llm_provider_selection() {
        let dir = tmp();

        // With explicit provider.
        let args = InitArgs {
            dir: dir.path().to_path_buf(),
            llm_provider: Some(LlmProvider::Ollama),
            no_install_plugins: true,
        };
        run(args).await.expect("init should succeed");

        let toml_str =
            std::fs::read_to_string(dir.path().join(".ucil/ucil.toml")).expect("ucil.toml");
        assert!(
            toml_str.contains("provider = \"ollama\""),
            "ucil.toml must contain 'provider = \"ollama\"'; got:\n{toml_str}"
        );

        // Re-init with no provider — should default to "none".
        let dir2 = tmp();
        let args2 = InitArgs {
            dir: dir2.path().to_path_buf(),
            llm_provider: None,
            no_install_plugins: true,
        };
        run(args2).await.expect("init (no provider) should succeed");

        let toml_str2 =
            std::fs::read_to_string(dir2.path().join(".ucil/ucil.toml")).expect("ucil.toml");
        assert!(
            toml_str2.contains("provider = \"none\""),
            "ucil.toml must default to provider = \"none\"; got:\n{toml_str2}"
        );
    }

    // ── F05 — Plugin health verification ─────────────────────────────────────

    /// `verify_plugin_health` returns one entry per P0 plugin and never panics
    /// even when all binaries are absent.  `skipped_plugin_health` records all
    /// as `Skipped`.
    #[tokio::test]
    async fn test_plugin_health_verification() {
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
                "status for '{}' must be Ok or Degraded, never Skipped from verify_plugin_health",
                s.name
            );
        }

        let skipped = skipped_plugin_health();
        assert_eq!(skipped.len(), P0_PLUGINS.len());
        for s in &skipped {
            assert!(
                matches!(s.status, PluginStatusKind::Skipped),
                "'{}' should be Skipped when using skipped_plugin_health()",
                s.name
            );
        }
    }

    // ── F06 — init_report.json ────────────────────────────────────────────────

    /// `run` with `--no-install-plugins` produces a valid `.ucil/init_report.json`
    /// whose fields match the init arguments.
    #[tokio::test]
    async fn test_init_report_json() {
        let dir = tmp();
        // Place a Cargo.toml so language detection has something to find.
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname=\"test\"\n").unwrap();

        let args = InitArgs {
            dir: dir.path().to_path_buf(),
            llm_provider: Some(LlmProvider::Claude),
            no_install_plugins: true,
        };
        run(args).await.expect("init should succeed");

        let report_path = dir.path().join(".ucil/init_report.json");
        assert!(report_path.exists(), "init_report.json must be created");

        let content = std::fs::read_to_string(&report_path).expect("read init_report.json");
        let report: serde_json::Value =
            serde_json::from_str(&content).expect("init_report.json must be valid JSON");

        assert_eq!(
            report["llm_provider"], "claude",
            "llm_provider field mismatch"
        );
        assert_eq!(
            report["schema_version"], "1.0.0",
            "schema_version field mismatch"
        );
        assert!(report["languages"].is_array(), "languages must be an array");
        assert!(
            report["plugin_health"].is_array(),
            "plugin_health must be an array"
        );

        // All statuses must be "skipped" because --no-install-plugins was set.
        for entry in report["plugin_health"].as_array().expect("array") {
            assert_eq!(
                entry["status"], "skipped",
                "all plugin statuses should be 'skipped' with --no-install-plugins"
            );
        }

        // Rust detected because Cargo.toml is present.
        let langs = report["languages"].as_array().expect("array");
        assert!(
            langs.iter().any(|l| l == "rust"),
            "rust should be detected from Cargo.toml"
        );
    }
}
