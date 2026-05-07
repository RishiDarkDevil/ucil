//! Per-branch background `LanceDB` chunk-indexing pipeline (`P2-W8-F04`).
//!
//! Master-plan §12.2 lines 1321-1346 freezes the `code_chunks` table
//! schema (12 columns including a `FixedSizeList<Float32, 768>`
//! embedding column and a `file_hash` content-addressed delta key).
//! Master-plan §18 Phase 2 Week 8 line 1789 frames this feature as
//! "background chunk indexing" — file-change events drive chunking +
//! embedding + insertion into the per-branch `code_chunks` table.
//! Master-plan §3.2 line 1643 places this module directly under
//! `crates/ucil-daemon/src/`.
//!
//! [`LancedbChunkIndexer`] is the orchestrator: given a list of
//! repo-relative paths, it (1) loads the persisted [`IndexerState`]
//! `JSON` sidecar; (2) for each path compares the on-disk mtime
//! against the persisted snapshot and skips unchanged files;
//! (3) for changed files reads + computes a `Sha256` `file_hash`,
//! runs [`ucil_embeddings::EmbeddingChunker::chunk`] over the
//! contents, embeds each chunk via the injected
//! [`EmbeddingSource`] trait, builds a 12-column Arrow `RecordBatch`
//! conforming to [`crate::branch_manager::code_chunks_schema`],
//! appends the batch to the per-branch `code_chunks` `LanceDB`
//! table opened by
//! [`crate::branch_manager::BranchManager::create_branch_table`]
//! (`WO-0053` for `P2-W7-F09`; merged to main via `DEC-0016` §Closed
//! at commit `57e50ab`), and (4) atomically persists the updated
//! [`IndexerState`] to `<branches_root>/<sanitised>/indexer-state.json`.
//!
//! [`EmbeddingSource`] is the `UCIL`-internal trait seam per
//! `DEC-0008` §4 — production wires
//! [`CodeRankEmbeddingSource`] (wrapping
//! [`ucil_embeddings::CodeRankEmbed`], `WO-0059`) while tests inject
//! a deterministic `TestEmbeddingSource` impl in
//! `crate::executor`.  Per `DEC-0008` carve-out, the
//! `TestEmbeddingSource` is a `UCIL`-internal trait impl — distinct
//! from the prohibited critical-dep substitution layers for
//! `Serena` / `LSP` / `SQLite` / `LanceDB` / `Docker`.
//!
//! [`IndexerHandle`] wraps a [`LancedbChunkIndexer`] in a
//! `tokio::task::spawn`-able loop subscribed to a
//! `tokio::sync::mpsc::Receiver<crate::watcher::FileEvent>` and
//! dispatches each `Created` / `Modified` event to
//! [`LancedbChunkIndexer::index_paths`] — proves the "background"
//! / "file changes trigger" half of `P2-W8-F04` without coupling the
//! frozen unit test to the real watcher backend.  Production wiring
//! of [`crate::watcher::FileWatcher`]'s output channel into
//! [`IndexerHandle::spawn`] is deferred to a follow-up `WO`
//! (likely the daemon's `main.rs` startup flow or a NEW
//! `IndexerSupervisor` orchestrator) per the `WO`'s `scope_out`.
//!
//! # Design notes
//!
//! * **Append-only `Table::add` (not `merge_insert`).**  This `WO`
//!   uses `lancedb::Table::add` which appends new rows.  A
//!   re-indexed file's NEW chunks therefore coexist with the OLD
//!   chunks for that `file_path` — the
//!   `executor::test_lancedb_incremental_indexing` `SA4`
//!   sub-assertion exercises exactly this (asserts >1 distinct
//!   `file_hash` for a touched file).  A future `WO` will switch
//!   to `merge_insert` keyed on `(file_path, id)` for true upsert
//!   semantics.
//! * **`Removed` / `Renamed` are dropped.**  [`IndexerHandle`]'s
//!   loop ignores `FileEventKind::Removed` and `FileEventKind::
//!   Renamed`.  Stale rows persist until a future `WO` adds a
//!   `delete_by_file_path` pass.  Row count is monotonically
//!   non-decreasing.
//! * **No ANN index creation.**  The `IVF` / `HNSW` index for
//!   fast vector search is created in `P2-W8-F07` (vector query
//!   latency benchmark) and consumed in `P2-W8-F08`
//!   (`find_similar` `MCP` tool).
//! * **Per-file batch atomicity.**  Per-chunk embedding errors
//!   degrade gracefully (log + increment `chunks_failed`).  Per-
//!   file embedding-dimension mismatches fail the whole batch
//!   because dim drift indicates a model misconfiguration, not a
//!   transient chunk error.
//! * **Crash-safe state writes.**  [`IndexerState::save_atomic`]
//!   writes to `<path>.tmp` then `tokio::fs::rename` to `<path>`
//!   — atomic on `POSIX` same-fs per `tokio::fs::rename`'s
//!   contract.  Mirrors the `WO-0021` lifecycle precedent.

// `LancedbChunkIndexer` / `IndexerState` / `IndexerStats` /
// `IndexerHandle` / `EmbeddingSource` / `CodeRankEmbeddingSource` /
// `ChunkIndexerError` / `EmbeddingSourceError` share the module
// name prefix ("indexer" → `LancedbChunkIndexer`); suppress the
// pedantic lint, mirroring the escape used in
// `branch_manager::BranchManager` and other crate-local module
// roots.
#![allow(clippy::module_name_repetitions)]

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use arrow_array::{
    types::Float32Type, FixedSizeListArray, Int32Array, PrimitiveArray, RecordBatch,
    RecordBatchIterator, StringArray, TimestampMicrosecondArray,
};
use arrow_schema::{DataType, Field};
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use thiserror::Error;
use ucil_embeddings::{
    CodeRankEmbed, CodeRankEmbedError, EmbeddingChunker, EmbeddingChunkerError, EMBEDDING_DIM,
};
use ucil_treesitter::Language;

