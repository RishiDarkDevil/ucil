//! LSP JSON-RPC dispatch client (`P1-W5-F04`, `WO-0015`).
//!
//! This module implements the Serena-delegation branch of the LSP
//! bridge per master-plan §13.3–§13.5 and `DEC-0008` §4.  When the
//! daemon passes a [`SerenaClient`] impl into
//! [`crate::bridge::LspDiagnosticsBridge::with_serena_client`], the
//! resulting bridge can hand out [`DiagnosticsClient`] instances that
//! dispatch the four LSP requests UCIL needs in Phase 1:
//!
//! * `textDocument/diagnostic` → [`DiagnosticsClient::diagnostics`]
//! * `callHierarchy/incomingCalls` →
//!   [`DiagnosticsClient::call_hierarchy_incoming`]
//! * `callHierarchy/outgoingCalls` →
//!   [`DiagnosticsClient::call_hierarchy_outgoing`]
//! * `typeHierarchy/supertypes` →
//!   [`DiagnosticsClient::type_hierarchy_supertypes`]
//!
//! The degraded-mode branch (Serena absent) is the domain of
//! `P1-W5-F07`; at `P1-W5-F04` time a
//! [`crate::bridge::LspDiagnosticsBridge`] constructed via
//! [`crate::bridge::LspDiagnosticsBridge::new`] with `false` carries
//! no client and surfaces
//! [`crate::bridge::BridgeError::NoLspServerConfigured`] from
//! [`crate::bridge::LspDiagnosticsBridge::require_endpoint`].
//!
//! # Why a [`SerenaClient`] trait?
//!
//! Per `DEC-0008` the `ucil-lsp-diagnostics` crate must not take a
//! direct dependency on `ucil-daemon` — that would create a cycle
//! through the plugin manager.  The [`SerenaClient`] trait is UCIL's
//! own dependency-inversion seam: this crate owns the trait, a future
//! daemon-integration work-order will ship the concrete `PluginManager`
//! -backed impl that forwards requests through Serena's MCP channel,
//! and the trait object (`Arc<dyn SerenaClient + Send + Sync>`) is
//! held by the bridge.
//!
//! The trait is UCIL's abstraction — it is **not** a mock of Serena's
//! MCP wire format.  Implementations (including the test-side
//! `FakeSerenaClient`) are concrete impls of UCIL's trait, not
//! stand-ins for the Serena binary; that distinction matters for the
//! phase-log invariant 1 (no mocking of Serena MCP).
//!
//! # Timeouts
//!
//! Every `.await` in [`DiagnosticsClient`]'s dispatch methods is
//! wrapped in `tokio::time::timeout(Duration::from_millis(LSP_REQUEST_TIMEOUT_MS), …)`
//! per rust-style.md Async §.  A hit surfaces as
//! [`DiagnosticsClientError::Timeout`].

// `DiagnosticsClient` / `DiagnosticsClientError` legitimately repeat
// the module name — the module is named `diagnostics` because the WO
// scopes the exported surface around the LSP diagnostics operations,
// and the types would otherwise collide with the sibling
// `types::Diagnostic` data container.  Allowing the lint at module
// scope keeps the naming consistent without per-item `#[allow]` spam.
#![allow(clippy::module_name_repetitions)]

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use lsp_types::{
    CallHierarchyIncomingCall, CallHierarchyItem, CallHierarchyOutgoingCall,
    Diagnostic as LspDiagnostic, TypeHierarchyItem, Url,
};
use thiserror::Error;

// ── Timeout constant ─────────────────────────────────────────────────────────

/// Maximum wall-clock budget for a single LSP request dispatched
/// through [`DiagnosticsClient`].
///
/// Every `.await` inside [`DiagnosticsClient`] is wrapped in
/// `tokio::time::timeout(Duration::from_millis(LSP_REQUEST_TIMEOUT_MS), …)`
/// per rust-style.md Async §.  A hit surfaces as
/// [`DiagnosticsClientError::Timeout`].
///
/// 5 s is the master-plan §13 budget for LSP round-trips delegated via
/// Serena's MCP channel (`DEC-0008` §4).  The constant lives here
/// rather than in `bridge.rs` because every caller of the dispatch
/// methods shares this single timeout budget — future WOs that split
/// the budget per-operation (e.g. a shorter cap on
/// `textDocument/diagnostic`) should introduce additional named
/// constants beside this one rather than mutating the shared value.
pub const LSP_REQUEST_TIMEOUT_MS: u64 = 5000;

