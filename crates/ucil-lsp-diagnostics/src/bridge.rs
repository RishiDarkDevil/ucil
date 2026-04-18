//! The LSP diagnostics bridge skeleton (`P1-W5-F03`, `WO-0014`).
//!
//! Per master-plan §13.3, UCIL owns a thin bridge that either delegates
//! LSP responsibilities to a Serena plugin (when Serena is registered
//! and ACTIVE) or spawns its own language servers (degraded mode).  This
//! module introduces the [`LspDiagnosticsBridge`] struct and its
//! accessor surface.
//!
//! At `P1-W5-F03` the bridge carries only a single boolean
//! (`serena_managed`) plus two empty maps — the endpoint map is
//! populated by `P1-W5-F07`, the diagnostics cache by `P1-W5-F04`.
//! Per `DEC-0008` the bridge takes a plain `bool` rather than a
//! reference to `ucil-daemon`'s `PluginManager` so that
//! `ucil-lsp-diagnostics` stays cycle-free.  The daemon integration
//! (reading `PluginManager::registered_runtimes()` and passing the
//! resulting bool) is reserved for a future progressive-startup WO.

// `BridgeError` deliberately repeats the `bridge` module name — the
// convention in this workspace is `<module>Error` (see
// `KnowledgeGraphError` in `ucil-core::knowledge_graph`).  Allowing
// the lint at module scope keeps the naming consistent without
// per-item `#[allow]` spam.
#![allow(clippy::module_name_repetitions)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use thiserror::Error;

use crate::diagnostics::{DiagnosticsClient, SerenaClient};
use crate::server_sharing::FallbackSpawner;
use crate::types::{Diagnostic, Language, LspEndpoint};

// ── Errors ───────────────────────────────────────────────────────────────────

/// Errors produced by [`LspDiagnosticsBridge`] operations.
///
/// Marked `#[non_exhaustive]` so downstream crates cannot rely on
/// exhaustive matching — `P1-W5-F04` and `P1-W5-F07` will extend this
/// enum with JSON-RPC transport failures and subprocess spawn
/// failures respectively, and that growth must not constitute a
/// `SemVer` break.
///
/// `P1-W5-F03` does not return `Err(BridgeError)` from any accessor —
/// the enum exists to anchor the crate-wide error surface and is
/// already exercised by downstream features.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum BridgeError {
    /// An [`LspEndpoint`] insert collided with an already-registered
    /// entry for the same [`Language`].  Reserved for `P1-W5-F07`'s
    /// degraded-mode spawner, which must not double-register a
    /// language server.  The skeleton's
    /// [`LspDiagnosticsBridge::insert_endpoint`] API returns the
    /// prior endpoint rather than surfacing this error — it is kept
    /// here as the forward-looking variant so the error enum has at
    /// least one variant at `P1-W5-F03` time.
    #[error("duplicate endpoint for language {language:?}")]
    DuplicateEndpoint {
        /// The [`Language`] whose endpoint was already registered.
        language: Language,
    },
    /// The bridge was queried for an LSP endpoint in the degraded-mode
    /// branch (Serena absent) but the endpoint map does not yet hold
    /// an entry for the requested [`Language`].
    ///
    /// `P1-W5-F04` surfaces this variant from
    /// [`LspDiagnosticsBridge::require_endpoint`] — the endpoint map
    /// is populated exclusively by `P1-W5-F07`, so until that feature
    /// lands every degraded-mode lookup returns this error.  The
    /// variant is distinct from [`Self::DuplicateEndpoint`] so callers
    /// (quality-issue feed `F05`, architecture feed `F06`) can fall
    /// back to Serena-delegation or surface a clear "no LSP server
    /// configured" diagnostic to the user.
    #[error("no LSP server configured for language {language:?}")]
    NoLspServerConfigured {
        /// The [`Language`] for which no endpoint has been registered.
        language: Language,
    },
}

// ── Bridge struct ────────────────────────────────────────────────────────────