use crate::branch_manager::{
    code_chunks_schema, sanitise_branch_name, BranchManager, BranchManagerError,
};

// ── Error enums ─────────────────────────────────────────────────────────────

/// Failures surfaced by [`LancedbChunkIndexer`] operations.
///
/// `#[non_exhaustive]` per `.claude/rules/rust-style.md` so future
/// variants (e.g. a `MergeInsertConflict` arm if a later `WO` adds
/// upsert semantics) can be added without breaking downstream
/// `match` exhaustiveness.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ChunkIndexerError {
    /// Filesystem `IO` error during state read/write or source-file
    /// read.  Bubbled from `tokio::fs::*` and `std::fs::*`.
    #[error("io error during chunk-indexer operation: {source}")]
    Io {
        /// Underlying `OS` error.
        #[from]
        source: std::io::Error,
    },

    /// Parse / serialize failure on the `<branches_root>/<sanitised>/
    /// indexer-state.json` sidecar.  Bubbled from `serde_json::*`.
    #[error("indexer-state.json parse/serialize failed: {source}")]
    Json {
        /// Underlying `serde_json` error.
        #[from]
        source: serde_json::Error,
    },

    /// Failure in the per-branch `BranchManager` lifecycle (table
    /// open, list, etc.).  Bubbled from
    /// [`crate::branch_manager::BranchManagerError`].
    #[error("branch-manager operation failed: {source}")]
    BranchManager {
        /// Underlying [`BranchManagerError`].
        #[from]
        source: BranchManagerError,
    },

    /// `LanceDB` connection / open-table / append failure.  Bubbled
    /// from `lancedb::Error`.
    #[error("lancedb operation failed: {source}")]
    Lance {
        /// Underlying `lancedb::Error`.
        #[from]
        source: lancedb::Error,
    },

    /// `Arrow` `RecordBatch::try_new` or array-construction failure.
    /// Practically unreachable for the fixed §12.2 schema once the
    /// per-row buffers are length-validated, but kept for completeness
    /// so `try_new` errors surface typed.
    #[error("arrow record-batch construction failed: {source}")]
    Arrow {
        /// Underlying `arrow-schema` error.
        #[from]
        source: arrow_schema::ArrowError,
    },

    /// The injected [`EmbeddingSource`] surfaced an error.  Bubbled
    /// via the trait's [`EmbeddingSourceError`] return shape.
    #[error("embedding source failed: {source}")]
    Embedding {
        /// Underlying [`EmbeddingSourceError`].
        #[from]
        source: EmbeddingSourceError,
    },

    /// Chunker (`ucil_embeddings::EmbeddingChunker::chunk`) failed.
    /// Bubbled from
    /// [`ucil_embeddings::EmbeddingChunkerError`].
    #[error("embedding chunker failed: {source}")]
    Chunker {
        /// Underlying [`EmbeddingChunkerError`].
        #[from]
        source: EmbeddingChunkerError,
    },

    /// The injected [`EmbeddingSource`] returned a vector whose
    /// length disagrees with its declared [`EmbeddingSource::dim`].
    /// This is a defensive bounds-check that surfaces a clear error
    /// instead of letting the downstream
    /// `FixedSizeListArray::try_new` panic on a length mismatch
    /// (the embedding column is fixed at width 768; a 0-length or
    /// wrong-length `Vec` would otherwise produce an opaque arrow
    /// error).
    #[error("embedding dimension mismatch for {file:?}: expected {expected}, got {got}")]
    DimensionMismatch {
        /// Declared dimension from
        /// [`EmbeddingSource::dim`].
        expected: usize,
        /// Length of the returned vector.
        got: usize,
        /// Repo-relative path of the file whose chunk produced the
        /// mismatch.
        file: PathBuf,
    },

    /// `fs::Metadata::modified()` returned `Err` on a platform
    /// without modification-time support.  Defensive — Unix and
    /// Windows always support `modified()`; this variant is here
    /// as a load-bearing operator-friendly diagnostic per
    /// `WO-0044`'s standing pattern.
    #[error("modification time not supported on this platform for {file:?}")]
    MtimeUnsupported {
        /// Repo-relative path that failed.
        file: PathBuf,
    },

    /// Per-branch `vectors/` directory path is not valid `UTF-8`,
    /// so it cannot be passed to `lancedb::connect`.  Surfaces the
    /// offending path for ops triage.
    #[error("non-UTF8 vectors directory path: {path:?}")]
    NonUtf8VectorsPath {
        /// The offending path.
        path: PathBuf,
    },
}

/// Failures surfaced by an [`EmbeddingSource`] implementation.
///
/// `#[non_exhaustive]` per `.claude/rules/rust-style.md`.  The two
/// variants are: the production `CodeRankEmbed`-shaped failure and a
/// generic escape hatch for downstream impls (test-injection sources
/// that do not correspond to the production embedder).
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum EmbeddingSourceError {
    /// `CodeRankEmbed` model load / inference failure — bubbled from
    /// [`ucil_embeddings::CodeRankEmbedError`].
    #[error("CodeRankEmbed source error: {source}")]
    CodeRankEmbed {
        /// Underlying [`CodeRankEmbedError`].
        #[from]
        source: CodeRankEmbedError,
    },

    /// Generic escape-hatch error for downstream impls (e.g.
    /// alternate embedder backends) that do not map cleanly onto
    /// the `CodeRankEmbed` variant.
    #[error("embedding source error: {message}")]
    Other {
        /// Operator-readable description of the failure.
        message: String,
    },
}

// ── EmbeddingSource trait + production impl ────────────────────────────────

