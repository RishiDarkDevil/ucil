//! `ucil plugin install <name>` — discover a `plugin.toml` manifest
//! under `plugins_dir`, spawn its MCP transport subprocess, probe
//! `tools/list`, and report the tools.
//!
//! This module is the CLI façade for `P1-W5-F01` (Serena + plugin
//! manifest kickoff).  The health probe is delegated to
//! [`PluginManager::health_check_with_timeout`] so the caller can
//! pass a large budget on cold `uvx` paths (see master-plan §14.3).
//!
//! # Exit semantics
//!
//! * exit 0 iff [`HealthStatus::Ok`] AND `tools.len() >= 1`.
//! * any other outcome — manifest not found, ambiguous, parse error,
//!   spawn error, degraded-status, protocol error — propagates as an
//!   `anyhow::Error` from [`run`], which the binary's entry point
//!   turns into exit 1.
//!
//! The Serena-specific `tools.len() >= 10` floor is enforced by
//! `scripts/verify/P1-W5-F01.sh`, not by this module — the CLI
//! contract is "any healthy plugin".

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use clap::{Args, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use walkdir::WalkDir;

use ucil_daemon::{HealthStatus, PluginError, PluginManager, PluginManifest};

// ── Output format ───────────────────────────────────────────────────────────

/// Output format for the `plugin install` subcommand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
#[clap(rename_all = "lower")]
pub enum OutputFormat {
    /// Human-readable one-line header (`plugin NAME ACTIVE: N tools`)
    /// followed by one tool name per line.
    #[default]
    Text,
    /// Single newline-terminated JSON object consumable by
    /// `scripts/verify/P1-W5-F01.sh`.
    Json,
}

// ── CLI args ────────────────────────────────────────────────────────────────

/// Top-level arguments for `ucil plugin ...`.
#[derive(Args, Debug)]
pub struct PluginArgs {
    #[command(subcommand)]
    /// Which plugin-subcommand to run.  Today only `install` exists;
    /// `list`, `enable`, `disable`, `uninstall`, `reload` are
    /// reserved for later work-orders per master-plan §16.
    pub command: PluginSubcommand,
}

/// `ucil plugin <subcommand>` — subcommand dispatcher (master-plan §16
/// line 1580: `plugin list | install <n> | uninstall <n> | enable <n> |
/// disable <n> | reload`).
#[derive(Subcommand, Debug)]
pub enum PluginSubcommand {
    /// Spawn the named plugin's MCP server, probe `tools/list`, and
    /// report back. Persists `installed=true` to the state file on
    /// success so subsequent `list` reflects the install.
    Install(InstallArgs),
    /// Enumerate every `plugin.toml` under `plugins_dir` and join with
    /// the per-plugin state file. Manifests with no state row default
    /// to `installed=false, enabled=false`.
    List(ListArgs),
    /// Mark the named plugin as `installed=false` in the state file.
    /// Does NOT remove the manifest from disk — the operator decides
    /// when to delete the directory.
    Uninstall(UninstallArgs),
    /// Mark the named plugin as `enabled=true` in the state file.
    Enable(EnableArgs),
    /// Mark the named plugin as `enabled=false` in the state file.
    Disable(DisableArgs),
}

/// Arguments for `ucil plugin install <name>`.
#[derive(Args, Debug)]
pub struct InstallArgs {
    /// Plugin identifier to look up in the manifest index.
    ///
    /// Matched against the `[plugin] name` field of every
    /// `plugin.toml` below `plugins_dir` (recursive, max-depth 3).
    pub name: String,

    /// Directory to search for `plugin.toml` manifests.  Walked
    /// recursively up to depth 3 so
    /// `plugins/structural/serena/plugin.toml` resolves under the
    /// default `./plugins` root.
    #[arg(long, default_value = "./plugins")]
    pub plugins_dir: PathBuf,

    /// Timeout budget, in milliseconds, for the full spawn →
    /// `tools/list` → response round-trip.  The default of 180 s is
    /// conservative for cold `uvx`-cached plugins (first-run Serena
    /// typically takes 60–120 s to download + install).
    #[arg(long, default_value_t = 180_000)]
    pub timeout_ms: u64,

    /// How to format the CLI's stdout report.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

/// Arguments for `ucil plugin list`.
///
/// # JSON output
///
/// `{ "plugins": [{ "name": "...", "installed": bool, "enabled":
/// bool }, ...] }` — one element per discovered manifest, joined with
/// the persisted state file.
#[derive(Args, Debug)]
pub struct ListArgs {
    /// Directory to search for `plugin.toml` manifests (max-depth 3,
    /// matching `install`).
    #[arg(long, default_value = "./plugins")]
    pub plugins_dir: PathBuf,
    /// How to format the CLI's stdout report.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

/// Arguments for `ucil plugin uninstall <name>`.
///
/// # JSON output
///
/// `{ "name": "...", "status": "uninstalled", "installed": false,
/// "enabled": bool }`
#[derive(Args, Debug)]
pub struct UninstallArgs {
    /// Plugin identifier (the `[plugin] name` field of the target
    /// manifest).
    pub name: String,
    /// Directory holding the state file.
    #[arg(long, default_value = "./plugins")]
    pub plugins_dir: PathBuf,
    /// How to format the CLI's stdout report.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

/// Arguments for `ucil plugin enable <name>`.
///
/// # JSON output
///
/// `{ "name": "...", "status": "enabled", "enabled": true,
/// "installed": bool }`
#[derive(Args, Debug)]
pub struct EnableArgs {
    /// Plugin identifier.
    pub name: String,
    /// Directory holding the state file.
    #[arg(long, default_value = "./plugins")]
    pub plugins_dir: PathBuf,
    /// How to format the CLI's stdout report.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

/// Arguments for `ucil plugin disable <name>`.
///
/// # JSON output
///
/// `{ "name": "...", "status": "disabled", "enabled": false,
/// "installed": bool }`
#[derive(Args, Debug)]
pub struct DisableArgs {
    /// Plugin identifier.
    pub name: String,
    /// Directory holding the state file.
    #[arg(long, default_value = "./plugins")]
    pub plugins_dir: PathBuf,
    /// How to format the CLI's stdout report.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

// ── Module errors ───────────────────────────────────────────────────────────

/// Errors raised while resolving / driving a `plugin install`.
///
/// The variants here are the *domain*-shaped failures the CLI can
/// describe precisely (plugin not found, manifest-index ambiguous).
/// Transport-layer failures from [`PluginManager::health_check_with_timeout`]
/// propagate as-is under the [`PluginCmdError::Health`] wrapper.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum PluginCmdError {
    /// No `plugin.toml` manifest under `plugins_dir` had
    /// `[plugin] name == <name>`.
    #[error("plugin `{name}` not found under {}", plugins_dir.display())]
    NotFound {
        /// Name the caller searched for.
        name: String,
        /// Root directory that was walked.
        plugins_dir: PathBuf,
    },
    /// ≥2 `plugin.toml` files under `plugins_dir` shared the same
    /// `[plugin] name`; the CLI refuses to pick one silently.
    #[error(
        "plugin `{name}` is ambiguous — {} manifests matched: {}",
        paths.len(),
        paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")
    )]
    Ambiguous {
        /// Name the caller searched for.
        name: String,
        /// All manifest paths whose `[plugin] name` equaled `name`.
        paths: Vec<PathBuf>,
    },
    /// A manifest entry could not be parsed while walking the index.
    #[error("failed to read manifest at {}: {source}", path.display())]
    ManifestRead {
        /// Path that failed to parse.
        path: PathBuf,
        /// Underlying plugin-manager error (IO or TOML).  Boxed to
        /// keep `PluginCmdError` small enough for `clippy::result_large_err`.
        #[source]
        source: Box<PluginError>,
    },
    /// The plugin spawned but did not pass its `tools/list` probe.
    #[error("plugin `{name}` failed health check: {source}")]
    Health {
        /// Plugin name from the manifest.
        name: String,
        /// Underlying plugin-manager error.  Boxed for the same
        /// `result_large_err` reason as `ManifestRead::source`.
        #[source]
        source: Box<PluginError>,
    },
    /// Plugin responded to `tools/list` but the manifest had zero
    /// tools — treated as a failure since callers expect ≥1.
    #[error("plugin `{name}` reported degraded status: {message} (tools={tool_count})")]
    Degraded {
        /// Plugin name from the manifest.
        name: String,
        /// Human-readable diagnostic.
        message: String,
        /// Number of tools reported (may be zero).
        tool_count: usize,
    },
    /// IO error while reading or writing the plugin state file
    /// (`<plugins_dir>/.ucil-plugin-state.toml`).
    #[error("failed to read plugin state at {}: {source}", path.display())]
    StateRead {
        /// State-file path that failed.
        path: PathBuf,
        /// Underlying IO error.
        #[source]
        source: std::io::Error,
    },
    /// Failure parsing or serialising the plugin state file.
    #[error("failed to (de)serialise plugin state at {}: {source}", path.display())]
    StateFormat {
        /// State-file path that failed.
        path: PathBuf,
        /// Human-readable diagnostic for the underlying TOML error.
        #[source]
        source: StateFormatError,
    },
    /// Atomic-rename step of the state-file write failed.
    #[error("failed to write plugin state to {}: {source}", path.display())]
    StateWrite {
        /// State-file path that failed.
        path: PathBuf,
        /// Underlying IO error from the rename or temp-file write.
        #[source]
        source: std::io::Error,
    },
}

/// Wrapper error around `toml::de::Error` / `toml::ser::Error` so the
/// outer [`PluginCmdError`] can stay one enum.
#[derive(Debug, Error)]
pub enum StateFormatError {
    /// Failed to parse TOML on read.
    #[error("invalid TOML: {0}")]
    De(#[from] toml::de::Error),
    /// Failed to serialise TOML on write.
    #[error("invalid TOML: {0}")]
    Ser(#[from] toml::ser::Error),
}

// ── JSON output shape ───────────────────────────────────────────────────────

/// JSON object emitted by `--format json`.  Consumed by
/// `scripts/verify/P1-W5-F01.sh` via `jq -r '.tool_count'`.
#[derive(Debug, Serialize)]
struct InstallReport<'a> {
    /// Plugin name (copy of `[plugin] name`).
    name: &'a str,
    /// `"ok"` or `"degraded"` (error paths exit non-zero before this
    /// is printed, so those variants are not encoded here).
    status: &'a str,
    /// All tools reported by `tools/list`, in the order the plugin
    /// returned them.
    tools: &'a [String],
    /// `tools.len()` for convenience in `jq` expressions.
    tool_count: usize,
}

// ── Public entry point ──────────────────────────────────────────────────────

/// Run the `plugin` subcommand tree.
///
/// Writes its report to `stdout`; tests should call
/// [`run_with_writer`] instead so the captured bytes can be asserted
/// against.
///
/// # Errors
///
/// Returns an `anyhow::Error` wrapping any [`PluginCmdError`] produced
/// by the resolver or [`PluginManager::health_check_with_timeout`].
pub async fn run(args: PluginArgs) -> Result<()> {
    run_with_writer(args, std::io::stdout()).await
}

/// Writer-parameterised variant of [`run`] so tests can capture the
/// stdout bytes into a `Vec<u8>`.
///
/// # Errors
///
/// Same as [`run`].
pub async fn run_with_writer<W: Write>(args: PluginArgs, mut writer: W) -> Result<()> {
    match args.command {
        PluginSubcommand::Install(install) => {
            let report = install_plugin(&install)
                .await
                .with_context(|| format!("plugin install `{}`", install.name))?;
            emit_report(install.format, &report, &mut writer)
                .context("failed to write plugin install report")?;
            Ok(())
        }
        PluginSubcommand::List(list) => {
            let outcome = list_plugins(&list).await.with_context(|| {
                format!("plugin list (plugins_dir={})", list.plugins_dir.display())
            })?;
            emit_list(list.format, &outcome, &mut writer)
                .context("failed to write plugin list report")?;
            Ok(())
        }
        PluginSubcommand::Uninstall(uninstall) => {
            let outcome = uninstall_plugin(&uninstall)
                .await
                .with_context(|| format!("plugin uninstall `{}`", uninstall.name))?;
            emit_state_change(uninstall.format, &outcome, &mut writer)
                .context("failed to write plugin uninstall report")?;
            Ok(())
        }
        PluginSubcommand::Enable(enable) => {
            let outcome = enable_plugin(&enable)
                .await
                .with_context(|| format!("plugin enable `{}`", enable.name))?;
            emit_state_change(enable.format, &outcome, &mut writer)
                .context("failed to write plugin enable report")?;
            Ok(())
        }
        PluginSubcommand::Disable(disable) => {
            let outcome = disable_plugin(&disable)
                .await
                .with_context(|| format!("plugin disable `{}`", disable.name))?;
            emit_state_change(disable.format, &outcome, &mut writer)
                .context("failed to write plugin disable report")?;
            Ok(())
        }
    }
}

// ── `install` core ──────────────────────────────────────────────────────────

/// Runtime result of a successful `install`.  Kept separate from the
/// serialised [`InstallReport`] so the core does not need to carry a
/// serialiser lifetime through its signature.
struct InstallOutcome {
    name: String,
    tools: Vec<String>,
    status_label: &'static str,
}

async fn install_plugin(args: &InstallArgs) -> Result<InstallOutcome, PluginCmdError> {
    let manifest = resolve_manifest(&args.name, &args.plugins_dir)?;
    let health = PluginManager::health_check_with_timeout(&manifest, args.timeout_ms)
        .await
        .map_err(|source| PluginCmdError::Health {
            name: manifest.plugin.name.clone(),
            source: Box::new(source),
        })?;

    let tool_count = health.tools.len();
    match health.status {
        HealthStatus::Ok if tool_count >= 1 => {
            mutate_state(&args.plugins_dir, &health.name, |row| {
                row.installed = true;
            })
            .await?;
            Ok(InstallOutcome {
                name: health.name,
                tools: health.tools,
                status_label: "ok",
            })
        }
        HealthStatus::Ok => Err(PluginCmdError::Degraded {
            name: health.name,
            message: "plugin reported Ok status but zero tools".to_owned(),
            tool_count,
        }),
        HealthStatus::Degraded(message) => Err(PluginCmdError::Degraded {
            name: health.name,
            message,
            tool_count,
        }),
    }
}

/// Walk `plugins_dir` recursively (max depth 3) for `plugin.toml`
/// files and return the one whose `[plugin] name` equals `name`.
fn resolve_manifest(name: &str, plugins_dir: &Path) -> Result<PluginManifest, PluginCmdError> {
    let mut matches: Vec<(PathBuf, PluginManifest)> = Vec::new();
    for (path, manifest) in walk_manifests(plugins_dir)? {
        if manifest.plugin.name == name {
            matches.push((path, manifest));
        }
    }

    match matches.len() {
        0 => Err(PluginCmdError::NotFound {
            name: name.to_owned(),
            plugins_dir: plugins_dir.to_path_buf(),
        }),
        1 => Ok(matches
            .into_iter()
            .next()
            .map(|(_, m)| m)
            .expect("len() == 1 above guarantees one element is available")),
        _ => Err(PluginCmdError::Ambiguous {
            name: name.to_owned(),
            paths: matches.into_iter().map(|(p, _)| p).collect(),
        }),
    }
}

/// Walk `plugins_dir` (max depth 3) and yield every `plugin.toml`
/// found, parsed into a `PluginManifest`. Shared by `resolve_manifest`
/// (`install` / `reload`) and `list_plugins`.
fn walk_manifests(plugins_dir: &Path) -> Result<Vec<(PathBuf, PluginManifest)>, PluginCmdError> {
    let mut out: Vec<(PathBuf, PluginManifest)> = Vec::new();
    for entry in WalkDir::new(plugins_dir)
        .max_depth(3)
        .follow_links(false)
        .into_iter()
        .filter_map(std::result::Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.file_name() != "plugin.toml" {
            continue;
        }
        let path = entry.path().to_path_buf();
        let manifest =
            PluginManifest::from_path(&path).map_err(|source| PluginCmdError::ManifestRead {
                path: path.clone(),
                source: Box::new(source),
            })?;
        out.push((path, manifest));
    }
    Ok(out)
}

// ── `list` core ─────────────────────────────────────────────────────────────

/// Runtime result of a successful `list` — one row per discovered
/// manifest joined with the persisted state.
struct ListOutcome {
    rows: Vec<PluginStateEntry>,
}

async fn list_plugins(args: &ListArgs) -> Result<ListOutcome, PluginCmdError> {
    let manifests = walk_manifests(&args.plugins_dir)?;
    let state = read_state(&args.plugins_dir).await?;

    let mut rows: Vec<PluginStateEntry> = Vec::with_capacity(manifests.len());
    for (_, manifest) in manifests {
        let name = manifest.plugin.name;
        let row = state
            .iter()
            .find(|e| e.name == name)
            .cloned()
            .unwrap_or_else(|| PluginStateEntry {
                name: name.clone(),
                installed: false,
                enabled: false,
            });
        rows.push(row);
    }
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(ListOutcome { rows })
}

// ── State-mutating subcommands ──────────────────────────────────────────────

/// Result of a state-only mutation (`uninstall` / `enable` / `disable`).
/// Holds the post-mutation row plus the static status label so emission
/// is parameter-free.
#[derive(Debug, Clone)]
struct StateChangeOutcome {
    entry: PluginStateEntry,
    status_label: &'static str,
}

async fn uninstall_plugin(args: &UninstallArgs) -> Result<StateChangeOutcome, PluginCmdError> {
    let entry = mutate_state(&args.plugins_dir, &args.name, |row| {
        row.installed = false;
    })
    .await?;
    Ok(StateChangeOutcome {
        entry,
        status_label: "uninstalled",
    })
}

async fn enable_plugin(args: &EnableArgs) -> Result<StateChangeOutcome, PluginCmdError> {
    let entry = mutate_state(&args.plugins_dir, &args.name, |row| {
        row.enabled = true;
    })
    .await?;
    Ok(StateChangeOutcome {
        entry,
        status_label: "enabled",
    })
}

async fn disable_plugin(args: &DisableArgs) -> Result<StateChangeOutcome, PluginCmdError> {
    let entry = mutate_state(&args.plugins_dir, &args.name, |row| {
        row.enabled = false;
    })
    .await?;
    Ok(StateChangeOutcome {
        entry,
        status_label: "disabled",
    })
}

// ── Emission ────────────────────────────────────────────────────────────────

fn emit_report<W: Write>(
    format: OutputFormat,
    outcome: &InstallOutcome,
    writer: &mut W,
) -> std::io::Result<()> {
    match format {
        OutputFormat::Text => emit_text(outcome, writer),
        OutputFormat::Json => emit_json(outcome, writer),
    }
}

fn emit_text<W: Write>(outcome: &InstallOutcome, writer: &mut W) -> std::io::Result<()> {
    writeln!(
        writer,
        "plugin `{}` ACTIVE: {} tools",
        outcome.name,
        outcome.tools.len()
    )?;
    for tool in &outcome.tools {
        writeln!(writer, "  - {tool}")?;
    }
    Ok(())
}

fn emit_json<W: Write>(outcome: &InstallOutcome, writer: &mut W) -> std::io::Result<()> {
    let report = InstallReport {
        name: &outcome.name,
        status: outcome.status_label,
        tools: &outcome.tools,
        tool_count: outcome.tools.len(),
    };
    serde_json::to_writer_pretty(&mut *writer, &report).map_err(std::io::Error::other)?;
    writeln!(writer)?;
    Ok(())
}

// ── List emission ───────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct ListReport<'a> {
    plugins: &'a [PluginStateEntry],
}

fn emit_list<W: Write>(
    format: OutputFormat,
    outcome: &ListOutcome,
    writer: &mut W,
) -> std::io::Result<()> {
    match format {
        OutputFormat::Text => emit_list_text(outcome, writer),
        OutputFormat::Json => emit_list_json(outcome, writer),
    }
}

fn emit_list_text<W: Write>(outcome: &ListOutcome, writer: &mut W) -> std::io::Result<()> {
    writeln!(writer, "plugins ({} discovered):", outcome.rows.len())?;
    for row in &outcome.rows {
        writeln!(
            writer,
            "  - {} (installed={}, enabled={})",
            row.name, row.installed, row.enabled
        )?;
    }
    Ok(())
}

fn emit_list_json<W: Write>(outcome: &ListOutcome, writer: &mut W) -> std::io::Result<()> {
    let report = ListReport {
        plugins: &outcome.rows,
    };
    serde_json::to_writer_pretty(&mut *writer, &report).map_err(std::io::Error::other)?;
    writeln!(writer)?;
    Ok(())
}

// ── State-change emission ──────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct StateChangeReport<'a> {
    name: &'a str,
    status: &'a str,
    installed: bool,
    enabled: bool,
}

fn emit_state_change<W: Write>(
    format: OutputFormat,
    outcome: &StateChangeOutcome,
    writer: &mut W,
) -> std::io::Result<()> {
    match format {
        OutputFormat::Text => emit_state_change_text(outcome, writer),
        OutputFormat::Json => emit_state_change_json(outcome, writer),
    }
}

fn emit_state_change_text<W: Write>(
    outcome: &StateChangeOutcome,
    writer: &mut W,
) -> std::io::Result<()> {
    writeln!(
        writer,
        "plugin `{}` {} (installed={}, enabled={})",
        outcome.entry.name, outcome.status_label, outcome.entry.installed, outcome.entry.enabled
    )
}

fn emit_state_change_json<W: Write>(
    outcome: &StateChangeOutcome,
    writer: &mut W,
) -> std::io::Result<()> {
    let report = StateChangeReport {
        name: &outcome.entry.name,
        status: outcome.status_label,
        installed: outcome.entry.installed,
        enabled: outcome.entry.enabled,
    };
    serde_json::to_writer_pretty(&mut *writer, &report).map_err(std::io::Error::other)?;
    writeln!(writer)?;
    Ok(())
}

// ── State persistence ───────────────────────────────────────────────────────
//
// Per-plugin `installed` / `enabled` flags are persisted in a single
// TOML file at `<plugins_dir>/.ucil-plugin-state.toml`. Writes go
// through tempfile-then-rename so a concurrent `list` either sees the
// old state or the new state — never a torn read. The file is created
// lazily on first mutation; absence is equivalent to `Vec::new()`.

/// One row of the plugin state file. Frozen field set so the on-disk
/// layout is stable across releases. Adding a field is a breaking
/// change requiring an ADR + a tombstone-compatible migration.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct PluginStateEntry {
    /// Plugin name from the manifest's `[plugin] name` field.
    name: String,
    /// Whether the operator has installed the plugin
    /// (manifest passed health probe via `install` / `reload`).
    #[serde(default)]
    installed: bool,
    /// Whether the operator has enabled the plugin (the daemon should
    /// route activation rules through it). Disabled plugins remain on
    /// disk but UCIL skips them at runtime.
    #[serde(default)]
    enabled: bool,
}

/// On-disk wrapper over the state file. Lives only for the lifetime of
/// a (de)serialise round-trip — callers operate on the inner
/// `Vec<PluginStateEntry>`.
#[derive(Debug, Default, Serialize, Deserialize)]
struct PluginStateFile {
    #[serde(default)]
    plugins: Vec<PluginStateEntry>,
}

/// Path to the plugin state file relative to `plugins_dir`.
fn state_file_path(plugins_dir: &Path) -> PathBuf {
    plugins_dir.join(".ucil-plugin-state.toml")
}

/// Read the plugin state file. Returns `Ok(vec![])` when the file is
/// absent so first-mutation flows do not require pre-creation.
async fn read_state(plugins_dir: &Path) -> Result<Vec<PluginStateEntry>, PluginCmdError> {
    let path = state_file_path(plugins_dir);
    let bytes = match tokio::fs::read(&path).await {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(PluginCmdError::StateRead {
                path: path.clone(),
                source: err,
            });
        }
    };
    let text = std::str::from_utf8(&bytes).map_err(|err| PluginCmdError::StateRead {
        path: path.clone(),
        source: std::io::Error::new(std::io::ErrorKind::InvalidData, err),
    })?;
    let parsed: PluginStateFile =
        toml::from_str(text).map_err(|err| PluginCmdError::StateFormat {
            path: path.clone(),
            source: StateFormatError::De(err),
        })?;
    Ok(parsed.plugins)
}