/// The UCIL LSP diagnostics bridge.
///
/// Tracks whether Serena is managing LSP subprocesses for this daemon
/// and carries — in future Phase-1 features — an endpoint map and a
/// per-file diagnostics cache.  At `P1-W5-F03` both maps are empty in
/// every code path; see the module-level docs for the WO's scope
/// boundary.
///
/// # Invariants
///
/// * While `self.is_serena_managed()` returns `true`, `self.endpoints()`
///   is empty and remains empty — UCIL never spawns its own LSP
///   subprocesses in this branch.  `P1-W5-F04` will plug a
///   `SerenaClient` trait into the bridge for request dispatch; that
///   path does not populate the endpoint map.
/// * While `serena_managed` is `false`, the endpoint map is populated
///   exclusively by `P1-W5-F07` through
///   [`LspDiagnosticsBridge::insert_endpoint`].  `P1-W5-F03` itself
///   populates nothing — the map remains empty after construction.
pub struct LspDiagnosticsBridge {
    /// `true` when the daemon's `PluginManager` reports a Serena
    /// runtime in `PluginState::Active`.  Computed by the future
    /// daemon-integration WO and passed through [`Self::new`] per
    /// `DEC-0008`.
    serena_managed: bool,
    /// Map of language → UCIL-owned LSP endpoint.
    ///
    /// `P1-W5-F03` leaves this empty in both `serena_managed`
    /// branches.  `P1-W5-F07` populates it with standalone-subprocess
    /// endpoints when Serena is absent.  `std::collections::HashMap`
    /// is used deliberately — master-plan §13.3 lists
    /// `DashMap<PathBuf, Vec<Diagnostic>>` for the cache but the
    /// skeleton has no concurrent-access requirement; promoting to
    /// `DashMap` is explicitly `P1-W5-F04`'s call once real
    /// concurrent access lands.
    endpoints: HashMap<Language, LspEndpoint>,
    /// Per-file diagnostics cache keyed by absolute path.
    ///
    /// `P1-W5-F03` leaves this empty; `P1-W5-F04` populates it from
    /// `textDocument/diagnostic` LSP responses.  Standard
    /// `HashMap` for the same reason as [`Self::endpoints`].
    diagnostics_cache: HashMap<PathBuf, Vec<Diagnostic>>,
    /// Optional reference to an MCP-backed Serena client.
    ///
    /// Populated via [`Self::with_serena_client`] at `P1-W5-F04`;
    /// [`Self::new`] leaves this as `None`.  When `Some`, the
    /// [`Self::diagnostics_client`] accessor surfaces a typed
    /// [`DiagnosticsClient`] that callers use to dispatch
    /// `textDocument/diagnostic`, `callHierarchy/*`, and
    /// `typeHierarchy/supertypes` requests through Serena's MCP
    /// channel per `DEC-0008` §4.  When `None`, `diagnostics_client`
    /// returns `None` — callers must fall back to the degraded-mode
    /// path (populated by `P1-W5-F07`) or surface
    /// [`BridgeError::NoLspServerConfigured`].
    ///
    /// The trait object is wrapped in `Arc` (rather than `Box`) so
    /// each emitted [`DiagnosticsClient`] can clone-share ownership
    /// cheaply — future daemon-integration WOs may spawn multiple
    /// per-session `DiagnosticsClient`s from the same bridge.  The
    /// `Send + Sync` bounds are required for `tokio::spawn`.
    serena_client: Option<Arc<dyn SerenaClient + Send + Sync>>,
    /// Optional degraded-mode LSP-subprocess spawner.
    ///
    /// Populated via [`Self::with_fallback_spawner`] (the `P1-W5-F07`
    /// additive constructor per `DEC-0008` §Consequences); always
    /// `None` for bridges constructed via [`Self::new`] or
    /// [`Self::with_serena_client`].  Owning the spawner here ties
    /// the spawned subprocesses' lifecycle to the bridge — when the
    /// bridge is dropped, the spawner's [`Drop`] best-effort kills
    /// every surviving child via tokio's `kill_on_drop(true)`.
    fallback_spawner: Option<FallbackSpawner>,
}

impl LspDiagnosticsBridge {
    /// Construct a bridge in its skeleton state.
    ///
    /// `serena_managed` is load-bearing per `DEC-0008`: when `true`,
    /// UCIL delegates every LSP request to Serena via the MCP channel
    /// `PluginManager` already owns; when `false`, UCIL will spawn
    /// its own LSP subprocesses (once `P1-W5-F07` lands).  At
    /// `P1-W5-F03` both branches produce a bridge with empty endpoint
    /// and diagnostics maps.
    ///
    /// This single-parameter constructor is frozen for the duration
    /// of Phase 1.  `P1-W5-F04` may add *additional* constructors
    /// (e.g. `with_serena_client`) but must not break `new`.
    #[must_use]
    pub fn new(serena_managed: bool) -> Self {
        Self {
            serena_managed,
            endpoints: HashMap::new(),
            diagnostics_cache: HashMap::new(),
            serena_client: None,
            fallback_spawner: None,
        }
    }

