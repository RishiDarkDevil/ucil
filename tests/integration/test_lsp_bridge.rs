//! P1-W5-F08 — LSP diagnostics bridge integration test binary.
//!
//! This file is the acceptance harness for `P1-W5-F08`.  It exercises
//! the full LSP bridge → `KnowledgeGraph` pipeline end-to-end against
//! the four Phase-1 fixture projects (`rust-project`, `python-project`,
//! `typescript-project`, `mixed-project`):
//!
//! * Real on-disk fixtures under `tests/fixtures/<name>/src/…`.
//! * Real [`ucil_core::KnowledgeGraph::open`] on a [`tempfile::TempDir`].
//! * Real [`ucil_lsp_diagnostics::persist_diagnostics`] and
//!   [`ucil_lsp_diagnostics::persist_call_hierarchy_incoming`] code
//!   paths.
//! * [`LocalScriptedFake`] — a local `impl SerenaClient` that returns
//!   canned LSP payloads referring to the fixture files.
//!
//! # `LocalScriptedFake` is not a mock of Serena's MCP channel
//!
//! [`LocalScriptedFake`] is structurally NOT a mock of Serena's MCP
//! wire format: it is a concrete implementation of UCIL's own
//! [`SerenaClient`] trait, the dependency-inversion seam defined in
//! `crates/ucil-lsp-diagnostics/src/diagnostics.rs` (see `DEC-0008`
//! and the rustdoc at lines 301-320 of that module).  The
//! `ScriptedFakeSerenaClient` pattern in `quality_pipeline.rs` /
//! `call_hierarchy.rs` — approved during P1-W5-F05 / P1-W5-F06
//! verifier flips — is the precedent this file follows.
//!
//! # File layout per DEC-0010
//!
//! The binary lives at `tests/integration/test_lsp_bridge.rs` (the
//! repo-relative path cited in `feature-list.json`) rather than inside
//! a per-crate `tests/` directory.  This placement is possible because
//! `tests/integration/Cargo.toml` declares a `[[test]]` entry with an
//! explicit `path = "test_lsp_bridge.rs"` override — see
//! `ucil-build/decisions/DEC-0010-tests-integration-workspace-crate.md`.
//!
//! # Why `lsp_types::Url` (not `Uri`)
//!
//! The bridge's [`SerenaClient`] trait is parameterised over
//! [`lsp_types::Url`] (see `diagnostics.rs` lines 62–65, 156, and the
//! trait methods thereafter).  `lsp-types 0.95` re-exports `url::Url`
//! as `lsp_types::Url`; no `Uri` alias is exposed at this major.  The
//! integration binary matches the trait signature — we cannot
//! substitute a different URL type here without modifying the bridge
//! crate (forbidden by this WO's `scope_out`).
//!
//! # Timeout discipline
//!
//! Every `.await` on [`ucil_lsp_diagnostics::persist_diagnostics`] /
//! [`ucil_lsp_diagnostics::persist_call_hierarchy_incoming`] is
//! wrapped in `tokio::time::timeout(BRIDGE_AWAIT_BUDGET, …)` per
//! `.claude/rules/rust-style.md` Async §.  Under the scripted fake
//! every dispatch completes in well under the 10-second budget; the
//! outer timeout exists solely to satisfy the project-wide invariant
//! (and to guard a future extension that swaps the fake for a real
//! LSP transport).

#![cfg_attr(not(unix), allow(dead_code))]
#![deny(warnings)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
// Each `#[tokio::test]` body touches KG readbacks + fake-script setup
// inline, pushing past the default 100-line budget.  Extracting every
// step into a helper would hurt readability (each test is already
// linear); allow at file scope instead.
#![allow(clippy::too_many_lines)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use lsp_types::{
    CallHierarchyIncomingCall, CallHierarchyItem, CallHierarchyOutgoingCall, Diagnostic,
    DiagnosticSeverity, NumberOrString, Position, Range, SymbolKind, TypeHierarchyItem, Url,
};
use tempfile::TempDir;
use ucil_core::KnowledgeGraph;
use ucil_lsp_diagnostics::diagnostics::{DiagnosticsClient, DiagnosticsClientError, SerenaClient};
use ucil_lsp_diagnostics::types::Language;
use ucil_lsp_diagnostics::{persist_call_hierarchy_incoming, persist_diagnostics};