// ── Errors ───────────────────────────────────────────────────────────────────

/// Errors produced by [`DiagnosticsClient`] dispatch methods.
///
/// Marked `#[non_exhaustive]` so downstream crates cannot rely on
/// exhaustive matching — future Phase-1 work-orders will extend this
/// enum with JSON-RPC framing failures, cancellation mid-flight, and
/// UTF-8 decode errors as the Serena-side transport grows richer,
/// and that growth must not constitute a `SemVer` break.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum DiagnosticsClientError {
    /// The LSP request exceeded [`LSP_REQUEST_TIMEOUT_MS`].  The field
    /// is the millisecond budget that was exceeded so the caller's
    /// log message can cite the exact limit.
    #[error("LSP request timed out after {timeout_ms}ms")]
    Timeout {
        /// The millisecond budget that was exceeded — always equals
        /// [`LSP_REQUEST_TIMEOUT_MS`] at present, but is carried
        /// explicitly so future per-operation budgets can be
        /// distinguished in logs without consulting a second source.
        timeout_ms: u64,
    },
    /// The underlying transport (Serena MCP channel, or the future
    /// standalone LSP socket) reported an error.  The [`String`]
    /// carries the transport's human-readable description; callers
    /// should log it rather than parse it.
    #[error("LSP transport error: {message}")]
    Transport {
        /// Human-readable description of the transport failure.
        message: String,
    },
}

// ── SerenaClient trait ───────────────────────────────────────────────────────

/// UCIL's abstraction over "something that can speak LSP requests
/// through a Serena-backed channel".
///
/// This trait is the dependency-inversion seam between
/// `ucil-lsp-diagnostics` (which owns the trait) and a future
/// daemon-integration WO (which will ship the concrete impl that
/// forwards to `ucil-daemon::plugin_manager::PluginManager`).  The
/// trait keeps the crate cycle-free per `DEC-0008` §Consequences.
///
/// Implementations return the LSP wire types directly from the
/// `lsp-types` community crate, so `P1-W5-F05`'s adapter can convert
/// them into UCIL's internal [`crate::types::Diagnostic`] form
/// without a second marshalling pass.
///
/// # Object safety
///
/// The trait is object-safe: every method takes `&self`, the
/// associated types are erased through `#[async_trait]`'s `Box<dyn
/// Future>` lowering, and the `Send + Sync` supertraits flow through
/// to `Arc<dyn SerenaClient + Send + Sync>` so the bridge can share
/// a single client across `tokio::spawn`-ed tasks.
#[async_trait]
pub trait SerenaClient: Send + Sync {
    /// Fetch `textDocument/diagnostic` results for the file at `uri`.
    ///
    /// # Errors
    ///
    /// Returns [`DiagnosticsClientError::Transport`] when the
    /// underlying Serena MCP channel surfaces an error (plugin
    /// unavailable, JSON-RPC decode failure, etc.).  Callers that
    /// wrap the call in [`DiagnosticsClient`] additionally observe
    /// [`DiagnosticsClientError::Timeout`] when the server takes
    /// longer than [`LSP_REQUEST_TIMEOUT_MS`] to respond.
    async fn diagnostics(&self, uri: Url) -> Result<Vec<LspDiagnostic>, DiagnosticsClientError>;

    /// Fetch `callHierarchy/incomingCalls` for the prepared `item`.
    ///
    /// # Errors
    ///
    /// Same contract as [`Self::diagnostics`].
    async fn call_hierarchy_incoming(
        &self,
        item: CallHierarchyItem,
    ) -> Result<Vec<CallHierarchyIncomingCall>, DiagnosticsClientError>;

    /// Fetch `callHierarchy/outgoingCalls` for the prepared `item`.
    ///
    /// # Errors
    ///
    /// Same contract as [`Self::diagnostics`].
    async fn call_hierarchy_outgoing(
        &self,
        item: CallHierarchyItem,
    ) -> Result<Vec<CallHierarchyOutgoingCall>, DiagnosticsClientError>;

    /// Fetch `typeHierarchy/supertypes` for the prepared `item`.
    ///
    /// # Errors
    ///
    /// Same contract as [`Self::diagnostics`].
    async fn type_hierarchy_supertypes(
        &self,
        item: TypeHierarchyItem,
    ) -> Result<Vec<TypeHierarchyItem>, DiagnosticsClientError>;
}

// ── DiagnosticsClient ────────────────────────────────────────────────────────