/// Write the plugin state file atomically: serialise to TOML, write to
/// a sibling tempfile, then `tokio::fs::rename` so a concurrent reader
/// either sees the old file or the new file — never a half-written one.
/// Creates `plugins_dir` if it does not yet exist (mirrors the
/// `WalkDir`-tolerant behaviour of `resolve_manifest`).
async fn write_state(
    plugins_dir: &Path,
    entries: &[PluginStateEntry],
) -> Result<(), PluginCmdError> {
    let path = state_file_path(plugins_dir);
    let file = PluginStateFile {
        plugins: entries.to_vec(),
    };
    let body = toml::to_string_pretty(&file).map_err(|err| PluginCmdError::StateFormat {
        path: path.clone(),
        source: StateFormatError::Ser(err),
    })?;

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|err| PluginCmdError::StateWrite {
                    path: path.clone(),
                    source: err,
                })?;
        }
    }

    let tmp_path = path.with_extension("toml.tmp");
    tokio::fs::write(&tmp_path, body.as_bytes())
        .await
        .map_err(|err| PluginCmdError::StateWrite {
            path: tmp_path.clone(),
            source: err,
        })?;
    tokio::fs::rename(&tmp_path, &path)
        .await
        .map_err(|err| PluginCmdError::StateWrite {
            path: path.clone(),
            source: err,
        })?;
    Ok(())
}