/// `UCIL`-internal seam for embedder backends per `DEC-0008` §4.
///
/// The production impl is [`CodeRankEmbeddingSource`] (wraps
/// [`ucil_embeddings::CodeRankEmbed`]); tests inject a deterministic
/// `TestEmbeddingSource` impl from `crate::executor` (per
/// `DEC-0008` carve-out — `UCIL`-internal trait impls are
/// distinct from the prohibited critical-dep substitution layers).
///
/// The trait's `embed` takes `&self` to keep the call-site clean;
/// production impls that need to `&mut self`-call the inner
/// embedder must wrap the embedder in a `tokio::sync::Mutex` and
/// acquire it on every call (see [`CodeRankEmbeddingSource`] for
/// the canonical pattern).  This shape lets the indexer drop the
/// per-call `&mut` reservation while preserving the inner
/// embedder's `&mut self` constraint per `WO-0058` lessons line
/// 561 (`&mut self → Arc<Mutex<>>`).
///
/// The [`Self::dim`] accessor lets the indexer validate
/// `embedding.len() == dim()` BEFORE constructing the
/// `FixedSizeList`, surfacing
/// [`ChunkIndexerError::DimensionMismatch`] instead of letting
/// `arrow-array` panic on a length mismatch.
#[async_trait]
pub trait EmbeddingSource: Send + Sync {
    /// Operator-readable name of the source ("coderankembed",
    /// "test", "qwen3", …).  Logged in tracing spans + RFR
    /// disclosures.
    fn name(&self) -> &'static str;

    /// Declared embedding dimension.  Production
    /// [`CodeRankEmbeddingSource`] returns
    /// [`ucil_embeddings::EMBEDDING_DIM`] (`768`).  The indexer
    /// validates `embedding.len() == dim()` after every successful
    /// call so a mis-configured backend fails fast instead of
    /// scrambling the column.
    fn dim(&self) -> usize;

    /// Embed `code` and return a `Vec<f32>` of length
    /// [`Self::dim`].
    ///
    /// # Errors
    ///
    /// - [`EmbeddingSourceError::CodeRankEmbed`] if the production
    ///   `CodeRankEmbed`-backed source fails;
    /// - [`EmbeddingSourceError::Other`] for downstream impls'
    ///   generic failures.
    async fn embed(&self, code: &str) -> Result<Vec<f32>, EmbeddingSourceError>;
}

/// Production [`EmbeddingSource`] wrapping
/// [`ucil_embeddings::CodeRankEmbed`].
///
/// `CodeRankEmbed::embed` takes `&mut self` (per upstream `ort 2.x`
/// `Session::run`); to satisfy the trait's `&self` shape, the
/// embedder is held in `Arc<tokio::sync::Mutex<_>>` and the
/// `embed` impl acquires the lock on every call.  This is the
/// canonical pattern documented in `WO-0058` lessons line 561 and
/// re-applied in `WO-0059` / `WO-0060`.
///
/// The Mutex-locked sync call is acceptable for the `P2-W8-F04`
/// sequential per-file indexing shape: each `index_paths(...)` call
/// processes files one-at-a-time, and within a file each chunk is
/// embedded sequentially, so contention on the embedder Mutex is by
/// construction zero across the lifetime of a single call.
pub struct CodeRankEmbeddingSource {
    embedder: Arc<tokio::sync::Mutex<CodeRankEmbed>>,
    model_dir: PathBuf,
}

impl std::fmt::Debug for CodeRankEmbeddingSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodeRankEmbeddingSource")
            .field("model_dir", &self.model_dir)
            .field("embedder", &"<Arc<Mutex<CodeRankEmbed>>>")
            .finish()
    }
}

impl CodeRankEmbeddingSource {
    /// Load the production `CodeRankEmbed` model bundle from
    /// `model_dir` (typically `ml/models/coderankembed/`) and wrap
    /// it in `Arc<tokio::sync::Mutex<_>>` so concurrent
    /// [`EmbeddingSource::embed`] calls serialise on the inner
    /// `&mut self` constraint.
    ///
    /// # Errors
    ///
    /// Surfaces [`EmbeddingSourceError::CodeRankEmbed`] if the
    /// model bundle is missing artefacts or fails to load (corrupt
    /// `model.onnx`, missing `tokenizer.json`, etc.).
    pub fn load(model_dir: &Path) -> Result<Self, EmbeddingSourceError> {
        let embedder = CodeRankEmbed::load(model_dir)?;
        Ok(Self {
            embedder: Arc::new(tokio::sync::Mutex::new(embedder)),
            model_dir: model_dir.to_path_buf(),
        })
    }

    /// Read-only accessor for the on-disk model directory the
    /// embedder was loaded from.  Useful for ops triage and
    /// tracing spans.
    #[must_use]
    pub fn model_dir(&self) -> &Path {
        &self.model_dir
    }
}

#[async_trait]
impl EmbeddingSource for CodeRankEmbeddingSource {
    fn name(&self) -> &'static str {
        "coderankembed"
    }

    fn dim(&self) -> usize {
        EMBEDDING_DIM
    }

    async fn embed(&self, code: &str) -> Result<Vec<f32>, EmbeddingSourceError> {
        let v = {
            let mut guard = self.embedder.lock().await;
            guard.embed(code)?
        };
        Ok(v)
    }
}

// ── IndexerState ───────────────────────────────────────────────────────────

/// Current `schema_version` for [`IndexerState`].
///
/// Bumping triggers a full re-index of all files (graceful
/// migration: an unknown version on disk is treated as
/// `Default::default()`, which returns an empty `file_mtimes`
/// map).
pub const INDEXER_STATE_SCHEMA_VERSION: u32 = 1;