/// LSP request dispatcher bound to a [`SerenaClient`].
///
/// Each dispatch method:
///
/// 1. Hands the request arguments to the underlying [`SerenaClient`].
/// 2. Wraps the returned future in `tokio::time::timeout` with a
///    [`LSP_REQUEST_TIMEOUT_MS`] budget.
/// 3. Lifts an elapsed timeout into
///    [`DiagnosticsClientError::Timeout`]; transport errors are
///    surfaced verbatim.
///
/// Clone-shared ownership through `Arc<dyn SerenaClient + Send +
/// Sync>` lets the same dispatcher be used across multiple
/// `tokio::spawn`-ed tasks without contention — each clone is a
/// cheap refcount bump.
pub struct DiagnosticsClient {
    /// The Serena-backed transport.  Every dispatch method defers
    /// the actual wire operation to this handle.
    serena: Arc<dyn SerenaClient + Send + Sync>,
}

impl DiagnosticsClient {
    /// Construct a [`DiagnosticsClient`] around an already-built
    /// [`SerenaClient`] handle.
    ///
    /// Typically constructed through
    /// [`crate::bridge::LspDiagnosticsBridge::diagnostics_client`]
    /// rather than directly; the constructor is `pub` so
    /// integration-test harnesses can assemble a client against a
    /// `FakeSerenaClient` without constructing a bridge when the
    /// bridge's endpoint map is irrelevant to the test.
    #[must_use]
    pub fn new(serena: Arc<dyn SerenaClient + Send + Sync>) -> Self {
        Self { serena }
    }

    /// Dispatch `textDocument/diagnostic` for `uri`.
    ///
    /// # Errors
    ///
    /// * [`DiagnosticsClientError::Timeout`] if the LSP server takes
    ///   longer than [`LSP_REQUEST_TIMEOUT_MS`] to respond.
    /// * [`DiagnosticsClientError::Transport`] when the Serena MCP
    ///   channel surfaces an error.
    pub async fn diagnostics(
        &self,
        uri: Url,
    ) -> Result<Vec<LspDiagnostic>, DiagnosticsClientError> {
        let fut = self.serena.diagnostics(uri);
        tokio::time::timeout(Duration::from_millis(LSP_REQUEST_TIMEOUT_MS), fut)
            .await
            .unwrap_or(Err(DiagnosticsClientError::Timeout {
                timeout_ms: LSP_REQUEST_TIMEOUT_MS,
            }))
    }

    /// Dispatch `callHierarchy/incomingCalls` for the prepared
    /// `item`.
    ///
    /// # Errors
    ///
    /// Same contract as [`Self::diagnostics`].
    pub async fn call_hierarchy_incoming(
        &self,
        item: CallHierarchyItem,
    ) -> Result<Vec<CallHierarchyIncomingCall>, DiagnosticsClientError> {
        let fut = self.serena.call_hierarchy_incoming(item);
        tokio::time::timeout(Duration::from_millis(LSP_REQUEST_TIMEOUT_MS), fut)
            .await
            .unwrap_or(Err(DiagnosticsClientError::Timeout {
                timeout_ms: LSP_REQUEST_TIMEOUT_MS,
            }))
    }

    /// Dispatch `callHierarchy/outgoingCalls` for the prepared
    /// `item`.
    ///
    /// # Errors
    ///
    /// Same contract as [`Self::diagnostics`].
    pub async fn call_hierarchy_outgoing(
        &self,
        item: CallHierarchyItem,
    ) -> Result<Vec<CallHierarchyOutgoingCall>, DiagnosticsClientError> {
        let fut = self.serena.call_hierarchy_outgoing(item);
        tokio::time::timeout(Duration::from_millis(LSP_REQUEST_TIMEOUT_MS), fut)
            .await
            .unwrap_or(Err(DiagnosticsClientError::Timeout {
                timeout_ms: LSP_REQUEST_TIMEOUT_MS,
            }))
    }

    /// Dispatch `typeHierarchy/supertypes` for the prepared `item`.
    ///
    /// # Errors
    ///
    /// Same contract as [`Self::diagnostics`].
    pub async fn type_hierarchy_supertypes(
        &self,
        item: TypeHierarchyItem,
    ) -> Result<Vec<TypeHierarchyItem>, DiagnosticsClientError> {
        let fut = self.serena.type_hierarchy_supertypes(item);
        tokio::time::timeout(Duration::from_millis(LSP_REQUEST_TIMEOUT_MS), fut)
            .await
            .unwrap_or(Err(DiagnosticsClientError::Timeout {
                timeout_ms: LSP_REQUEST_TIMEOUT_MS,
            }))
    }
}