    /// Construct a bridge pre-bound to a [`SerenaClient`] for the
    /// `serena_managed = true` branch of `DEC-0008` §4.
    ///
    /// Unlike [`Self::new`], this constructor sets
    /// [`Self::is_serena_managed`] to `true` and stores the provided
    /// client so [`Self::diagnostics_client`] can hand a typed
    /// [`DiagnosticsClient`] to callers.  The caller supplies the
    /// concrete `SerenaClient` implementation — in Phase 1 the
    /// daemon-integration WO will pass a `PluginManager`-backed
    /// forwarder; unit tests may pass a `FakeSerenaClient` that
    /// implements UCIL's own trait (not a mock of Serena's MCP wire
    /// format — those are two structurally distinct concerns per
    /// `DEC-0008`).
    ///
    /// This constructor is additive per `DEC-0008` §Consequences:
    /// [`Self::new`]'s `(serena_managed: bool) -> Self` signature
    /// stays byte-for-byte unchanged, so existing call sites are not
    /// perturbed.
    #[must_use]
    pub fn with_serena_client(serena_client: Arc<dyn SerenaClient + Send + Sync>) -> Self {
        Self {
            serena_managed: true,
            endpoints: HashMap::new(),
            diagnostics_cache: HashMap::new(),
            serena_client: Some(serena_client),
            fallback_spawner: None,
        }
    }

    /// Construct a degraded-mode bridge that owns a
    /// [`FallbackSpawner`] and pre-installs every spawned subprocess
    /// as a [`crate::types::LspTransport::Standalone`] endpoint.
    ///
    /// This is the additive `P1-W5-F07` constructor per `DEC-0008`
    /// §Consequences: [`Self::new`]'s `(serena_managed: bool) -> Self`
    /// signature stays byte-for-byte unchanged.  Returned bridge has
    /// `serena_managed = false` and no [`SerenaClient`] — the
    /// spawner's [`crate::server_sharing::FallbackSpawner::install_into`]
    /// has already been invoked so callers can immediately
    /// [`Self::endpoint_for`] any spawned language.
    ///
    /// The spawner is moved into the bridge so its lifecycle (and
    /// the lifecycle of every spawned LSP subprocess) is tied to the
    /// bridge — when the bridge is dropped, the spawner's
    /// best-effort [`Drop`] runs and tokio's `kill_on_drop(true)`
    /// reaps every surviving child.
    #[must_use]
    pub fn with_fallback_spawner(spawner: FallbackSpawner) -> Self {
        let mut bridge = Self::new(false);
        spawner.install_into(&mut bridge);
        bridge.fallback_spawner = Some(spawner);
        bridge
    }

    /// Borrow the optional [`FallbackSpawner`] owned by this bridge,
    /// or `None` if the bridge was not constructed via
    /// [`Self::with_fallback_spawner`].
    ///
    /// Exposed as a shared reference so callers can read spawner
    /// state (PIDs, last-used timestamps) without taking ownership.
    /// The spawner's mutating operations
    /// ([`crate::server_sharing::FallbackSpawner::touch`],
    /// [`crate::server_sharing::FallbackSpawner::shutdown_all`])
    /// require `&mut FallbackSpawner` — see
    /// [`Self::fallback_spawner_mut`] for the mutable accessor.
    #[must_use]
    pub const fn fallback_spawner(&self) -> Option<&FallbackSpawner> {
        self.fallback_spawner.as_ref()
    }

    /// Mutably borrow the optional [`FallbackSpawner`] owned by this
    /// bridge, or `None` if the bridge was not constructed via
    /// [`Self::with_fallback_spawner`].
    ///
    /// Required for `touch`, `shutdown_all`, and `is_alive`, which
    /// take `&mut FallbackSpawner`.
    pub const fn fallback_spawner_mut(&mut self) -> Option<&mut FallbackSpawner> {
        self.fallback_spawner.as_mut()
    }

    /// Returns `true` when this bridge was constructed with Serena in
    /// charge of LSP subprocesses; `false` otherwise (degraded mode).
    ///
    /// The flag is set once at construction and never mutates during
    /// the bridge's lifetime — a plugin state change would
    /// reconstruct the bridge via a future progressive-startup WO.
    #[must_use]
    pub const fn is_serena_managed(&self) -> bool {
        self.serena_managed
    }