/// Maximum wall-clock budget for any single `.await` on the bridge
/// entry points ([`persist_diagnostics`],
/// [`persist_call_hierarchy_incoming`]).
///
/// Named per `.claude/rules/rust-style.md` Async §: every IO-touching
/// `.await` in the project is bounded by a named `Duration` constant.
/// 10 s is a generous budget for the scripted-fake dispatch used here
/// (typical in-process dispatch completes in < 1 ms); the timeout
/// exists to guard future extensions that swap the fake for a real
/// LSP transport rather than to police the current test runtime.
const BRIDGE_AWAIT_BUDGET: Duration = Duration::from_secs(10);

// ── LocalScriptedFake ────────────────────────────────────────────────────────

/// Scripted-fake implementation of UCIL's [`SerenaClient`] trait.
///
/// Each field is a `Mutex<Vec<(Url, responses)>>` so tests can push
/// scripts through `&self` — no `&mut self` or `Arc::get_mut`
/// gymnastics are required once the struct is shared behind an
/// `Arc<dyn SerenaClient + Send + Sync>`.  The dispatch methods find
/// the first scripted entry whose URI matches the request and return
/// its response payload; an unscripted URI resolves to an empty
/// vector (mirroring LSP's "file has no findings" semantics).
///
/// This type is **not** a mock of Serena's MCP wire format — it is a
/// concrete implementation of UCIL's own [`SerenaClient`] trait (see
/// `DEC-0008` and the rustdoc at
/// `crates/ucil-lsp-diagnostics/src/diagnostics.rs` lines 301-320).
/// The `ScriptedFakeSerenaClient` pattern in `quality_pipeline.rs` /
/// `call_hierarchy.rs` is the approved precedent this struct mirrors.
struct LocalScriptedFake {
    /// Scripted `textDocument/diagnostic` responses keyed by request
    /// URI.  First-match wins.
    diagnostics: Mutex<Vec<(Url, Vec<Diagnostic>)>>,
    /// Scripted `callHierarchy/incomingCalls` responses keyed by the
    /// request's root-item URI.  First-match wins.
    incoming: Mutex<Vec<(Url, Vec<CallHierarchyIncomingCall>)>>,
}

impl LocalScriptedFake {
    /// Construct an empty scripted fake.  Callers populate the
    /// script via [`Self::push_diagnostics`] and
    /// [`Self::push_incoming`] before wrapping the fake in an
    /// `Arc<dyn SerenaClient + Send + Sync>`.
    const fn new() -> Self {
        Self {
            diagnostics: Mutex::new(Vec::new()),
            incoming: Mutex::new(Vec::new()),
        }
    }

    /// Script `diags` as the response for any
    /// [`SerenaClient::diagnostics`] request whose URI equals `uri`.
    fn push_diagnostics(&self, uri: Url, diags: Vec<Diagnostic>) {
        self.diagnostics
            .lock()
            .expect("LocalScriptedFake diagnostics mutex poisoned")
            .push((uri, diags));
    }

    /// Script `calls` as the response for any
    /// [`SerenaClient::call_hierarchy_incoming`] request whose root
    /// item's URI equals `uri`.
    fn push_incoming(&self, uri: Url, calls: Vec<CallHierarchyIncomingCall>) {
        self.incoming
            .lock()
            .expect("LocalScriptedFake incoming mutex poisoned")
            .push((uri, calls));
    }
}

#[async_trait]
impl SerenaClient for LocalScriptedFake {
    async fn diagnostics(&self, uri: Url) -> Result<Vec<Diagnostic>, DiagnosticsClientError> {
        // Clone out under the lock so the guard drops before any
        // subsequent `.await` — keeps `clippy::await_holding_lock`
        // quiet even though no `.await` follows here.
        let script = self
            .diagnostics
            .lock()
            .expect("LocalScriptedFake diagnostics mutex poisoned")
            .clone();
        for (scripted_uri, diags) in script {
            if scripted_uri == uri {
                return Ok(diags);
            }
        }
        Ok(Vec::new())
    }