// ── Test-side FakeSerenaClient (not a mock of Serena MCP) ────────────────────
//
// This nested submodule exists per the WO-0015 plan: it houses a real
// implementation of UCIL's own [`SerenaClient`] trait that records the
// arguments it was handed so the five module-root `diagnostics::test_*`
// tests below can assert the dispatch path is wired end-to-end.  The
// `FakeSerenaClient` is structurally distinct from mocking Serena's
// MCP wire format — it implements UCIL's own trait, which is the
// dependency-inversion seam the bridge uses (`DEC-0008` §4).

#[cfg(test)]
mod fake_serena_client {
    use std::sync::Mutex;

    use lsp_types::{
        CallHierarchyIncomingCall, CallHierarchyItem, CallHierarchyOutgoingCall,
        Diagnostic as LspDiagnostic, Position, Range, SymbolKind, TypeHierarchyItem, Url,
    };

    use super::{DiagnosticsClientError, SerenaClient};
    use async_trait::async_trait;

    /// Records the arguments handed to each dispatch method so the
    /// enclosing test can assert the dispatch path reached Serena.
    ///
    /// Each `*_calls` field is a `Mutex<Vec<_>>` so the
    /// `&self`-taking trait methods can record through a shared
    /// reference; the mutexes are contention-free in practice (each
    /// test awaits a single dispatch before asserting).  The shared
    /// `_calls` postfix is intentional — it mirrors the four LSP
    /// operation names in the [`super::SerenaClient`] trait, so the
    /// `struct_field_names` lint is allowed at type scope.
    #[allow(clippy::struct_field_names)]
    pub(super) struct FakeSerenaClient {
        pub(super) diagnostics_calls: Mutex<Vec<Url>>,
        pub(super) incoming_calls: Mutex<Vec<CallHierarchyItem>>,
        pub(super) outgoing_calls: Mutex<Vec<CallHierarchyItem>>,
        pub(super) supertypes_calls: Mutex<Vec<TypeHierarchyItem>>,
    }

    impl FakeSerenaClient {
        pub(super) fn new() -> Self {
            Self {
                diagnostics_calls: Mutex::new(Vec::new()),
                incoming_calls: Mutex::new(Vec::new()),
                outgoing_calls: Mutex::new(Vec::new()),
                supertypes_calls: Mutex::new(Vec::new()),
            }
        }
    }

    /// Construct a canned [`CallHierarchyItem`] for round-trip
    /// fixtures.  `uri` is the only field the tests meaningfully
    /// vary; everything else gets a fixed dummy value.
    pub(super) fn fake_call_hierarchy_item(name: &str, uri: Url) -> CallHierarchyItem {
        let pos = Position::new(0, 0);
        let range = Range::new(pos, pos);
        CallHierarchyItem {
            name: name.to_owned(),
            kind: SymbolKind::FUNCTION,
            tags: None,
            detail: None,
            uri,
            range,
            selection_range: range,
            data: None,
        }
    }

    /// Construct a canned [`TypeHierarchyItem`] for round-trip
    /// fixtures.
    pub(super) fn fake_type_hierarchy_item(name: &str, uri: Url) -> TypeHierarchyItem {
        let pos = Position::new(0, 0);
        let range = Range::new(pos, pos);
        TypeHierarchyItem {
            name: name.to_owned(),
            kind: SymbolKind::CLASS,
            tags: None,
            detail: None,
            uri,
            range,
            selection_range: range,
            data: None,
        }
    }

    #[async_trait]
    impl SerenaClient for FakeSerenaClient {
        async fn diagnostics(
            &self,
            uri: Url,
        ) -> Result<Vec<LspDiagnostic>, DiagnosticsClientError> {
            self.diagnostics_calls
                .lock()
                .expect("FakeSerenaClient mutex poisoned")
                .push(uri.clone());
            Ok(vec![LspDiagnostic {
                range: Range::new(Position::new(0, 0), Position::new(0, 1)),
                severity: None,
                code: None,
                code_description: None,
                source: Some("fake-serena".to_owned()),
                message: format!("fake diagnostic for {uri}"),
                related_information: None,
                tags: None,
                data: None,
            }])
        }