    /// Look up the UCIL-owned endpoint for a language, if any.
    ///
    /// Returns `None` whenever the bridge has not registered an
    /// endpoint for `language`.  At `P1-W5-F03` this is the always-`None`
    /// path — the endpoint map is empty in both `serena_managed`
    /// branches.  Once `P1-W5-F07` lands, standalone endpoints will
    /// become available in the degraded-mode (`serena_managed = false`)
    /// branch.
    #[must_use]
    pub fn endpoint_for(&self, language: Language) -> Option<&LspEndpoint> {
        self.endpoints.get(&language)
    }

    /// Borrow the full language → endpoint map.
    ///
    /// Exposed as a reference — callers must not assume ordering and
    /// must not mutate the map.  `P1-W5-F07` mutates only through
    /// [`Self::insert_endpoint`].
    #[must_use]
    pub const fn endpoints(&self) -> &HashMap<Language, LspEndpoint> {
        &self.endpoints
    }

    /// Install a new UCIL-owned endpoint, returning the prior entry
    /// (if any) per `HashMap::insert` semantics.
    ///
    /// Reserved for `P1-W5-F07`'s degraded-mode spawner.  At
    /// `P1-W5-F03` this method has no production callers — it exists
    /// so the `endpoints` field is not dead code and so unit tests can
    /// prove the accessor round-trips without waiting for `F07`.
    pub fn insert_endpoint(&mut self, endpoint: LspEndpoint) -> Option<LspEndpoint> {
        self.endpoints.insert(endpoint.language, endpoint)
    }

    /// Borrow the file → diagnostics cache.
    ///
    /// Exposed as a reference so `P1-W5-F05` (quality-issue feed) can
    /// read without cloning.  `P1-W5-F03` always returns an empty
    /// map; `P1-W5-F04` populates it as `textDocument/diagnostic`
    /// responses arrive.
    #[must_use]
    pub const fn diagnostics_cache(&self) -> &HashMap<PathBuf, Vec<Diagnostic>> {
        &self.diagnostics_cache
    }

    /// Hand out a [`DiagnosticsClient`] when the bridge is bound to a
    /// Serena client (via [`Self::with_serena_client`]); returns
    /// `None` otherwise.
    ///
    /// `None` is returned in two legitimate cases:
    ///
    /// * The bridge was constructed through [`Self::new`] — no client
    ///   has been bound yet (Phase 1 daemon-integration WO will pass
    ///   one in via [`Self::with_serena_client`]).
    /// * The bridge was constructed in degraded mode
    ///   (`serena_managed = false`), in which case callers should
    ///   resolve LSP requests through the endpoint map populated by
    ///   `P1-W5-F07` — see [`Self::require_endpoint`].
    ///
    /// Each call clones the underlying `Arc`, so multiple callers
    /// can hold their own `DiagnosticsClient` without contention.
    #[must_use]
    pub fn diagnostics_client(&self) -> Option<DiagnosticsClient> {
        self.serena_client
            .as_ref()
            .map(|client| DiagnosticsClient::new(Arc::clone(client)))
    }

    /// Look up the degraded-mode LSP endpoint for `language`,
    /// returning [`BridgeError::NoLspServerConfigured`] when no
    /// endpoint has been registered.
    ///
    /// This is the typed counterpart of [`Self::endpoint_for`],
    /// intended for `P1-W5-F07`'s degraded-mode dispatch path.  At
    /// `P1-W5-F04` the endpoint map is still empty because `F07` has
    /// not yet populated it, so this method currently always returns
    /// the error when `is_serena_managed` is `false`.  Callers in the
    /// `serena_managed = true` branch should use
    /// [`Self::diagnostics_client`] instead; invoking this method on
    /// a Serena-managed bridge is legal but simply mirrors the
    /// empty-map behaviour (the degraded-mode map is empty by design
    /// when Serena is active per `DEC-0008` §3).
    ///
    /// # Errors
    ///
    /// Returns [`BridgeError::NoLspServerConfigured`] when the bridge
    /// has no endpoint registered for `language`.
    pub fn require_endpoint(&self, language: Language) -> Result<&LspEndpoint, BridgeError> {
        self.endpoints
            .get(&language)
            .ok_or(BridgeError::NoLspServerConfigured { language })
    }
}

// ── Module-root acceptance tests (F03 oracle) ────────────────────────────────
//
// The two tests below live at module root (NOT inside `mod tests { … }`) to
// honour the WO-0006/WO-0007/WO-0010/WO-0011/WO-0013 lesson — keeping them
// at the module root means a future planner who promotes either test to a
// frozen exact-match selector gets the path
// `bridge::test_bridge_with_serena_managed_has_no_own_endpoints` rather than
// `bridge::tests::…`.  The feature-list selector for `P1-W5-F03` is the
// module prefix `bridge::` so either placement would match today, but the
// project convention is module-root discipline for frozen-pattern-aligned
// tests.