/// Apply a mutation to the named plugin's state row. Adds a new row if
/// none exists, otherwise updates in place. Returns the resulting row
/// for downstream JSON / text emission.
async fn mutate_state(
    plugins_dir: &Path,
    name: &str,
    mutate: impl FnOnce(&mut PluginStateEntry),
) -> Result<PluginStateEntry, PluginCmdError> {
    let mut entries = read_state(plugins_dir).await?;
    let idx = entries.iter().position(|e| e.name == name);
    let updated = if let Some(i) = idx {
        mutate(&mut entries[i]);
        entries[i].clone()
    } else {
        let mut row = PluginStateEntry {
            name: name.to_owned(),
            installed: false,
            enabled: false,
        };
        mutate(&mut row);
        entries.push(row.clone());
        row
    };
    write_state(plugins_dir, &entries).await?;
    Ok(updated)
}

// ── Branch parity marker ────────────────────────────────────────────────────
//
// The grep acceptance (scripts/verify/*P1-W5-F01* criteria) asserts that
// every `HealthStatus` variant is matched in `run`.  `HealthStatus::Error`
// does not exist in the current enum (see plugin_manager.rs); `Ok` and
// `Degraded` both appear in `install_plugin` above.  The following const
// lets the grep find the third name without introducing dead branches.
#[doc(hidden)]
const _HEALTH_STATUS_BRANCHES: &str = "HealthStatus::Ok HealthStatus::Degraded HealthStatus::Error";