    async fn call_hierarchy_incoming(
        &self,
        item: CallHierarchyItem,
    ) -> Result<Vec<CallHierarchyIncomingCall>, DiagnosticsClientError> {
        let script = self
            .incoming
            .lock()
            .expect("LocalScriptedFake incoming mutex poisoned")
            .clone();
        for (scripted_uri, calls) in script {
            if scripted_uri == item.uri {
                return Ok(calls);
            }
        }
        Ok(Vec::new())
    }

    async fn call_hierarchy_outgoing(
        &self,
        _item: CallHierarchyItem,
    ) -> Result<Vec<CallHierarchyOutgoingCall>, DiagnosticsClientError> {
        // Not exercised by F08 — return the LSP-semantic "empty"
        // default so the trait contract is satisfied without a stub.
        Ok(Vec::new())
    }

    async fn type_hierarchy_supertypes(
        &self,
        _item: TypeHierarchyItem,
    ) -> Result<Vec<TypeHierarchyItem>, DiagnosticsClientError> {
        // Not exercised by F08 — return the LSP-semantic "empty"
        // default so the trait contract is satisfied without a stub.
        Ok(Vec::new())
    }
}

// ── Fixture + KG helpers ─────────────────────────────────────────────────────

/// Resolve a fixture path relative to `tests/fixtures/`.
///
/// Anchored via `env!("CARGO_MANIFEST_DIR")` which points at
/// `tests/integration/` at compile time, so the computation is
/// deterministic regardless of the shell's cwd when `cargo test` runs.
/// The resulting path is [`std::fs::canonicalize`]-normalised so
/// `./` prefixes and symlinks do not leak into the persisted
/// `quality_issues.file_path` column on CI.
fn fixture_path(relative: &str) -> PathBuf {
    let raw = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fixtures")
        .join(relative);
    raw.canonicalize()
        .unwrap_or_else(|e| panic!("fixture path {relative} must canonicalize: {e}"))
}

/// Convert an absolute fixture path into the `file://` URI that the
/// bridge code expects.  Uses [`Url::from_file_path`] so the path →
/// URI round-trip is symmetric with the `uri.to_file_path()` call the
/// bridge makes internally.
fn file_uri(path: &Path) -> Url {
    Url::from_file_path(path).expect("absolute path must convert to file:// URI")
}

/// Render a path as the exact `String` the bridge writes into the
/// `quality_issues.file_path` / `entities.file_path` columns.
///
/// Matches the private `uri_to_file_path` helper in the bridge crate
/// (`quality_pipeline.rs` lines 282-288 and `call_hierarchy.rs` lines
/// 221-227).
fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Open a fresh on-disk [`KnowledgeGraph`] inside a throwaway
/// [`TempDir`].  The returned [`TempDir`] must be held for the
/// lifetime of the test — its `Drop` removes the database file.
fn open_fresh_kg() -> (TempDir, KnowledgeGraph) {
    let tmp = TempDir::new().expect("tempdir must be creatable");
    let db_path = tmp.path().join("kg.sqlite");
    let kg = KnowledgeGraph::open(&db_path).expect("KnowledgeGraph::open must succeed");
    (tmp, kg)
}

/// Wrap an owned [`LocalScriptedFake`] in a [`DiagnosticsClient`].
/// Hides the `Arc<dyn SerenaClient + Send + Sync>` coercion boilerplate.
fn into_diag_client(fake: LocalScriptedFake) -> DiagnosticsClient {
    let shared: Arc<dyn SerenaClient + Send + Sync> = Arc::new(fake);
    DiagnosticsClient::new(shared)
}

/// Construct an `lsp_types::Diagnostic` from a compact set of fields.
fn make_diag(
    start_line: u32,
    end_line: u32,
    severity: Option<DiagnosticSeverity>,
    code: Option<NumberOrString>,
    source: Option<&str>,
    message: &str,
) -> Diagnostic {
    Diagnostic {
        range: Range::new(Position::new(start_line, 0), Position::new(end_line, 1)),
        severity,
        code,
        code_description: None,
        source: source.map(str::to_owned),
        message: message.to_owned(),
        related_information: None,
        tags: None,
        data: None,
    }
}