        async fn call_hierarchy_incoming(
            &self,
            item: CallHierarchyItem,
        ) -> Result<Vec<CallHierarchyIncomingCall>, DiagnosticsClientError> {
            let uri = item.uri.clone();
            self.incoming_calls
                .lock()
                .expect("FakeSerenaClient mutex poisoned")
                .push(item);
            let caller = fake_call_hierarchy_item("caller_of_target", uri);
            Ok(vec![CallHierarchyIncomingCall {
                from: caller,
                from_ranges: vec![Range::new(Position::new(1, 0), Position::new(1, 4))],
            }])
        }

        async fn call_hierarchy_outgoing(
            &self,
            item: CallHierarchyItem,
        ) -> Result<Vec<CallHierarchyOutgoingCall>, DiagnosticsClientError> {
            let uri = item.uri.clone();
            self.outgoing_calls
                .lock()
                .expect("FakeSerenaClient mutex poisoned")
                .push(item);
            let callee = fake_call_hierarchy_item("callee_of_target", uri);
            Ok(vec![CallHierarchyOutgoingCall {
                to: callee,
                from_ranges: vec![Range::new(Position::new(2, 0), Position::new(2, 4))],
            }])
        }

        async fn type_hierarchy_supertypes(
            &self,
            item: TypeHierarchyItem,
        ) -> Result<Vec<TypeHierarchyItem>, DiagnosticsClientError> {
            let uri = item.uri.clone();
            self.supertypes_calls
                .lock()
                .expect("FakeSerenaClient mutex poisoned")
                .push(item);
            Ok(vec![fake_type_hierarchy_item("SuperType", uri)])
        }
    }
}

// ── Module-root acceptance tests (F04 oracle) ────────────────────────────────
//
// The five tests below live at module root (NOT inside a nested `mod
// tests { … }`) to honour the WO-0006/WO-0007/WO-0010/WO-0011/WO-0013
// /WO-0014 discipline: the `diagnostics::` selector in
// `feature-list.json` is a module prefix, and keeping the frozen tests
// at module root means a future planner who promotes any single test
// to an exact-match selector gets `diagnostics::test_*` rather than
// `diagnostics::tests::test_*`.
//
// The `FakeSerenaClient` impl itself is allowed inside a nested
// `#[cfg(test)] mod fake_serena_client { … }` (per the WO lesson) —
// only the selector-exercising tests must stay at module root.

#[cfg(test)]
#[tokio::test]
async fn test_diagnostics_via_serena() {
    use crate::bridge::LspDiagnosticsBridge;
    use fake_serena_client::FakeSerenaClient;

    let fake = Arc::new(FakeSerenaClient::new());
    let bridge = LspDiagnosticsBridge::with_serena_client(
        fake.clone() as Arc<dyn SerenaClient + Send + Sync>
    );
    let client = bridge
        .diagnostics_client()
        .expect("diagnostics_client must be Some when bridge carries a SerenaClient");

    let uri = Url::parse("file:///fixture/foo.rs").expect("url must parse");
    let diags = client
        .diagnostics(uri.clone())
        .await
        .expect("diagnostics dispatch must succeed");

    assert_eq!(
        diags.len(),
        1,
        "FakeSerenaClient returns one canned diagnostic"
    );
    assert_eq!(
        diags[0].source.as_deref(),
        Some("fake-serena"),
        "canned diagnostic must carry the fake's source tag"
    );

    // Prove the URI actually reached the SerenaClient impl rather
    // than being swallowed by the timeout wrapper.  We clone the
    // recorded vector out of the mutex so the lock is released
    // immediately — `clippy::significant_drop_tightening` objects to
    // holding the guard across the asserts.
    let recorded = fake
        .diagnostics_calls
        .lock()
        .expect("mutex poisoned")
        .clone();
    assert_eq!(recorded.len(), 1, "exactly one dispatch must be recorded");
    assert_eq!(recorded[0], uri, "recorded URI must match the request URI");
}