/// Bridge with `serena_managed = true` has no UCIL-owned endpoints.
///
/// This is the `serena_managed` branch of `DEC-0008`: UCIL never
/// spawns LSP subprocesses while Serena is active, so the endpoint
/// map must be empty and every `endpoint_for` lookup must return
/// `None`.
#[cfg(test)]
#[test]
fn test_bridge_with_serena_managed_has_no_own_endpoints() {
    let bridge = LspDiagnosticsBridge::new(true);
    assert!(bridge.is_serena_managed());
    assert!(bridge.endpoints().is_empty());
    assert!(bridge.endpoint_for(Language::Python).is_none());
    assert!(bridge.endpoint_for(Language::Rust).is_none());
    assert!(bridge.endpoint_for(Language::TypeScript).is_none());
    assert!(bridge.diagnostics_cache().is_empty());
}

/// Bridge with `serena_managed = false` also has no endpoints at
/// `P1-W5-F03` — the degraded-mode branch does not spawn anything
/// until `P1-W5-F07` lands.
#[cfg(test)]
#[test]
fn test_bridge_without_serena_has_no_endpoints_until_f07() {
    let bridge = LspDiagnosticsBridge::new(false);
    assert!(!bridge.is_serena_managed());
    assert!(bridge.endpoints().is_empty());
    assert!(bridge.endpoint_for(Language::Rust).is_none());
    assert!(bridge.endpoint_for(Language::Python).is_none());
    assert!(bridge.diagnostics_cache().is_empty());
}

// ── Supporting tests (non-selector-frozen) ───────────────────────────────────

#[cfg(test)]
mod tests {
    use super::{BridgeError, LspDiagnosticsBridge};
    use crate::types::{Language, LspEndpoint, LspTransport};

    /// Round-trip `insert_endpoint` → `endpoint_for` → re-insert to
    /// prove the endpoint map wiring is real.
    ///
    /// This test is not selector-frozen in `feature-list.json`, so
    /// wrapping it in `mod tests { … }` (for shared helpers and
    /// clearer `super::` imports) is acceptable.
    #[test]
    fn test_insert_endpoint_round_trip() {
        let mut bridge = LspDiagnosticsBridge::new(false);
        let pyright = LspEndpoint {
            language: Language::Python,
            transport: LspTransport::Standalone {
                command: "pyright-langserver".into(),
                args: vec!["--stdio".into()],
            },
        };

        let prior = bridge.insert_endpoint(pyright.clone());
        assert!(
            prior.is_none(),
            "first insert must return None (no prior entry)"
        );
        assert_eq!(bridge.endpoints().len(), 1);
        assert_eq!(bridge.endpoint_for(Language::Python), Some(&pyright));
        assert!(bridge.endpoint_for(Language::Rust).is_none());

        // Re-insert a different endpoint for the same language — the
        // prior entry should be returned per HashMap::insert semantics.
        let ruff = LspEndpoint {
            language: Language::Python,
            transport: LspTransport::Standalone {
                command: "ruff-lsp".into(),
                args: vec![],
            },
        };
        let displaced = bridge.insert_endpoint(ruff.clone());
        assert_eq!(
            displaced.as_ref(),
            Some(&pyright),
            "re-insert must return the prior entry"
        );
        assert_eq!(bridge.endpoint_for(Language::Python), Some(&ruff));
    }

    /// Exercises the `BridgeError::DuplicateEndpoint` variant's
    /// `Debug` + `Display` surface.  The variant is not produced by
    /// the skeleton itself (`insert_endpoint` returns the prior entry
    /// rather than erroring), but the enum must still carry a usable
    /// variant to anchor the crate-wide error surface for
    /// `P1-W5-F04`/`F07`.
    #[test]
    fn test_bridge_error_duplicate_endpoint_render() {
        let err = BridgeError::DuplicateEndpoint {
            language: Language::Rust,
        };
        let rendered = format!("{err}");
        assert!(
            rendered.contains("duplicate endpoint"),
            "Display must mention the failure mode, got: {rendered}"
        );
        assert!(
            rendered.contains("Rust"),
            "Display must name the colliding language, got: {rendered}"
        );
        // Debug must not panic.
        let _ = format!("{err:?}");
    }
}