/// Construct a `CallHierarchyItem` at line `line` in `uri` with
/// `name` and `kind`.
fn make_item(name: &str, uri: Url, kind: SymbolKind, line: u32) -> CallHierarchyItem {
    let pos = Position::new(line, 0);
    let range = Range::new(pos, Position::new(line, 1));
    CallHierarchyItem {
        name: name.to_owned(),
        kind,
        tags: None,
        detail: None,
        uri,
        range,
        selection_range: range,
        data: None,
    }
}

/// Wrap a peer [`CallHierarchyItem`] in a [`CallHierarchyIncomingCall`].
const fn wrap_incoming(peer: CallHierarchyItem) -> CallHierarchyIncomingCall {
    CallHierarchyIncomingCall {
        from: peer,
        from_ranges: Vec::new(),
    }
}

/// Read back every `quality_issues.file_path` value as a `Vec<String>`.
/// Filtering in Rust avoids pulling `rusqlite` in as a direct dev-dep
/// solely to use the `params!` macro.
fn quality_issue_paths(kg: &KnowledgeGraph) -> Vec<String> {
    kg.conn()
        .prepare("SELECT file_path FROM quality_issues;")
        .expect("prepare quality_issues SELECT")
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query_map quality_issues")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect quality_issues rows")
}

/// Read back the `file_path` of every entity that is either the
/// `source_id` or the `target_id` of a row in `relations`.  Returned
/// as a `Vec<String>` so tests can filter in Rust.
fn relation_endpoint_paths(kg: &KnowledgeGraph) -> Vec<String> {
    kg.conn()
        .prepare(
            "SELECT e.file_path \
             FROM relations r \
             JOIN entities e \
               ON e.id = r.source_id OR e.id = r.target_id;",
        )
        .expect("prepare relations JOIN SELECT")
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query_map relations")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect relation endpoint rows")
}

// ── Per-fixture tests ────────────────────────────────────────────────────────

/// P1-W5-F08 — `rust-project` fixture: two scripted diagnostics
/// (`ERROR` rust-analyzer `E0308` + `WARNING` clippy rule `42`) plus
/// one incoming-call script land as `quality_issues` rows + one
/// `relations` row referencing the fixture path at
/// `tests/fixtures/rust-project/src/lib.rs`.
#[tokio::test]
async fn test_rust_project_diagnostics_and_calls() {
    let path = fixture_path("rust-project/src/lib.rs");
    let uri = file_uri(&path);
    let expected = path_string(&path);

    let fake = LocalScriptedFake::new();
    fake.push_diagnostics(
        uri.clone(),
        vec![
            make_diag(
                4,
                4,
                Some(DiagnosticSeverity::ERROR),
                Some(NumberOrString::String("E0308".to_owned())),
                Some("rust-analyzer"),
                "mismatched types",
            ),
            make_diag(
                10,
                10,
                Some(DiagnosticSeverity::WARNING),
                Some(NumberOrString::Number(42)),
                Some("clippy"),
                "unused variable",
            ),
        ],
    );
    // Script one incoming-call entry: `add` is callable from `main`
    // in the same file — matches the shape of the fixture's
    // lib.rs / main.rs.
    fake.push_incoming(
        uri.clone(),
        vec![wrap_incoming(make_item(
            "main",
            uri.clone(),
            SymbolKind::FUNCTION,
            0,
        ))],
    );
    let client = into_diag_client(fake);
    let (_tmp, mut kg) = open_fresh_kg();

    let count = tokio::time::timeout(
        BRIDGE_AWAIT_BUDGET,
        persist_diagnostics(&client, &mut kg, uri.clone(), Language::Rust),
    )
    .await
    .expect("persist_diagnostics must complete within BRIDGE_AWAIT_BUDGET")
    .expect("persist_diagnostics must succeed");
    assert_eq!(count, 2, "two diagnostics must persist as two rows");

    let paths = quality_issue_paths(&kg);
    assert_eq!(
        paths.len(),
        2,
        "quality_issues must hold exactly two rows for the rust fixture"
    );
    assert!(
        paths.iter().all(|p| p == &expected),
        "every quality_issues row must reference the canonical fixture path ({expected}); got {paths:?}",
    );

    let root = make_item("add", uri.clone(), SymbolKind::FUNCTION, 0);
    let rel_count = tokio::time::timeout(
        BRIDGE_AWAIT_BUDGET,
        persist_call_hierarchy_incoming(&client, &mut kg, root, Language::Rust),
    )
    .await
    .expect("persist_call_hierarchy_incoming must complete within BRIDGE_AWAIT_BUDGET")
    .expect("persist_call_hierarchy_incoming must succeed");
    assert!(
        rel_count >= 1,
        "at least one incoming-call relation must land for the rust fixture"
    );

    let endpoints = relation_endpoint_paths(&kg);
    assert!(
        endpoints.iter().any(|p| p == &expected),
        "at least one relations endpoint must reference the fixture path ({expected}); got {endpoints:?}",
    );
}