#[cfg(test)]
#[tokio::test]
async fn test_call_hierarchy_incoming_via_serena() {
    use crate::bridge::LspDiagnosticsBridge;
    use fake_serena_client::{fake_call_hierarchy_item, FakeSerenaClient};

    let fake = Arc::new(FakeSerenaClient::new());
    let bridge = LspDiagnosticsBridge::with_serena_client(
        fake.clone() as Arc<dyn SerenaClient + Send + Sync>
    );
    let client = bridge
        .diagnostics_client()
        .expect("diagnostics_client must be Some when bridge carries a SerenaClient");

    let target_uri = Url::parse("file:///fixture/lib.rs").expect("url must parse");
    let target = fake_call_hierarchy_item("target_fn", target_uri.clone());
    let calls = client
        .call_hierarchy_incoming(target.clone())
        .await
        .expect("call_hierarchy_incoming dispatch must succeed");

    assert_eq!(calls.len(), 1, "FakeSerenaClient returns one caller");
    assert_eq!(
        calls[0].from.name, "caller_of_target",
        "canned caller must carry the fixed name"
    );

    let recorded = fake.incoming_calls.lock().expect("mutex poisoned").clone();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].name, target.name);
    assert_eq!(recorded[0].uri, target_uri);
}

#[cfg(test)]
#[tokio::test]
async fn test_call_hierarchy_outgoing_via_serena() {
    use crate::bridge::LspDiagnosticsBridge;
    use fake_serena_client::{fake_call_hierarchy_item, FakeSerenaClient};

    let fake = Arc::new(FakeSerenaClient::new());
    let bridge = LspDiagnosticsBridge::with_serena_client(
        fake.clone() as Arc<dyn SerenaClient + Send + Sync>
    );
    let client = bridge
        .diagnostics_client()
        .expect("diagnostics_client must be Some when bridge carries a SerenaClient");

    let target_uri = Url::parse("file:///fixture/main.rs").expect("url must parse");
    let target = fake_call_hierarchy_item("calling_fn", target_uri.clone());
    let calls = client
        .call_hierarchy_outgoing(target.clone())
        .await
        .expect("call_hierarchy_outgoing dispatch must succeed");

    assert_eq!(calls.len(), 1, "FakeSerenaClient returns one callee");
    assert_eq!(
        calls[0].to.name, "callee_of_target",
        "canned callee must carry the fixed name"
    );

    let recorded = fake.outgoing_calls.lock().expect("mutex poisoned").clone();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].name, target.name);
    assert_eq!(recorded[0].uri, target_uri);
}

#[cfg(test)]
#[tokio::test]
async fn test_type_hierarchy_supertypes_via_serena() {
    use crate::bridge::LspDiagnosticsBridge;
    use fake_serena_client::{fake_type_hierarchy_item, FakeSerenaClient};

    let fake = Arc::new(FakeSerenaClient::new());
    let bridge = LspDiagnosticsBridge::with_serena_client(
        fake.clone() as Arc<dyn SerenaClient + Send + Sync>
    );
    let client = bridge
        .diagnostics_client()
        .expect("diagnostics_client must be Some when bridge carries a SerenaClient");

    let target_uri = Url::parse("file:///fixture/model.py").expect("url must parse");
    let target = fake_type_hierarchy_item("SubType", target_uri.clone());
    let supers = client
        .type_hierarchy_supertypes(target.clone())
        .await
        .expect("type_hierarchy_supertypes dispatch must succeed");

    assert_eq!(supers.len(), 1, "FakeSerenaClient returns one supertype");
    assert_eq!(
        supers[0].name, "SuperType",
        "canned supertype must carry the fixed name"
    );

    let recorded = fake
        .supertypes_calls
        .lock()
        .expect("mutex poisoned")
        .clone();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].name, target.name);
    assert_eq!(recorded[0].uri, target_uri);
}

#[cfg(test)]
#[tokio::test]
async fn test_no_lsp_configured_returns_error() {
    use crate::bridge::{BridgeError, LspDiagnosticsBridge};
    use crate::types::Language;

    // Degraded-mode bridge: constructed without a SerenaClient, no
    // endpoints registered (F07 has not yet populated them).
    let bridge = LspDiagnosticsBridge::new(false);
    assert!(
        !bridge.is_serena_managed(),
        "bridge constructed via new(false) must report serena_managed=false"
    );
    assert!(
        bridge.diagnostics_client().is_none(),
        "diagnostics_client must be None when bridge has no SerenaClient"
    );

    // The degraded-mode lookup must surface the typed error variant.
    let err = bridge
        .require_endpoint(Language::Rust)
        .expect_err("require_endpoint must fail in degraded mode with empty endpoint map");
    match err {
        BridgeError::NoLspServerConfigured { language } => {
            assert_eq!(
                language,
                Language::Rust,
                "error must carry the queried Language"
            );
        }
        other => {
            panic!("expected BridgeError::NoLspServerConfigured, got {other:?}")
        }
    }
}