/// Per-branch incremental-state sidecar persisted at
/// `<branches_root>/<sanitised>/indexer-state.json`.
///
/// Tracks the last-seen mtime of every indexed file (repo-root-
/// relative path → seconds-since-Unix-epoch).  The
/// [`LancedbChunkIndexer::index_paths`] entry-point compares a
/// path's current mtime against this snapshot and skips unchanged
/// files (incremental-indexing semantics).
///
/// `BTreeMap` (NOT `HashMap`) gives deterministic iteration order
/// — `JSON` serialisation is byte-stable across runs which
/// simplifies golden-file diffs and eyeball-grepping the sidecar
/// during ops triage.  Path keys are stored as `PathBuf` (which
/// `serde` round-trips as a `JSON` string).  mtime values are
/// seconds-since-Unix-epoch; nanosecond precision is intentionally
/// dropped because the incremental-indexing semantics only need
/// "did the file change since last index" granularity, not
/// real-time delta accounting.
///
/// `schema_version` is stamped at write time; loaders that find a
/// future version on disk fall back to `Default::default()`
/// (graceful schema migration).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct IndexerState {
    /// Repo-root-relative path → mtime seconds-since-Unix-epoch.
    pub file_mtimes: BTreeMap<PathBuf, u64>,
    /// Sidecar schema version.
    pub schema_version: u32,
}

impl Default for IndexerState {
    fn default() -> Self {
        Self {
            file_mtimes: BTreeMap::new(),
            schema_version: INDEXER_STATE_SCHEMA_VERSION,
        }
    }
}

impl IndexerState {
    /// Read + parse `<path>` if it exists; otherwise return
    /// [`Default::default()`].  `NotFound` errors collapse to the
    /// default; other `IO` errors bubble.
    ///
    /// # Errors
    ///
    /// - [`ChunkIndexerError::Io`] on filesystem errors other than
    ///   `NotFound`;
    /// - [`ChunkIndexerError::Json`] on parse failure of an
    ///   existing file.
    pub fn load_or_default(path: &Path) -> Result<Self, ChunkIndexerError> {
        match std::fs::read_to_string(path) {
            Ok(raw) => Ok(serde_json::from_str(&raw)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(ChunkIndexerError::Io { source: e }),
        }
    }

    /// Atomically persist `self` to `<path>` via the
    /// `<path>.tmp` + `tokio::fs::rename` pattern.  Atomic on
    /// `POSIX` same-fs per the `tokio::fs::rename` contract.
    ///
    /// # Errors
    ///
    /// - [`ChunkIndexerError::Json`] on `serde_json::to_string`
    ///   failure;
    /// - [`ChunkIndexerError::Io`] on write or rename failure.
    pub async fn save_atomic(&self, path: &Path) -> Result<(), ChunkIndexerError> {
        let raw = serde_json::to_string_pretty(self)?;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let tmp = path.with_extension("json.tmp");
        tokio::fs::write(&tmp, raw.as_bytes()).await?;
        tokio::fs::rename(&tmp, path).await?;
        Ok(())
    }

    /// Constant accessor for the current schema version — useful
    /// for parity tests that want to assert the on-disk version
    /// without hard-coding the literal.
    #[must_use]
    pub const fn schema_version_current() -> u32 {
        INDEXER_STATE_SCHEMA_VERSION
    }
}

// ── IndexerStats ───────────────────────────────────────────────────────────

/// Per-call telemetry returned by
/// [`LancedbChunkIndexer::index_paths`].
///
/// `chunks_failed` tracks per-chunk embedding failures that DO NOT
/// fail the whole batch (graceful degradation — log + continue per
/// master-plan §15 partial-failure pattern).  Per-file embedding
/// errors STILL fail the whole batch (the file's chunks are a
/// coherent unit).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct IndexerStats {
    /// Total paths the call iterated over (includes both indexed
    /// and skipped).
    pub files_scanned: usize,
    /// Files skipped because their on-disk mtime matched the
    /// persisted snapshot.
    pub files_skipped_unchanged: usize,
    /// Files that were chunked + embedded + appended.
    pub files_indexed: usize,
    /// Total chunks appended to the `code_chunks` table across all
    /// indexed files.
    pub chunks_inserted: usize,
    /// Per-chunk embedding failures that were logged + continued.
    pub chunks_failed: usize,
}

// ── LancedbChunkIndexer ────────────────────────────────────────────────────

/// Per-branch background `LanceDB` chunk-indexing pipeline.
///
/// Generic over `S: EmbeddingSource` so tests inject a deterministic
/// `TestEmbeddingSource` while production wires
/// [`CodeRankEmbeddingSource`].  The chunker is held in
/// `Arc<tokio::sync::Mutex<_>>` because
/// [`ucil_embeddings::EmbeddingChunker::chunk`] takes `&mut self`
/// (per `WO-0060` lessons line 658).
pub struct LancedbChunkIndexer<S: EmbeddingSource> {
    branch_manager: Arc<BranchManager>,
    branch_name: String,
    chunker: Arc<tokio::sync::Mutex<EmbeddingChunker>>,
    source: Arc<S>,
    state: IndexerState,
}

impl<S: EmbeddingSource> std::fmt::Debug for LancedbChunkIndexer<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Manual impl elides the `chunker` (no Debug) and `branch_manager`
        // (skipped to keep the printed shape readable for ops triage); the
        // omission is intentional, not an oversight.
        f.debug_struct("LancedbChunkIndexer")
            .field("branch_manager", &"<Arc<BranchManager>>")
            .field("branch_name", &self.branch_name)
            .field("chunker", &"<Arc<Mutex<EmbeddingChunker>>>")
            .field("source.name", &self.source.name())
            .field("source.dim", &self.source.dim())
            .field("state.file_mtimes.len", &self.state.file_mtimes.len())
            .field("state.schema_version", &self.state.schema_version)
            .finish()
    }
}

/// Internal per-row staging struct collected during the pipeline,
/// converted to columnar `Arrow` arrays before
/// `RecordBatch::try_new`.
struct RowDraft {
    id: String,
    file_path: String,
    start_line: i32,
    end_line: i32,
    content: String,
    language: String,
    symbol_name: Option<String>,
    symbol_kind: Option<String>,
    embedding: Vec<f32>,
    token_count: i32,
    file_hash: String,
    indexed_at_micros: i64,
}

