//! Data types shared by the LSP diagnostics bridge skeleton
//! (`P1-W5-F03`, `WO-0014`).
//!
//! This module is intentionally logic-free — it only defines the
//! placeholder structs and enums that the `bridge` module will hand
//! off to downstream features:
//!
//! * [`Language`] — the trimmed set of source languages the Phase-1
//!   fixtures target.  Future phases may extend the enum (e.g. Ruby,
//!   Bash) once the corresponding LSP servers are in scope.
//! * [`LspEndpoint`] + [`LspTransport`] — a placeholder handle for a
//!   future LSP-server connection.  `P1-W5-F03` only constructs empty
//!   endpoint maps; `P1-W5-F07` populates them with
//!   [`LspTransport::Standalone`] entries when Serena is absent.
//! * [`Diagnostic`] + [`DiagnosticSeverity`] — the minimum-surface
//!   diagnostic record the master-plan §13.3 diagram shows.  `P1-W5-F04`
//!   will produce real values from `textDocument/diagnostic` LSP
//!   responses; this module carries only the data container.
//!
//! All public items derive `serde::Serialize` + `serde::Deserialize`
//! because downstream features (F05 quality-issue feeds, F06
//! architecture deltas, F08 integration fixtures) serialise these
//! values into JSON blobs that cross the MCP boundary.
//!
//! This module has no `Result`-returning API — every item is a data
//! container — so there are no `# Errors` rustdoc sections to write.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

// ── Language ─────────────────────────────────────────────────────────────────

/// The set of source languages the UCIL LSP bridge can address.
///
/// The variant set is intentionally trimmed to match the Phase-1 fixture
/// projects; later phases may extend the enum as additional LSP servers
/// come into scope.  The enum is derived `Copy` + `Hash` so it can key a
/// `HashMap<Language, LspEndpoint>` cheaply.
///
/// Default serde naming (`"Python"`, `"Rust"`, …) is used — downstream
/// work-orders may layer on `#[serde(rename_all = "lowercase")]` if a
/// specific cross-tool wire format demands it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Language {
    /// Python — `pyright-langserver`, `ruff-lsp`, `pylsp`.
    Python,
    /// Rust — `rust-analyzer`.
    Rust,
    /// TypeScript — `tsserver`, `typescript-language-server`.
    TypeScript,
    /// Go — `gopls`.
    Go,
    /// Java — `jdtls`, `eclipse.jdt.ls`.
    Java,
    /// C — `clangd`.
    C,
    /// C++ — `clangd`.
    Cpp,
}

// ── LspTransport ─────────────────────────────────────────────────────────────

/// How an [`LspEndpoint`] reaches its language server.
///
/// Two variants exist so that the enum is exhaustive at `P1-W5-F03`
/// time even though no endpoint is actually constructed yet.
/// `DelegatedToSerena` documents the Serena-active branch (UCIL does
/// not hold a direct LSP connection; requests are forwarded through
/// Serena's MCP channel, per `DEC-0008`).  `Standalone` is a
/// placeholder for `P1-W5-F07` — populated by the degraded-mode
/// spawner once real LSP subprocess management lands.
///
/// `P1-W5-F03` never constructs an [`LspTransport`] — the bridge's
/// endpoint map is empty in both `serena_managed` branches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LspTransport {
    /// UCIL does not own the LSP connection; requests are routed
    /// through Serena's MCP channel by `P1-W5-F04`'s `SerenaClient`
    /// trait.  Reserved for future use — no endpoint map entries use
    /// this variant at `P1-W5-F03` time.
    DelegatedToSerena,
    /// UCIL owns a spawned LSP subprocess.  Populated by
    /// `P1-W5-F07`; `command` is the executable (e.g.
    /// `pyright-langserver`) and `args` are the invocation flags
    /// (e.g. `["--stdio"]`).  Unused at `P1-W5-F03`.
    Standalone {
        /// Executable name or absolute path of the spawned LSP
        /// server (e.g. `"pyright-langserver"`, `"rust-analyzer"`).
        command: String,
        /// Invocation arguments (e.g. `vec!["--stdio".into()]`).
        args: Vec<String>,
    },
}

// ── LspEndpoint ──────────────────────────────────────────────────────────────

/// A handle to a single language-server connection.
///
/// `LspEndpoint` is a placeholder at `P1-W5-F03`: the bridge's
/// endpoint map is introduced empty and remains empty until
/// `P1-W5-F07` fills it with [`LspTransport::Standalone`] entries in
/// the degraded-mode (Serena absent) branch.
///
/// The struct is deliberately small — transport details that `P1-W5-F04`
/// will need (the JSON-RPC reader/writer, capability bitmap, running
/// PID) are not part of this skeleton; they will land as follow-on
/// fields wrapped in `Option` so this module stays API-stable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspEndpoint {
    /// The source language this endpoint speaks.
    pub language: Language,
    /// How UCIL reaches the language server — either delegated to
    /// Serena (no UCIL-owned connection) or a UCIL-spawned
    /// standalone subprocess (`P1-W5-F07` only).
    pub transport: LspTransport,
}

// ── Diagnostic ───────────────────────────────────────────────────────────────

/// A compiler/linter diagnostic anchored to a specific source location.
///
/// This is the minimum surface the master-plan §13.3 bridge diagram
/// requires — the LSP JSON-RPC client in `P1-W5-F04` will populate
/// these fields from `textDocument/diagnostic` responses.  At
/// `P1-W5-F03` the bridge's diagnostics cache is empty; no
/// [`Diagnostic`] values are constructed by the skeleton.
///
/// Note: `ucil-core::types::Diagnostic` is a sibling type used by the
/// broader fusion engine; this bridge-local copy keeps the LSP layer
/// decoupled from core's evolution while the two surfaces are being
/// defined.  `P1-W5-F05` is expected to introduce the adapter that
/// converts this form into the core type before it enters the
/// knowledge graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    /// Absolute path of the file that produced this diagnostic.
    pub file: PathBuf,
    /// 1-indexed line number of the diagnostic anchor.
    pub line: u32,
    /// 1-indexed column offset of the diagnostic anchor.
    pub column: u32,
    /// Severity level reported by the language server.
    pub severity: DiagnosticSeverity,
    /// Human-readable diagnostic description.
    pub message: String,
    /// Tool or language-server name that produced the diagnostic
    /// (e.g. `"rust-analyzer"`, `"pyright"`).  `None` if the server
    /// did not emit a source tag.
    pub source: Option<String>,
}

// ── DiagnosticSeverity ───────────────────────────────────────────────────────

/// LSP diagnostic severity levels, matching the LSP spec's
/// `DiagnosticSeverity` numeric ladder (1 = Error … 4 = Hint).
///
/// Stored as a distinct enum (rather than the numeric code) so that
/// JSON serialisations produced by F05 are self-describing when they
/// flow into the `quality_issues` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DiagnosticSeverity {
    /// LSP severity 1 — a hard compiler/linter error.
    Error,
    /// LSP severity 2 — a warning that does not block compilation.
    Warning,
    /// LSP severity 3 — informational note, e.g. unused import.
    Information,
    /// LSP severity 4 — a hint, e.g. an inlay-hint or refactor
    /// suggestion.
    Hint,
}