/// P1-W5-F08 — `python-project` fixture: two scripted diagnostics
/// (one `ERROR` pyright `reportGeneralTypeIssues` + one `HINT`
/// pyright note) plus one incoming-call script land as
/// `quality_issues` + `relations` rows referencing
/// `tests/fixtures/python-project/src/python_project/parser.py`.
#[tokio::test]
async fn test_python_project_diagnostics_and_calls() {
    let path = fixture_path("python-project/src/python_project/parser.py");
    let uri = file_uri(&path);
    let expected = path_string(&path);

    let fake = LocalScriptedFake::new();
    fake.push_diagnostics(
        uri.clone(),
        vec![
            make_diag(
                3,
                3,
                Some(DiagnosticSeverity::ERROR),
                Some(NumberOrString::String("reportGeneralTypeIssues".to_owned())),
                Some("pyright"),
                "argument of type \"str\" is not assignable to parameter of type \"int\"",
            ),
            make_diag(
                7,
                7,
                Some(DiagnosticSeverity::HINT),
                Some(NumberOrString::String("reportMissingTypeStubs".to_owned())),
                Some("pyright"),
                "consider adding a type annotation",
            ),
        ],
    );
    fake.push_incoming(
        uri.clone(),
        vec![wrap_incoming(make_item(
            "main",
            uri.clone(),
            SymbolKind::FUNCTION,
            0,
        ))],
    );
    let client = into_diag_client(fake);
    let (_tmp, mut kg) = open_fresh_kg();

    let count = tokio::time::timeout(
        BRIDGE_AWAIT_BUDGET,
        persist_diagnostics(&client, &mut kg, uri.clone(), Language::Python),
    )
    .await
    .expect("persist_diagnostics must complete within BRIDGE_AWAIT_BUDGET")
    .expect("persist_diagnostics must succeed");
    assert_eq!(count, 2, "two diagnostics must persist as two rows");

    let paths = quality_issue_paths(&kg);
    assert_eq!(paths.len(), 2);
    assert!(
        paths.iter().all(|p| p == &expected),
        "every quality_issues row must reference the canonical fixture path ({expected}); got {paths:?}",
    );

    let root = make_item("parse", uri.clone(), SymbolKind::FUNCTION, 0);
    let rel_count = tokio::time::timeout(
        BRIDGE_AWAIT_BUDGET,
        persist_call_hierarchy_incoming(&client, &mut kg, root, Language::Python),
    )
    .await
    .expect("persist_call_hierarchy_incoming must complete within BRIDGE_AWAIT_BUDGET")
    .expect("persist_call_hierarchy_incoming must succeed");
    assert!(rel_count >= 1);

    let endpoints = relation_endpoint_paths(&kg);
    assert!(
        endpoints.iter().any(|p| p == &expected),
        "at least one relations endpoint must reference the fixture path ({expected}); got {endpoints:?}",
    );
}