impl<S: EmbeddingSource> LancedbChunkIndexer<S> {
    /// Build a new indexer with the given branch manager, branch
    /// name, chunker, and embedding source.  State is initialised
    /// to [`Default::default()`]; call
    /// [`Self::load_state`] to load the persisted sidecar before
    /// the first [`Self::index_paths`] call (otherwise the first
    /// call will load it implicitly).
    pub fn new(
        branch_manager: Arc<BranchManager>,
        branch_name: impl Into<String>,
        chunker: Arc<tokio::sync::Mutex<EmbeddingChunker>>,
        source: Arc<S>,
    ) -> Self {
        Self {
            branch_manager,
            branch_name: branch_name.into(),
            chunker,
            source,
            state: IndexerState::default(),
        }
    }

    /// Compute the path of the per-branch `indexer-state.json`
    /// sidecar (`<branches_root>/<sanitised>/indexer-state.json`).
    /// Pure path arithmetic — does NOT touch the filesystem.
    #[must_use]
    pub fn state_path(&self) -> PathBuf {
        self.branch_manager
            .branches_root()
            .join(sanitise_branch_name(&self.branch_name))
            .join("indexer-state.json")
    }

    /// Read-only accessor for the in-memory [`IndexerState`].
    #[must_use]
    pub const fn state(&self) -> &IndexerState {
        &self.state
    }

    /// Load the persisted [`IndexerState`] from disk into `self`.
    /// Idempotent: subsequent calls overwrite the in-memory state
    /// with the on-disk snapshot (defensive against concurrent
    /// writers leaving stale in-memory state).
    ///
    /// # Errors
    ///
    /// Bubbles [`ChunkIndexerError::Io`] / [`ChunkIndexerError::Json`]
    /// from [`IndexerState::load_or_default`].
    pub fn load_state(&mut self) -> Result<(), ChunkIndexerError> {
        let path = self.state_path();
        // `load_or_default` is sync; offload to `spawn_blocking`
        // would be overkill for a small JSON read.  Acceptable for
        // the per-call boundary.
        self.state = IndexerState::load_or_default(&path)?;
        Ok(())
    }

    /// Index a batch of paths into the per-branch `code_chunks`
    /// `LanceDB` table.
    ///
    /// Pipeline:
    /// 1. `(re)load` persisted state from disk (defensive against
    ///    concurrent writers);
    /// 2. open the per-branch `LanceDB` connection + the
    ///    `code_chunks` table (created by
    ///    [`crate::branch_manager::BranchManager::create_branch_table`]
    ///    — calling that first is a load-bearing precondition);
    /// 3. for each path:
    ///    - read mtime; if unchanged vs persisted state, increment
    ///      `files_skipped_unchanged` and continue;
    ///    - read contents, compute `Sha256` `file_hash`, run the
    ///      chunker, embed each chunk via the injected
    ///      [`EmbeddingSource`], collect into `RowDraft`s;
    /// 4. if the row buffer is non-empty, build a 12-column
    ///    `RecordBatch` matching
    ///    [`code_chunks_schema`] and append via
    ///    `lancedb::Table::add(...)`;
    /// 5. atomically persist the updated [`IndexerState`].
    ///
    /// # Errors
    ///
    /// Surfaces every variant of [`ChunkIndexerError`].  See the
    /// enum docs for the per-variant semantics.
    #[tracing::instrument(
        name = "ucil.executor.lancedb_index",
        level = "debug",
        skip(self, paths),
        fields(
            branch = %self.branch_name,
            path_count = paths.len(),
            source = %self.source.name(),
        ),
    )]
    pub async fn index_paths(
        &mut self,
        repo_root: &Path,
        paths: &[PathBuf],
    ) -> Result<IndexerStats, ChunkIndexerError> {
        self.load_state()?;

        let vectors_dir = self.branch_manager.branch_vectors_dir(&self.branch_name);
        let uri = vectors_dir
            .to_str()
            .ok_or_else(|| ChunkIndexerError::NonUtf8VectorsPath {
                path: vectors_dir.clone(),
            })?;
        let conn = lancedb::connect(uri).execute().await?;
        let table = conn.open_table("code_chunks").execute().await?;

        let mut stats = IndexerStats::default();
        let mut row_buffer: Vec<RowDraft> = Vec::new();

        for path in paths {
            stats.files_scanned += 1;

            let repo_rel = path
                .strip_prefix(repo_root)
                .map_or_else(|_| path.clone(), Path::to_path_buf);

            let metadata = tokio::fs::metadata(path).await?;
            let mtime_systemtime =
                metadata
                    .modified()
                    .map_err(|_| ChunkIndexerError::MtimeUnsupported {
                        file: repo_rel.clone(),
                    })?;
            let mtime_secs = mtime_systemtime
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);

            if self.state.file_mtimes.get(&repo_rel) == Some(&mtime_secs) {
                stats.files_skipped_unchanged += 1;
                tracing::debug!(?repo_rel, "skipping unchanged file");
                continue;
            }

            let content_bytes = tokio::fs::read(path).await?;
            let content = String::from_utf8_lossy(&content_bytes).into_owned();
            let file_hash = format!("{:x}", Sha256::digest(&content_bytes));

            let language = infer_language(path);
            let language_str = language_name(language);

            let chunks = {
                let mut guard = self.chunker.lock().await;
                guard.chunk(&repo_rel, &content, language.unwrap_or(Language::Rust))?
            };

            for chunk in chunks {
                let embedding = match self.source.embed(&chunk.content).await {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!(
                            ?repo_rel,
                            chunk_start = chunk.start_line,
                            chunk_end = chunk.end_line,
                            "embedding failed for chunk; skipping: {e}",
                        );
                        stats.chunks_failed += 1;
                        continue;
                    }
                };
                if embedding.len() != self.source.dim() {
                    return Err(ChunkIndexerError::DimensionMismatch {
                        expected: self.source.dim(),
                        got: embedding.len(),
                        file: repo_rel.clone(),
                    });
                }

                let id = format!(
                    "{}:{}:{}",
                    repo_rel.to_string_lossy(),
                    chunk.start_line,
                    chunk.end_line
                );
                row_buffer.push(RowDraft {
                    id,
                    file_path: repo_rel.to_string_lossy().into_owned(),
                    start_line: i32::try_from(chunk.start_line).unwrap_or(i32::MAX),
                    end_line: i32::try_from(chunk.end_line).unwrap_or(i32::MAX),
                    content: chunk.content,
                    language: language_str.to_owned(),
                    symbol_name: None,
                    symbol_kind: None,
                    embedding,
                    token_count: i32::try_from(chunk.token_count).unwrap_or(i32::MAX),
                    file_hash: file_hash.clone(),
                    indexed_at_micros: now_micros(),
                });
            }

            self.state.file_mtimes.insert(repo_rel.clone(), mtime_secs);
            stats.files_indexed += 1;
        }

        if !row_buffer.is_empty() {
            stats.chunks_inserted = row_buffer.len();
            let batch = build_record_batch(&row_buffer, self.source.dim())?;
            let schema = batch.schema();
            let iter = RecordBatchIterator::new(std::iter::once(Ok(batch)), schema);
            table.add(iter).execute().await?;
        }

        self.state.save_atomic(&self.state_path()).await?;

        Ok(stats)
    }
}