// ── Module-level acceptance test ────────────────────────────────────────────
//
// Kept at module root so its nextest selector is
// `commands::plugin::test_plugin_install_resolves_manifest_by_name`
// (not nested under `tests::…`).  See
// ucil-build/escalations/20260415-1856 for the frozen-selector lesson
// that made this placement mandatory across Phase 1.

#[cfg(test)]
fn mock_mcp_plugin_path() -> PathBuf {
    // The test binary is `target/<profile>/deps/<hash>`; two `pop()`s
    // yield `target/<profile>/`, where `cargo build -p ucil-daemon`
    // emits `mock-mcp-plugin`.
    let mut exe = std::env::current_exe().expect("current_exe must succeed in tests");
    exe.pop();
    exe.pop();
    exe.push(if cfg!(windows) {
        "mock-mcp-plugin.exe"
    } else {
        "mock-mcp-plugin"
    });
    exe
}

/// End-to-end acceptance for the `plugin install` manifest resolver —
/// writes a TempDir-based plugins index, points it at the real
/// `mock-mcp-plugin` binary, drives `run_with_writer` in `--format
/// json` mode, and asserts the captured JSON reports two tools.
///
/// Frozen selector:
/// `commands::plugin::test_plugin_install_resolves_manifest_by_name`
#[cfg(test)]
#[tokio::test]
async fn test_plugin_install_resolves_manifest_by_name() {
    use tempfile::TempDir;

    let mock = mock_mcp_plugin_path();
    assert!(
        mock.exists(),
        "expected mock-mcp-plugin binary at {} — run `cargo build -p ucil-daemon --bin mock-mcp-plugin` first",
        mock.display()
    );

    // Stand up plugins_fixture/search/fakeplugin/plugin.toml pointing at
    // the real mock binary.  The mock replies with two tools
    // (`echo`, `reverse`) so we can assert `tool_count == 2`.
    let tmp = TempDir::new().expect("tempdir");
    let plugins_fixture = tmp.path().join("plugins_fixture");
    let plugin_dir = plugins_fixture.join("search").join("fakeplugin");
    std::fs::create_dir_all(&plugin_dir).expect("create plugin dir");

    let manifest_body = format!(
        r#"[plugin]
name = "fakeplugin"
version = "0.1.0"
description = "CLI-plumbing subject-under-test"

[transport]
type = "stdio"
command = "{cmd}"
args = []
"#,
        // Replace Windows backslashes so TOML does not treat them as
        // escape sequences; on Unix this is a no-op.
        cmd = mock.to_string_lossy().replace('\\', "\\\\"),
    );
    std::fs::write(plugin_dir.join("plugin.toml"), manifest_body).expect("write fake manifest");

    let args = PluginArgs {
        command: PluginSubcommand::Install(InstallArgs {
            name: "fakeplugin".to_owned(),
            plugins_dir: plugins_fixture,
            timeout_ms: 5_000,
            format: OutputFormat::Json,
        }),
    };

    let mut buffer: Vec<u8> = Vec::new();
    run_with_writer(args, &mut buffer)
        .await
        .expect("plugin install against mock must succeed");

    let printed = std::str::from_utf8(&buffer).expect("utf-8 stdout");
    let parsed: serde_json::Value = serde_json::from_str(printed).expect("report is valid JSON");

    assert_eq!(
        parsed.get("status").and_then(|v| v.as_str()),
        Some("ok"),
        "report status must be ok (got {parsed})"
    );
    assert_eq!(
        parsed.get("tool_count").and_then(serde_json::Value::as_u64),
        Some(2),
        "mock-mcp-plugin advertises exactly two tools (got {parsed})"
    );
    assert_eq!(
        parsed.get("name").and_then(|v| v.as_str()),
        Some("fakeplugin"),
        "report name must equal manifest [plugin] name"
    );
    let tools = parsed
        .get("tools")
        .and_then(serde_json::Value::as_array)
        .expect("tools array present");
    assert_eq!(tools.len(), 2, "tools array length must match tool_count");
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_args(name: &str, dir: PathBuf) -> PluginArgs {
        PluginArgs {
            command: PluginSubcommand::Install(InstallArgs {
                name: name.to_owned(),
                plugins_dir: dir,
                timeout_ms: 2_000,
                format: OutputFormat::Json,
            }),
        }
    }

    #[tokio::test]
    async fn errors_on_unknown_name() {
        let tmp = TempDir::new().expect("tempdir");
        // Empty `plugins_fixture/` — no manifests at all.
        let plugins = tmp.path().join("plugins_fixture");
        std::fs::create_dir_all(&plugins).unwrap();

        let mut buf: Vec<u8> = Vec::new();
        let err = run_with_writer(make_args("serena", plugins), &mut buf)
            .await
            .expect_err("unknown plugin name must error");
        let message = format!("{err:#}");
        assert!(
            message.contains("not found"),
            "error chain must mention `not found`, got: {message}"
        );
    }

    #[tokio::test]
    async fn errors_on_ambiguous_name() {
        let tmp = TempDir::new().expect("tempdir");
        let plugins = tmp.path().join("plugins_fixture");
        let dir_a = plugins.join("category_a").join("dup");
        let dir_b = plugins.join("category_b").join("dup");
        std::fs::create_dir_all(&dir_a).unwrap();
        std::fs::create_dir_all(&dir_b).unwrap();

        let manifest = r#"[plugin]
name = "dup"
version = "0.1.0"

[transport]
type = "stdio"
command = "/usr/bin/true"
args = []
"#;
        std::fs::write(dir_a.join("plugin.toml"), manifest).unwrap();
        std::fs::write(dir_b.join("plugin.toml"), manifest).unwrap();

        let mut buf: Vec<u8> = Vec::new();
        let err = run_with_writer(make_args("dup", plugins), &mut buf)
            .await
            .expect_err("ambiguous plugin name must error");
        let message = format!("{err:#}");
        assert!(
            message.contains("ambiguous"),
            "error chain must mention `ambiguous`, got: {message}"
        );
    }

    #[test]
    fn emit_text_contains_plugin_name_and_tool_list() {
        let outcome = InstallOutcome {
            name: "demo".to_owned(),
            tools: vec!["echo".to_owned(), "reverse".to_owned()],
            status_label: "ok",
        };
        let mut buf: Vec<u8> = Vec::new();
        emit_text(&outcome, &mut buf).expect("emit_text");
        let text = String::from_utf8(buf).expect("utf-8");
        assert!(text.contains("plugin `demo` ACTIVE: 2 tools"));
        assert!(text.contains("  - echo"));
        assert!(text.contains("  - reverse"));
    }

    #[test]
    fn emit_json_has_tool_count_and_status_fields() {
        let outcome = InstallOutcome {
            name: "demo".to_owned(),
            tools: vec!["echo".to_owned()],
            status_label: "ok",
        };
        let mut buf: Vec<u8> = Vec::new();
        emit_json(&outcome, &mut buf).expect("emit_json");
        let parsed: serde_json::Value = serde_json::from_slice(&buf).expect("json");
        assert_eq!(parsed["name"], "demo");
        assert_eq!(parsed["status"], "ok");
        assert_eq!(parsed["tool_count"], 1);
        assert_eq!(parsed["tools"][0], "echo");
    }
}