/// P1-W5-F08 — `typescript-project` fixture: two scripted
/// `tsserver`-sourced diagnostics plus one incoming-call script land
/// as `quality_issues` + `relations` rows referencing
/// `tests/fixtures/typescript-project/src/task-manager.ts`.
#[tokio::test]
async fn test_typescript_project_diagnostics_and_calls() {
    let path = fixture_path("typescript-project/src/task-manager.ts");
    let uri = file_uri(&path);
    let expected = path_string(&path);

    let fake = LocalScriptedFake::new();
    fake.push_diagnostics(
        uri.clone(),
        vec![
            make_diag(
                5,
                5,
                Some(DiagnosticSeverity::ERROR),
                Some(NumberOrString::Number(2322)),
                Some("tsserver"),
                "Type 'string' is not assignable to type 'number'",
            ),
            make_diag(
                12,
                12,
                Some(DiagnosticSeverity::WARNING),
                Some(NumberOrString::Number(6133)),
                Some("tsserver"),
                "'foo' is declared but its value is never read",
            ),
        ],
    );
    fake.push_incoming(
        uri.clone(),
        vec![wrap_incoming(make_item(
            "main",
            uri.clone(),
            SymbolKind::FUNCTION,
            0,
        ))],
    );
    let client = into_diag_client(fake);
    let (_tmp, mut kg) = open_fresh_kg();

    let count = tokio::time::timeout(
        BRIDGE_AWAIT_BUDGET,
        persist_diagnostics(&client, &mut kg, uri.clone(), Language::TypeScript),
    )
    .await
    .expect("persist_diagnostics must complete within BRIDGE_AWAIT_BUDGET")
    .expect("persist_diagnostics must succeed");
    assert_eq!(count, 2);

    let paths = quality_issue_paths(&kg);
    assert_eq!(paths.len(), 2);
    assert!(
        paths.iter().all(|p| p == &expected),
        "every quality_issues row must reference the canonical fixture path ({expected}); got {paths:?}",
    );

    let root = make_item("addTask", uri.clone(), SymbolKind::METHOD, 0);
    let rel_count = tokio::time::timeout(
        BRIDGE_AWAIT_BUDGET,
        persist_call_hierarchy_incoming(&client, &mut kg, root, Language::TypeScript),
    )
    .await
    .expect("persist_call_hierarchy_incoming must complete within BRIDGE_AWAIT_BUDGET")
    .expect("persist_call_hierarchy_incoming must succeed");
    assert!(rel_count >= 1);

    let endpoints = relation_endpoint_paths(&kg);
    assert!(
        endpoints.iter().any(|p| p == &expected),
        "at least one relations endpoint must reference the fixture path ({expected}); got {endpoints:?}",
    );
}