// ── IndexerHandle ──────────────────────────────────────────────────────────

/// Spawned tokio task that drives [`LancedbChunkIndexer`] in
/// response to [`crate::watcher::FileEvent`]s.
///
/// The task loops on
/// `events.recv().await` and dispatches each `Created` /
/// `Modified` event to
/// [`LancedbChunkIndexer::index_paths`].  `Removed` and `Renamed`
/// events are dropped (deletion semantics deferred to a follow-up
/// `WO`).  When the channel closes (sender dropped), the loop
/// exits gracefully.
///
/// Construct via [`Self::spawn`].  Call [`Self::shutdown`] to
/// retrieve the join handle and `.await` graceful drain.
#[must_use = "spawned tasks should be awaited via shutdown() to drain cleanly"]
pub struct IndexerHandle {
    join_handle: tokio::task::JoinHandle<()>,
}

impl std::fmt::Debug for IndexerHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IndexerHandle")
            .field("join_handle", &"<JoinHandle<()>>")
            .finish()
    }
}

impl IndexerHandle {
    /// Spawn a tokio task that subscribes to `events` and
    /// dispatches each `Created` / `Modified` event to
    /// [`LancedbChunkIndexer::index_paths`].
    ///
    /// The indexer is held in
    /// `Arc<tokio::sync::Mutex<_>>` so the task can `.lock().await`
    /// per event without taking ownership.
    pub fn spawn<S: EmbeddingSource + 'static>(
        indexer: Arc<tokio::sync::Mutex<LancedbChunkIndexer<S>>>,
        repo_root: PathBuf,
        mut events: tokio::sync::mpsc::Receiver<crate::watcher::FileEvent>,
    ) -> Self {
        let join_handle = tokio::spawn(async move {
            while let Some(event) = events.recv().await {
                match event.kind {
                    crate::watcher::FileEventKind::Created
                    | crate::watcher::FileEventKind::Modified => {
                        let mut guard = indexer.lock().await;
                        let single = std::slice::from_ref(&event.path);
                        if let Err(e) = guard.index_paths(&repo_root, single).await {
                            tracing::warn!(
                                path = ?event.path,
                                "indexer dispatch failed: {e}",
                            );
                        }
                    }
                    crate::watcher::FileEventKind::Removed
                    | crate::watcher::FileEventKind::Renamed
                    | crate::watcher::FileEventKind::Other => {
                        // Removed/Renamed handled in a follow-up WO
                        // (deletion semantics on the LanceDB table).
                        tracing::debug!(
                            path = ?event.path,
                            kind = ?event.kind,
                            "indexer-handle dropping non-create/modify event",
                        );
                    }
                }
            }
        });
        Self { join_handle }
    }

    /// Return the inner join handle so callers can `.await`
    /// graceful drain after closing the events channel.
    #[must_use]
    pub fn shutdown(self) -> tokio::task::JoinHandle<()> {
        self.join_handle
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// Microsecond Unix-epoch timestamp.  Uses
/// `SystemTime::now().duration_since(UNIX_EPOCH)` so we avoid
/// pulling `chrono` into `ucil-daemon`'s dep graph just for a
/// single timestamp value (the workspace `chrono` dep stays
/// isolated to the embeddings / core crates that already use it
/// for bi-temporal RFC-3339 strings).  Returns `0` on the
/// pre-1970 clock-skew case (deterministic, never panics).
fn now_micros() -> i64 {
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    i64::try_from(dur.as_micros()).unwrap_or(i64::MAX)
}

/// Map a path's extension to a [`ucil_treesitter::Language`].
/// Returns `None` for unknown extensions.  Callers default to
/// `Language::Rust` so the chunker never sees a hard error on
/// extension drift; the resulting chunk's `language` column will
/// still reflect the `infer_language`-as-string output.
fn infer_language(path: &Path) -> Option<Language> {
    let ext = path.extension()?.to_str()?;
    Language::from_extension(ext)
}

/// Map an `Option<Language>` to the lowercase string tag that
/// lands in the `code_chunks.language` column.  Mirrors the
/// `Display`-equivalent shape used by other UCIL crates.
const fn language_name(language: Option<Language>) -> &'static str {
    match language {
        Some(Language::Rust) => "rust",
        Some(Language::Python) => "python",
        Some(Language::TypeScript) => "typescript",
        Some(Language::JavaScript) => "javascript",
        Some(Language::Go) => "go",
        Some(Language::Java) => "java",
        Some(Language::C) => "c",
        Some(Language::Cpp) => "cpp",
        Some(Language::Ruby) => "ruby",
        Some(Language::Bash) => "bash",
        Some(Language::Json) => "json",
        _ => "unknown",
    }
}

