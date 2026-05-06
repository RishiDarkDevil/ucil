//! `[vector_store]` config block parser — `P2-W8-F03` / `WO-0062`.
//!
//! Master-plan §17.6 lines 2026-2030 freeze the operator-facing
//! `[vector_store]` `TOML` section that scripts the embedding /
//! indexing layer:
//!
//! ```toml
//! [vector_store]
//! backend = "lancedb"
//! embedding_model = "coderankembed"
//! embedding_dimensions = 768
//! chunk_max_tokens = 512
//! reindex_on_startup = false
//! ```
//!
//! Master-plan §4.2 line 303 expands the `embedding_model` strategy:
//! `CodeRankEmbed` (137M params, MIT license, 8K context — CPU default)
//! is the boot-strap pick; `Qwen3-Embedding-8B` (Apache 2.0,
//! 80.68 `MTEB-Code`, 32K context, Matryoshka dimension support 32–7168)
//! is the GPU-only upgrade path.  Master-plan §18 Phase 2 Week 8
//! line 1787 frames the choice as "`CodeRankEmbed` (137M, CPU) as
//! default, Qwen3-Embedding (8B, GPU optional) as upgrade".
//!
//! This module declares:
//!
//! - [`EmbeddingBackend`] — typed enum mapping the master-plan-frozen
//!   string literals (`"coderankembed"`, `"qwen3-embedding"`) to typed
//!   variants;
//! - [`VectorStoreConfig`] — the `TOML`-deserialised struct mirroring
//!   the master-plan-frozen `[vector_store]` table;
//! - [`from_toml_str`] — the operator-facing parser that consumes a
//!   `&str` (the contents of `ucil.toml`) and returns the typed
//!   config, or [`ConfigError::Toml`] on parse failure;
//! - [`ConfigError`] — `thiserror` enum with operator-readable
//!   `Display` strings.
//!
//! The default values mirror the master-plan-frozen literals exactly,
//! so a missing `[vector_store]` table parses as the
//! `CodeRankEmbed`-default shape without error.  This matches the
//! master-plan-§4.2 "boot-strap pick" semantics: an operator who has
//! not edited `ucil.toml` gets the `CodeRankEmbed` CPU-only path.
//!
//! Consumer wiring (the daemon-side dispatcher that switches between
//! `CodeRankEmbed` and `Qwen3Embedding` based on the parsed
//! [`EmbeddingBackend`]) is deferred to `P2-W8-F04` (`LanceDB` chunk
//! indexer) and beyond.  This module ships the parser only — see
//! `WO-0062` `scope_out` for the deferred wiring.

use serde::Deserialize;

/// Errors emitted by the `[vector_store]` config parser.
///
/// `#[non_exhaustive]` per `.claude/rules/rust-style.md` so future
/// validation cases (e.g. dimension/model coherence checks once
/// `LanceDB` ships at `P2-W8-F04`) can be added without breaking
/// downstream `match` exhaustiveness.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ConfigError {
    /// Wraps a `toml::de::Error` from the underlying parse.
    /// Triggered by malformed `TOML` (unterminated tables, syntax
    /// errors, missing `=` separators).
    #[error("toml parse error: {source}")]
    Toml {
        /// The underlying `toml::de::Error`.
        #[from]
        source: toml::de::Error,
    },

    /// The `embedding_model` string literal does not match any of
    /// the master-plan-frozen options (`"coderankembed"`,
    /// `"qwen3-embedding"`).
    #[error("unknown embedding model: {name}")]
    UnknownEmbeddingModel {
        /// The string the operator supplied.
        name: String,
    },

    /// The `backend` string literal does not match any supported
    /// backend (currently only `"lancedb"`).
    #[error("unknown vector-store backend: {name}")]
    UnknownBackend {
        /// The string the operator supplied.
        name: String,
    },

    /// Aggregate validation failure — e.g. dimensions out of range
    /// when paired with a model that doesn't support that shape.
    #[error("vector-store config validation error: {reason}")]
    Validation {
        /// Operator-readable description of the failure.
        reason: String,
    },
}

/// Typed enum for the master-plan-frozen `embedding_model` literals.
///
/// `serde::Deserialize` is derived with `#[serde(rename_all = "kebab-case")]`
/// so the master-plan-frozen literals (`"coderankembed"`,
/// `"qwen3-embedding"`) parse to the corresponding variant when used
/// as a typed field rather than a free-form string.  The
/// [`EmbeddingBackend::from_config_str`] constructor exposes the same
/// mapping for callers who already have a `&str`.
///
/// Master-plan §4.2 line 303 fixes the two supported models;
/// adding a third requires an `ADR` and a master-plan amendment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EmbeddingBackend {
    /// The default CPU embedding model — `CodeRankEmbed` (137M params,
    /// MIT license, 8K context, ~137MB Int8-quantised, 50-150 emb/sec
    /// on CPU).  Master-plan §18 Phase 2 Week 8 line 1787.
    #[serde(rename = "coderankembed")]
    CodeRankEmbed,

    /// The GPU-only upgrade model — `Qwen3-Embedding-8B` (Apache 2.0,
    /// 80.68 `MTEB-Code`, 32K context, Matryoshka dimension support
    /// 32-7168).  Master-plan §4.2 line 303 + §18 Phase 2 Week 8
    /// line 1787.  Requires a GPU `ort` execution-provider; on the
    /// current workspace `ort` build (`default-features = false`),
    /// loading this backend produces
    /// [`crate::Qwen3EmbeddingError::NoGpuDetected`].
    Qwen3,
}