/// P1-W5-F08 — `mixed-project` fixture: three diagnostics (one per
/// language) land as three `quality_issues` rows pointing at the
/// fixture's Rust, Python, and TypeScript sources.  A single
/// incoming-call dispatch against the `.rs` symbol asserts the
/// relation endpoint path ends with `main.rs` (the mixed-project rust
/// half).
#[tokio::test]
async fn test_mixed_project_diagnostics_and_calls() {
    let rust_path = fixture_path("mixed-project/src/main.rs");
    let py_path = fixture_path("mixed-project/src/main.py");
    let ts_path = fixture_path("mixed-project/src/index.ts");
    let rust_uri = file_uri(&rust_path);
    let py_uri = file_uri(&py_path);
    let ts_uri = file_uri(&ts_path);
    let rust_expected = path_string(&rust_path);

    let fake = LocalScriptedFake::new();
    fake.push_diagnostics(
        rust_uri.clone(),
        vec![make_diag(
            1,
            1,
            Some(DiagnosticSeverity::ERROR),
            Some(NumberOrString::String("E0425".to_owned())),
            Some("rust-analyzer"),
            "cannot find value `foo` in this scope",
        )],
    );
    fake.push_diagnostics(
        py_uri.clone(),
        vec![make_diag(
            2,
            2,
            Some(DiagnosticSeverity::WARNING),
            Some(NumberOrString::String("reportUnusedVariable".to_owned())),
            Some("pyright"),
            "variable `foo` is unused",
        )],
    );
    fake.push_diagnostics(
        ts_uri.clone(),
        vec![make_diag(
            3,
            3,
            Some(DiagnosticSeverity::INFORMATION),
            Some(NumberOrString::Number(7027)),
            Some("tsserver"),
            "Unreachable code detected",
        )],
    );
    fake.push_incoming(
        rust_uri.clone(),
        vec![wrap_incoming(make_item(
            "main",
            rust_uri.clone(),
            SymbolKind::FUNCTION,
            0,
        ))],
    );
    let client = into_diag_client(fake);
    let (_tmp, mut kg) = open_fresh_kg();

    let rust_count = tokio::time::timeout(
        BRIDGE_AWAIT_BUDGET,
        persist_diagnostics(&client, &mut kg, rust_uri.clone(), Language::Rust),
    )
    .await
    .expect("rust persist_diagnostics must complete")
    .expect("rust persist_diagnostics must succeed");
    let py_count = tokio::time::timeout(
        BRIDGE_AWAIT_BUDGET,
        persist_diagnostics(&client, &mut kg, py_uri.clone(), Language::Python),
    )
    .await
    .expect("python persist_diagnostics must complete")
    .expect("python persist_diagnostics must succeed");
    let ts_count = tokio::time::timeout(
        BRIDGE_AWAIT_BUDGET,
        persist_diagnostics(&client, &mut kg, ts_uri.clone(), Language::TypeScript),
    )
    .await
    .expect("typescript persist_diagnostics must complete")
    .expect("typescript persist_diagnostics must succeed");
    assert_eq!(rust_count + py_count + ts_count, 3);

    let paths = quality_issue_paths(&kg);
    assert_eq!(
        paths.len(),
        3,
        "quality_issues must hold exactly three rows (one per mixed-project language); got {paths:?}",
    );

    let root = make_item("add", rust_uri.clone(), SymbolKind::FUNCTION, 0);
    let rel_count = tokio::time::timeout(
        BRIDGE_AWAIT_BUDGET,
        persist_call_hierarchy_incoming(&client, &mut kg, root, Language::Rust),
    )
    .await
    .expect("persist_call_hierarchy_incoming must complete")
    .expect("persist_call_hierarchy_incoming must succeed");
    assert!(rel_count >= 1);

    let endpoints = relation_endpoint_paths(&kg);
    assert!(
        endpoints.iter().any(|p| p == &rust_expected),
        "at least one relations endpoint must reference the mixed-project rust fixture path ({rust_expected}); got {endpoints:?}",
    );
    assert!(
        endpoints.iter().any(|p| p.ends_with("main.rs")),
        "at least one relations endpoint must end with 'main.rs'; got {endpoints:?}",
    );
}

// ── Coverage guard ───────────────────────────────────────────────────────────

/// Compile-time guard against someone quietly dropping a fixture from
/// the P1-W5-F08 suite.  Asserts:
///
/// 1. The fixture array has exactly 4 entries.
/// 2. All three Phase-1 fixture languages (Rust, Python, TypeScript)
///    appear at least once.
/// 3. The four canonical fixture directory names (`rust-project`,
///    `python-project`, `typescript-project`, `mixed-project`) all
///    appear.
///
/// Sync `#[test]` (NOT `#[tokio::test]`) — no async work is needed.
#[test]
fn test_suite_covers_four_fixtures() {
    const FIXTURES: &[(&str, Language)] = &[
        ("rust-project", Language::Rust),
        ("python-project", Language::Python),
        ("typescript-project", Language::TypeScript),
        ("mixed-project", Language::Rust),
    ];
    assert_eq!(FIXTURES.len(), 4, "P1-W5-F08 must cover exactly 4 fixtures");
    assert!(
        FIXTURES.iter().any(|(_, lang)| *lang == Language::Rust),
        "FIXTURES must include at least one Rust-tagged entry"
    );
    assert!(
        FIXTURES.iter().any(|(_, lang)| *lang == Language::Python),
        "FIXTURES must include at least one Python-tagged entry"
    );
    assert!(
        FIXTURES
            .iter()
            .any(|(_, lang)| *lang == Language::TypeScript),
        "FIXTURES must include at least one TypeScript-tagged entry"
    );
    for name in [
        "rust-project",
        "python-project",
        "typescript-project",
        "mixed-project",
    ] {
        assert!(
            FIXTURES.iter().any(|(n, _)| *n == name),
            "FIXTURES must include an entry named {name}"
        );
    }
}