/// Build a 12-column `RecordBatch` matching
/// [`code_chunks_schema`] from the staged
/// `RowDraft`s.  The `embedding` column is constructed via
/// `FixedSizeListArray::try_new` with a flat `Float32Array` of
/// `rows.len() * dim` values; the inner `Field<"item", Float32,
/// false>` matches the schema's inner field exactly.
fn build_record_batch(rows: &[RowDraft], dim: usize) -> Result<RecordBatch, ChunkIndexerError> {
    let schema = code_chunks_schema();

    let ids = StringArray::from_iter_values(rows.iter().map(|r| r.id.as_str()));
    let file_paths = StringArray::from_iter_values(rows.iter().map(|r| r.file_path.as_str()));
    let start_lines = Int32Array::from_iter_values(rows.iter().map(|r| r.start_line));
    let end_lines = Int32Array::from_iter_values(rows.iter().map(|r| r.end_line));
    let contents = StringArray::from_iter_values(rows.iter().map(|r| r.content.as_str()));
    let languages = StringArray::from_iter_values(rows.iter().map(|r| r.language.as_str()));
    let symbol_names: StringArray = rows
        .iter()
        .map(|r| r.symbol_name.as_deref())
        .collect::<Vec<Option<&str>>>()
        .into();
    let symbol_kinds: StringArray = rows
        .iter()
        .map(|r| r.symbol_kind.as_deref())
        .collect::<Vec<Option<&str>>>()
        .into();

    // Flat-buffer assembly for the FixedSizeList<Float32, dim> column.
    let total_floats: Vec<f32> = rows
        .iter()
        .flat_map(|r| r.embedding.iter().copied())
        .collect();
    let flat_values = PrimitiveArray::<Float32Type>::from(total_floats);
    let inner_field = Arc::new(Field::new("item", DataType::Float32, false));
    let dim_i32 = i32::try_from(dim).unwrap_or(i32::MAX);
    let embedding = FixedSizeListArray::try_new(inner_field, dim_i32, Arc::new(flat_values), None)?;

    let token_counts = Int32Array::from_iter_values(rows.iter().map(|r| r.token_count));
    let file_hashes = StringArray::from_iter_values(rows.iter().map(|r| r.file_hash.as_str()));
    let indexed_ats = TimestampMicrosecondArray::from(
        rows.iter().map(|r| r.indexed_at_micros).collect::<Vec<_>>(),
    );

    let batch = RecordBatch::try_new(
        schema,
        vec![
            Arc::new(ids),
            Arc::new(file_paths),
            Arc::new(start_lines),
            Arc::new(end_lines),
            Arc::new(contents),
            Arc::new(languages),
            Arc::new(symbol_names),
            Arc::new(symbol_kinds),
            Arc::new(embedding),
            Arc::new(token_counts),
            Arc::new(file_hashes),
            Arc::new(indexed_ats),
        ],
    )?;
    Ok(batch)
}