impl EmbeddingBackend {
    /// Map a master-plan-frozen `embedding_model` string to the typed
    /// variant.
    ///
    /// Recognises the two master-plan-frozen literals exactly:
    /// `"coderankembed"` → [`EmbeddingBackend::CodeRankEmbed`],
    /// `"qwen3-embedding"` → [`EmbeddingBackend::Qwen3`].  Any other
    /// input returns [`ConfigError::UnknownEmbeddingModel`] with the
    /// supplied string preserved for operator-friendly diagnostics.
    ///
    /// # Errors
    ///
    /// - [`ConfigError::UnknownEmbeddingModel`] if `s` is not one of
    ///   the master-plan-frozen literals.
    pub fn from_config_str(s: &str) -> Result<Self, ConfigError> {
        match s {
            "coderankembed" => Ok(Self::CodeRankEmbed),
            "qwen3-embedding" => Ok(Self::Qwen3),
            _ => Err(ConfigError::UnknownEmbeddingModel { name: s.to_owned() }),
        }
    }
}

fn default_backend() -> String {
    "lancedb".to_owned()
}

fn default_embedding_model() -> String {
    "coderankembed".to_owned()
}

const fn default_embedding_dimensions() -> usize {
    768
}

const fn default_chunk_max_tokens() -> usize {
    512
}

const fn default_reindex_on_startup() -> bool {
    false
}

/// The `[vector_store]` config block (master-plan §17.6 lines 2026-2030).
///
/// Each field has a `#[serde(default = "...")]` helper returning the
/// master-plan-frozen literal so a missing `[vector_store]` table — or
/// a partial table that omits some fields — still produces a complete
/// struct without parse errors.  This matches the master-plan §4.2
/// "boot-strap pick" semantics: an operator who has not edited
/// `ucil.toml` gets the `CodeRankEmbed` default path.
///
/// `Default::default()` returns the same all-defaults shape via the
/// helper functions, so callers needing a programmatic default
/// (without going through `TOML`) can use [`VectorStoreConfig::default`].
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct VectorStoreConfig {
    /// The vector-store backend.  Currently only `"lancedb"` is
    /// supported; future backends would be additive
    /// (`"qdrant"` / `"pgvector"`) per master-plan §13 line 1326.
    #[serde(default = "default_backend")]
    pub backend: String,

    /// The embedding model name.  Master-plan-frozen literals are
    /// `"coderankembed"` (default) and `"qwen3-embedding"` (GPU
    /// upgrade); any other value produces
    /// [`ConfigError::UnknownEmbeddingModel`] when validated via
    /// [`EmbeddingBackend::from_config_str`].
    #[serde(default = "default_embedding_model")]
    pub embedding_model: String,

    /// The embedding dimension.  Master-plan §17.6 line 2029 fixes
    /// `768` for `CodeRankEmbed`; the operator may override to a
    /// Matryoshka-supported dimension (32-7168) when paired with
    /// `embedding_model = "qwen3-embedding"` per master-plan §4.2
    /// line 303.
    #[serde(default = "default_embedding_dimensions")]
    pub embedding_dimensions: usize,

    /// The maximum tokens per chunk produced by the chunker
    /// (`P2-W8-F05` / `WO-0061`).  Master-plan §17.6 line 2030
    /// fixes the default at `512`; lowering this trades recall for
    /// indexing throughput, raising it requires a model with longer
    /// context.
    #[serde(default = "default_chunk_max_tokens")]
    pub chunk_max_tokens: usize,

    /// If `true`, the daemon re-indexes the entire repo on startup
    /// (otherwise it relies on incremental updates from the file
    /// watcher).  Master-plan §17.6 line 2030 fixes the default at
    /// `false`.
    #[serde(default = "default_reindex_on_startup")]
    pub reindex_on_startup: bool,
}

impl Default for VectorStoreConfig {
    fn default() -> Self {
        Self {
            backend: default_backend(),
            embedding_model: default_embedding_model(),
            embedding_dimensions: default_embedding_dimensions(),
            chunk_max_tokens: default_chunk_max_tokens(),
            reindex_on_startup: default_reindex_on_startup(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct VectorStoreConfigDoc {
    #[serde(default)]
    vector_store: VectorStoreConfig,
}

/// Parse a `ucil.toml` payload and extract the `[vector_store]` block.
///
/// On absent `[vector_store]` table (empty `TOML` or a `TOML`
/// document that omits the section entirely), `serde::Deserialize` on
/// the wrapping document returns the all-defaults
/// [`VectorStoreConfig`] via the `#[serde(default)]` annotation —
/// this matches the master-plan-frozen "`coderankembed` default"
/// semantics: no config means `CodeRankEmbed`, NOT an error.
///
/// # Errors
///
/// - [`ConfigError::Toml`] on `TOML` parse failure (malformed input,
///   unterminated tables, type mismatches against the
///   [`VectorStoreConfig`] shape).
///
/// # Examples
///
/// ```
/// use ucil_embeddings::VectorStoreConfig;
///
/// let cfg = ucil_embeddings::config::from_toml_str("").expect("empty parses");
/// assert_eq!(cfg, VectorStoreConfig::default());
/// ```
pub fn from_toml_str(toml_str: &str) -> Result<VectorStoreConfig, ConfigError> {
    let doc: VectorStoreConfigDoc = toml::from_str(toml_str)?;
    Ok(doc.vector_store)
}