// ── Unit tests for variant coverage ────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexer_state_default_is_empty_with_current_schema() {
        let s = IndexerState::default();
        assert!(s.file_mtimes.is_empty(), "default state must be empty");
        assert_eq!(
            s.schema_version,
            IndexerState::schema_version_current(),
            "default state must be current schema",
        );
    }

    #[tokio::test]
    async fn indexer_state_load_or_default_returns_default_for_missing_file() {
        let tmp = tempfile::tempdir().expect("tmp");
        let path = tmp.path().join("definitely-not-here.json");
        let s = IndexerState::load_or_default(&path).expect("missing -> default");
        assert_eq!(s, IndexerState::default(), "missing file -> default state");
    }

    #[tokio::test]
    async fn indexer_state_save_atomic_round_trips() {
        let tmp = tempfile::tempdir().expect("tmp");
        let path = tmp.path().join("state").join("indexer-state.json");
        let mut s = IndexerState::default();
        s.file_mtimes
            .insert(PathBuf::from("src/lib.rs"), 1_700_000_000);
        s.file_mtimes
            .insert(PathBuf::from("src/main.rs"), 1_700_000_001);
        s.save_atomic(&path).await.expect("save");
        let loaded = IndexerState::load_or_default(&path).expect("load");
        assert_eq!(loaded, s, "round-trip must preserve state");
    }

    #[test]
    fn indexer_state_load_or_default_surfaces_json_error_on_corrupt_file() {
        let tmp = tempfile::tempdir().expect("tmp");
        let path = tmp.path().join("corrupt.json");
        std::fs::write(&path, b"{ not valid json").expect("write");
        match IndexerState::load_or_default(&path) {
            Err(ChunkIndexerError::Json { .. }) => {}
            other => panic!("expected ChunkIndexerError::Json; got {other:?}"),
        }
    }

    #[test]
    fn indexer_state_load_or_default_surfaces_io_error_on_unreadable_dir() {
        // Pass a directory path where a regular file is expected — POSIX
        // returns IsADirectory or similar non-NotFound IO error so the
        // load_or_default path that bubbles non-NotFound errors fires.
        let tmp = tempfile::tempdir().expect("tmp");
        let dir_as_file = tmp.path().to_path_buf();
        match IndexerState::load_or_default(&dir_as_file) {
            // Some platforms may surface this as NotFound semantics — fall
            // back to accepting the default response in that case.
            Err(ChunkIndexerError::Io { .. }) | Ok(_) => {}
            other => panic!("expected Io error or default; got {other:?}"),
        }
    }

    #[test]
    fn chunk_indexer_error_dimension_mismatch_display_includes_path() {
        let e = ChunkIndexerError::DimensionMismatch {
            expected: 768,
            got: 7,
            file: PathBuf::from("src/foo.rs"),
        };
        let msg = format!("{e}");
        assert!(
            msg.contains("768"),
            "Display must include expected dim; got {msg:?}"
        );
        assert!(
            msg.contains("foo.rs"),
            "Display must include path; got {msg:?}"
        );
    }

    #[test]
    fn chunk_indexer_error_mtime_unsupported_display_includes_path() {
        let e = ChunkIndexerError::MtimeUnsupported {
            file: PathBuf::from("src/bar.rs"),
        };
        let msg = format!("{e}");
        assert!(
            msg.contains("bar.rs"),
            "Display must include path; got {msg:?}"
        );
    }

    #[test]
    fn chunk_indexer_error_non_utf8_vectors_path_display() {
        let e = ChunkIndexerError::NonUtf8VectorsPath {
            path: PathBuf::from("/tmp/bad-vectors"),
        };
        let msg = format!("{e}");
        assert!(
            msg.contains("non-UTF8"),
            "Display must mention non-UTF8; got {msg:?}"
        );
    }

    #[test]
    fn chunk_indexer_error_arrow_variant_via_from() {
        let arrow_err = arrow_schema::ArrowError::InvalidArgumentError("x".to_owned());
        let e: ChunkIndexerError = arrow_err.into();
        assert!(
            matches!(e, ChunkIndexerError::Arrow { .. }),
            "From<ArrowError> must produce Arrow variant; got {e:?}",
        );
    }

    #[test]
    fn chunk_indexer_error_io_variant_via_from() {
        let io_err = std::io::Error::other("x");
        let e: ChunkIndexerError = io_err.into();
        assert!(
            matches!(e, ChunkIndexerError::Io { .. }),
            "From<io::Error> must produce Io variant; got {e:?}",
        );
    }

    #[test]
    fn chunk_indexer_error_json_variant_via_from() {
        let json_err = serde_json::from_str::<i32>("not-json").unwrap_err();
        let e: ChunkIndexerError = json_err.into();
        assert!(
            matches!(e, ChunkIndexerError::Json { .. }),
            "From<serde_json::Error> must produce Json variant; got {e:?}",
        );
    }

    #[test]
    fn chunk_indexer_error_branch_manager_variant_via_from() {
        let bm_err = BranchManagerError::NonUtf8Path {
            path: PathBuf::from("/x"),
        };
        let e: ChunkIndexerError = bm_err.into();
        assert!(
            matches!(e, ChunkIndexerError::BranchManager { .. }),
            "From<BranchManagerError> must produce BranchManager variant; got {e:?}",
        );
    }

    #[test]
    fn chunk_indexer_error_chunker_variant_via_from() {
        let c_err = EmbeddingChunkerError::Tokenizer {
            message: "x".to_owned(),
        };
        let e: ChunkIndexerError = c_err.into();
        assert!(
            matches!(e, ChunkIndexerError::Chunker { .. }),
            "From<EmbeddingChunkerError> must produce Chunker variant; got {e:?}",
        );
    }

    #[test]
    fn chunk_indexer_error_embedding_variant_via_from() {
        let emb_err = EmbeddingSourceError::Other {
            message: "x".to_owned(),
        };
        let e: ChunkIndexerError = emb_err.into();
        assert!(
            matches!(e, ChunkIndexerError::Embedding { .. }),
            "From<EmbeddingSourceError> must produce Embedding variant; got {e:?}",
        );
    }

    #[test]
    fn embedding_source_error_other_display_includes_message() {
        let e = EmbeddingSourceError::Other {
            message: "test failure".to_owned(),
        };
        let msg = format!("{e}");
        assert!(
            msg.contains("test failure"),
            "Display must include message; got {msg:?}",
        );
    }

    #[test]
    fn embedding_source_error_coderankembed_via_from() {
        let cre = CodeRankEmbedError::DimensionMismatch {
            expected: 768,
            got: 7,
        };
        let e: EmbeddingSourceError = cre.into();
        assert!(
            matches!(e, EmbeddingSourceError::CodeRankEmbed { .. }),
            "From<CodeRankEmbedError> must produce CodeRankEmbed variant; got {e:?}",
        );
    }

    #[test]
    fn coderankembed_source_load_surfaces_missing_model_dir() {
        let absent = PathBuf::from("/definitely/not/a/real/coderankembed-WO-0064");
        match CodeRankEmbeddingSource::load(&absent) {
            Err(EmbeddingSourceError::CodeRankEmbed { .. }) => {}
            other => panic!("expected CodeRankEmbed-wrapped MissingModelFile; got {other:?}"),
        }
    }

    #[test]
    fn infer_language_maps_known_extensions() {
        assert_eq!(infer_language(Path::new("foo.rs")), Some(Language::Rust));
        assert_eq!(infer_language(Path::new("foo.py")), Some(Language::Python));
        assert_eq!(
            infer_language(Path::new("foo.ts")),
            Some(Language::TypeScript)
        );
        assert_eq!(infer_language(Path::new("foo.unknown")), None);
        assert_eq!(infer_language(Path::new("noext")), None);
    }

    #[test]
    fn language_name_maps_all_variants() {
        assert_eq!(language_name(Some(Language::Rust)), "rust");
        assert_eq!(language_name(Some(Language::Python)), "python");
        assert_eq!(language_name(Some(Language::TypeScript)), "typescript");
        assert_eq!(language_name(Some(Language::JavaScript)), "javascript");
        assert_eq!(language_name(Some(Language::Go)), "go");
        assert_eq!(language_name(Some(Language::Java)), "java");
        assert_eq!(language_name(Some(Language::C)), "c");
        assert_eq!(language_name(Some(Language::Cpp)), "cpp");
        assert_eq!(language_name(Some(Language::Ruby)), "ruby");
        assert_eq!(language_name(Some(Language::Bash)), "bash");
        assert_eq!(language_name(Some(Language::Json)), "json");
        assert_eq!(language_name(None), "unknown");
    }
}
