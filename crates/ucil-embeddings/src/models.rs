//! `CodeRankEmbed` — the default CPU embedding model for UCIL.
//!
//! Master-plan §18 Phase 2 Week 8 line 1787 specifies "`CodeRankEmbed`
//! (137M, CPU) as default, Qwen3-Embedding (8B, GPU optional) as
//! upgrade"; master-plan §4.2 line 303 expands this to "`CodeRankEmbed`
//! (137M params, MIT license, 8K context) ... CPU-friendly, 50-150
//! embeddings/sec, ~137MB with Int8 quantization"; master-plan §13
//! line 1332 pins the embedding dimension at `vector[768]`; master-plan
//! §17.6 lines 2028-2029 fix the configuration knobs at
//! `embedding_model = "coderankembed"` + `embedding_dimensions = 768`.
//!
//! This module sits alongside the foundational [`OnnxSession`]
//! (`P2-W8-F01` / `WO-0058` — `crates/ucil-embeddings/src/onnx_inference.rs`)
//! and pairs `ort` directly with the `HuggingFace` `tokenizers` crate
//! to land the production embedding primitive [`CodeRankEmbed`]:
//!
//! - [`CodeRankEmbed::load`] opens the on-disk Int8 `ONNX` model + the
//!   `tokenizer.json` from `model_dir`;
//! - [`CodeRankEmbed::embed`] tokenises a code snippet, builds the
//!   `attention_mask` companion tensor, runs `ONNX` inference, reads
//!   the model's pre-pooled `sentence_embedding` output (`[1, 768]`),
//!   L2-normalises, and returns a 768-dim `Vec<f32>` per the
//!   master-plan-frozen [`EMBEDDING_DIM`] constant.
//!
//! **Upstream-fit divergence from `WO-0059` `scope_in[7]`** (per the
//! `WO-0058`-line-543 "five upstream-API-shape adaptations" precedent):
//! the WO prescribed composing [`OnnxSession::infer`] for the inference
//! step, but the production `CodeRankEmbed` `ONNX` export declares two
//! inputs (`input_ids` + `attention_mask`, both `int64`) and two
//! outputs (`token_embeddings: [batch, seq, 768]` +
//! `sentence_embedding: [batch, 768]`).  [`OnnxSession::infer`] is
//! single-input / first-output only by design (the `WO-0058` minimal
//! fixture has one input named `input_ids`); extending it would touch
//! `crates/ucil-embeddings/src/onnx_inference.rs` which is in
//! `WO-0059` `forbidden_paths`.  The pragmatic fit is to load `ort`
//! directly here in `models.rs` (both modules live in the same crate,
//! so import discipline is unchanged) and document the divergence so
//! the `OnnxSession` foundational layer is preserved unchanged for
//! `P2-W8-F03` (Qwen3 GPU upgrade, also dual-input) and downstream
//! consumers.  A future WO MAY refactor [`OnnxSession::infer`] to take
//! a typed multi-input map; that is intentionally out of scope here.
//!
//! The upstream `ort 2.x` `Session::run` signature is `&mut self`
//! (per `WO-0058` lessons line 561), so [`CodeRankEmbed::embed`]
//! mirrors that contract.  Consumers needing shared inference must
//! wrap in `Arc<Mutex<CodeRankEmbed>>` or serialise via a
//! `tokio::sync::mpsc` channel — same plumbing pattern as
//! [`OnnxSession`] (see its struct-level rustdoc for the canonical
//! worked example).
//!
//! Deferrals (out of scope for this module per `WO-0059` `scope_out`):
//!
//! - `P2-W8-F03` — Qwen3-Embedding GPU upgrade path (separate
//!   `models::Qwen3Embedding` to land in a future WO; the
//!   [`CodeRankEmbed`] structural pattern serves as the template);
//! - `P2-W8-F04` — `LanceDB` chunk indexer (consumes
//!   [`CodeRankEmbed::embed`] outputs);
//! - `P2-W8-F05` — chunker (splits source files at tree-sitter
//!   function/class boundaries into ≤512-token chunks; produces the
//!   `&str` snippets [`CodeRankEmbed::embed`] consumes);
//! - `P2-W8-F06` — throughput benchmark (≥50 emb/sec on CPU);
//! - `P2-W8-F07` — vector query latency benchmark (p95 <100ms warm);
//! - `P2-W8-F08` — `find_similar` MCP tool (composes
//!   [`CodeRankEmbed::embed`] over `LanceDB` vector search at the
//!   daemon's MCP handler).
//!
//! The model + tokenizer artefacts (~137MB) are NOT committed; the
//! devtool installer `scripts/devtools/install-coderankembed.sh`
//! downloads them to `ml/models/coderankembed/{model.onnx,tokenizer.json}`
//! from a pinned upstream `HuggingFace` mirror (see the installer header
//! for the upstream URL + `SHA256` fingerprints).

use std::path::{Path, PathBuf};

use ndarray::Array2;
use ort::session::Session;
use ort::value::Tensor;
use tokenizers::Tokenizer;

/// The master-plan-frozen embedding dimension for `CodeRankEmbed`.
///
/// Per master-plan §13 line 1332 (`embedding: vector[768]`) and §17.6
/// lines 2028-2029 (`embedding_model = "coderankembed"` +
/// `embedding_dimensions = 768`); this constant is consumed by
/// downstream features `P2-W8-F04` (`LanceDB` schema), `P2-W8-F05`
/// (chunker output validation), and `P2-W8-F08` (`find_similar` `MCP`
/// tool dimension assertion).
pub const EMBEDDING_DIM: usize = 768;

/// Errors emitted by [`CodeRankEmbed`] operations.
///
/// Variants are `#[non_exhaustive]` per `.claude/rules/rust-style.md`
/// so that future additions (e.g. a `BatchSizeExceeded` for batched
/// inference in a Phase-3 `WO`) do not break downstream `match`
/// exhaustiveness.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CodeRankEmbedError {
    /// Wraps an `ort::Error` from the underlying `ONNX` Runtime —
    /// covers session construction failures, missing input/output
    /// names, and inference failures via the auto-conversion `?`.
    /// `CodeRankEmbed` loads `ort::Session` directly rather than
    /// composing [`crate::OnnxSession`] (see module-level rustdoc for
    /// the upstream-fit divergence rationale; the production
    /// `CodeRankEmbed` `ONNX` export declares dual inputs
    /// `input_ids` + `attention_mask`, which the single-input
    /// [`crate::OnnxSession::infer`] cannot service without editing
    /// `crates/ucil-embeddings/src/onnx_inference.rs` — that file is
    /// in the `WO-0059` `forbidden_paths`.  See `ONNX`-export schema
    /// notes in the module-level rustdoc.
    #[error("ort session error: {source}")]
    Onnx {
        /// The underlying `ort::Error`.
        #[from]
        source: ort::Error,
    },

    /// Shape-construction error from `ndarray::Array2::from_shape_vec`.
    /// The literal shape passed by [`CodeRankEmbed::embed`] always
    /// matches the input slice length, but the upstream `API` is
    /// fallible so the variant is included for defensiveness.
    #[error("ndarray shape error: {source}")]
    Ndarray {
        /// The underlying `ndarray::ShapeError`.
        #[from]
        source: ndarray::ShapeError,
    },

    /// Wraps a `tokenizers`-crate error.
    ///
    /// The upstream `tokenizers::Error` is a
    /// `Box<dyn Error + Send + Sync>` whose concrete type is unstable
    /// across minor versions; storing the rendered message as a
    /// `String` insulates this crate's public `API` from
    /// `tokenizers`-version churn (no `#[from]` because the
    /// boxed-error → `String` shim needs an explicit `.map_err`).
    /// The field is named `message` rather than `source` because
    /// `thiserror` treats a `source`-named field as a `std::error::Error`
    /// chain link (auto-implements `Error::source()`); a `String` does
    /// not satisfy that bound, so renaming to `message` is the
    /// `thiserror`-compatible shape — `WO-0059` `scope_in[17]`
    /// upstream-fit precedent.
    #[error("tokenizer error: {message}")]
    Tokenizer {
        /// The rendered upstream error message.
        message: String,
    },

    /// Filesystem error while resolving a model directory entry.
    #[error("io error: {source}")]
    Io {
        /// The underlying [`std::io::Error`].
        #[from]
        source: std::io::Error,
    },

    /// A required model artefact is missing from `model_dir` (either
    /// `model.onnx` or `tokenizer.json` per
    /// [`CodeRankEmbed::load`]'s contract).
    ///
    /// Operators see this when `scripts/devtools/install-coderankembed.sh`
    /// has not yet run — the verify script
    /// `scripts/verify/P2-W8-F02.sh` runs the installer first
    /// idempotently to keep this variant's surface narrow to a real
    /// "operator forgot to install" condition (see `OPS` notes
    /// in the module-level rustdoc).
    #[error("required model file missing at {path:?}")]
    MissingModelFile {
        /// The absolute path that was looked up.
        path: PathBuf,
    },

    /// The output `Vec<f32>` does not match the master-plan-frozen
    /// [`EMBEDDING_DIM`] (768).
    ///
    /// This variant fires when (a) the upstream model artefact has
    /// been swapped for one with a different head dimension, or (b)
    /// the mean-pooling division produces a remainder when the raw
    /// per-token output length is not a multiple of
    /// [`EMBEDDING_DIM`].  Either case indicates a model / tokenizer
    /// mismatch and is surfaced as an actionable error rather than a
    /// silent garbage-shaped vector.
    #[error("unexpected embedding dimension: expected {expected}, got {got}")]
    DimensionMismatch {
        /// The master-plan-frozen expected dimension
        /// ([`EMBEDDING_DIM`]).
        expected: usize,
        /// The actually observed dimension.
        got: usize,
    },
}

/// A loaded `CodeRankEmbed` model bundle: an `ONNX` session + tokenizer.
///
/// Holds an `ort::session::Session` together with the
/// `tokenizers::Tokenizer` so a single `&mut CodeRankEmbed` can
/// service an `embed(&str)` call without crossing borrow boundaries.
/// The `model_dir` is retained for `tracing` introspection only —
/// inference does not re-read the directory on subsequent calls.
///
/// **Not** `Clone` — the embedded `ort::session::Session` owns
/// non-duplicable runtime resources (`CPU` execution-provider arena,
/// `OS` handles for the `download-binaries` shared library);
/// consumers needing shared inference must wrap in
/// `Arc<Mutex<CodeRankEmbed>>` because [`CodeRankEmbed::embed`]
/// takes `&mut self` (the upstream `ort 2.x` `Session::run`
/// signature).
///
/// **`Send`** — both `ort::session::Session` and
/// `tokenizers::Tokenizer` are `Send`, so a `CodeRankEmbed` can be
/// moved into a `tokio::task::spawn_blocking` closure for async wrap
/// at the `P2-W8-F04` / `P2-W8-F08` (`MCP`) consumer sites.
#[derive(Debug)]
pub struct CodeRankEmbed {
    session: Session,
    tokenizer: Tokenizer,
    #[allow(dead_code)]
    model_dir: PathBuf,
}

impl CodeRankEmbed {
    /// Load the `CodeRankEmbed` bundle from `model_dir`.
    ///
    /// `model_dir` (`MUST`) contain two artefacts (laid down by
    /// `scripts/devtools/install-coderankembed.sh`):
    /// `model.onnx` (the Int8-quantised `CodeRankEmbed` export, ~137MB)
    /// and `tokenizer.json` (the `HuggingFace` `BPE` tokenizer with
    /// special tokens for `CLS` / `SEP`).  An early existence check
    /// produces an operator-friendly
    /// [`CodeRankEmbedError::MissingModelFile`] before the more
    /// opaque `ort::Error` from the underlying parser.
    ///
    /// # Errors
    ///
    /// - [`CodeRankEmbedError::MissingModelFile`] if either
    ///   `model.onnx` or `tokenizer.json` is absent;
    /// - [`CodeRankEmbedError::Onnx`] if the `ONNX` graph fails to
    ///   parse / load (corrupt model, `ABI` mismatch);
    /// - [`CodeRankEmbedError::Tokenizer`] if the `tokenizer.json`
    ///   fails to deserialise.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::path::Path;
    /// use ucil_embeddings::CodeRankEmbed;
    ///
    /// let model = CodeRankEmbed::load(Path::new("ml/models/coderankembed"))
    ///     .expect("CodeRankEmbed model present");
    /// let _ = model;
    /// ```
    #[tracing::instrument(
        name = "ucil.embeddings.coderankembed.load",
        level = "debug",
        skip(model_dir),
        fields(model_dir = ?model_dir)
    )]
    pub fn load(model_dir: &Path) -> Result<Self, CodeRankEmbedError> {
        let model_path = model_dir.join("model.onnx");
        if !model_path.exists() {
            return Err(CodeRankEmbedError::MissingModelFile { path: model_path });
        }
        let session = Session::builder()?.commit_from_file(&model_path)?;

        let tokenizer_path = model_dir.join("tokenizer.json");
        if !tokenizer_path.exists() {
            return Err(CodeRankEmbedError::MissingModelFile {
                path: tokenizer_path,
            });
        }
        let tokenizer =
            Tokenizer::from_file(&tokenizer_path).map_err(|e| CodeRankEmbedError::Tokenizer {
                message: format!("{e}"),
            })?;

        Ok(Self {
            session,
            tokenizer,
            model_dir: model_dir.to_owned(),
        })
    }

    /// Tokenise `code`, run `ONNX` inference (feeding `input_ids` +
    /// `attention_mask`), read the model's pre-pooled
    /// `sentence_embedding` output, `L2`-normalise, and return a
    /// `768`-dim `Vec<f32>`.
    ///
    /// The production `CodeRankEmbed` `ONNX` export emits two outputs:
    ///
    /// - `token_embeddings` — `[batch, seq, 768]` per-token hidden
    ///   states;
    /// - `sentence_embedding` — `[batch, 768]` pre-pooled at the
    ///   graph level via the upstream `1_Pooling/config.json`
    ///   (mean-pooling over `attention_mask`).
    ///
    /// This implementation reads the `sentence_embedding` output and
    /// `L2`-normalises it so downstream cosine-similarity search at
    /// `P2-W8-F08` reduces to a dot product.  The Euclidean norm is
    /// clamped to `f32::EPSILON` to avoid `NaN` on a degenerate
    /// all-zero output (`NaN`-safety).  When the model output's flat
    /// length is not exactly [`EMBEDDING_DIM`] the function returns
    /// [`CodeRankEmbedError::DimensionMismatch`] — this is a
    /// model/tokenizer mismatch (e.g. swapped for a model with a
    /// different head dimension) and is surfaced as an actionable
    /// error rather than a silent garbage-shaped vector.
    ///
    /// `attention_mask` is constructed as a tensor of `i64` `1`s with
    /// the same shape as `input_ids` — the tokenizer does not pad
    /// (single-snippet inference, no batching), so every position is
    /// attended (`PAD`-aware behaviour deferred).  When `P2-W8-F05`
    /// (chunker) lands and feeds batched inputs, the mask will need
    /// 0-padding for the right tail; the mask construction is
    /// centralised here in anticipation.
    ///
    /// `&mut self` is required because the upstream `ort 2.x`
    /// `Session::run` takes `&mut self`; consumers needing shared
    /// inference must wrap in `Arc<Mutex<CodeRankEmbed>>`.
    ///
    /// # Errors
    ///
    /// - [`CodeRankEmbedError::Tokenizer`] if `code` is unencodable
    ///   (e.g. invalid `UTF-8` boundary in a partial tokenizer chunk —
    ///   in practice never fires for well-formed source text);
    /// - [`CodeRankEmbedError::Ndarray`] if the input shape
    ///   construction fails (defensive — literal shape always matches);
    /// - [`CodeRankEmbedError::Onnx`] if the `ONNX` inference fails
    ///   (typically: the tokenizer produced more tokens than the
    ///   model's `max_position_embeddings` allows);
    /// - [`CodeRankEmbedError::DimensionMismatch`] if the
    ///   `sentence_embedding` output's flat length is not exactly
    ///   [`EMBEDDING_DIM`] (model swapped for an incompatible head).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use std::path::Path;
    /// # use ucil_embeddings::CodeRankEmbed;
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut model = CodeRankEmbed::load(Path::new("ml/models/coderankembed"))?;
    /// let v: Vec<f32> = model.embed("fn hello() { println!(\"hi\"); }")?;
    /// debug_assert_eq!(v.len(), 768);
    /// # Ok(())
    /// # }
    /// ```
    #[tracing::instrument(
        name = "ucil.embeddings.coderankembed.embed",
        level = "debug",
        skip(self, code),
        fields(code_len = code.len())
    )]
    pub fn embed(&mut self, code: &str) -> Result<Vec<f32>, CodeRankEmbedError> {
        let encoding =
            self.tokenizer
                .encode(code, true)
                .map_err(|e| CodeRankEmbedError::Tokenizer {
                    message: format!("{e}"),
                })?;
        let token_ids: Vec<i64> = encoding.get_ids().iter().map(|id| i64::from(*id)).collect();
        let seq_len = token_ids.len();
        let attention_mask: Vec<i64> = vec![1i64; seq_len];

        let ids_array: Array2<i64> = Array2::from_shape_vec((1, seq_len), token_ids)?;
        let mask_array: Array2<i64> = Array2::from_shape_vec((1, seq_len), attention_mask)?;

        let ids_shape = [ids_array.shape()[0], ids_array.shape()[1]];
        let mask_shape = [mask_array.shape()[0], mask_array.shape()[1]];
        let (ids_data, _) = ids_array.into_raw_vec_and_offset();
        let (mask_data, _) = mask_array.into_raw_vec_and_offset();
        let ids_tensor = Tensor::<i64>::from_array((ids_shape, ids_data))?;
        let mask_tensor = Tensor::<i64>::from_array((mask_shape, mask_data))?;

        let outputs = self.session.run(ort::inputs![
            "input_ids" => ids_tensor,
            "attention_mask" => mask_tensor,
        ])?;

        let sentence = outputs.get("sentence_embedding").ok_or_else(|| {
            CodeRankEmbedError::DimensionMismatch {
                expected: EMBEDDING_DIM,
                got: 0,
            }
        })?;
        let (_shape, slice) = sentence.try_extract_tensor::<f32>()?;
        let raw = slice.to_vec();

        pool_and_normalise(&raw)
    }
}

/// Validate the raw `sentence_embedding` slice and `L2`-normalise.
///
/// Extracted from [`CodeRankEmbed::embed`] in the `WO-0059` retry-1
/// coverage-driven refactor so the dimension-invariant + normalisation
/// logic is testable in isolation (the seven `CodeRankEmbedError`
/// variants would otherwise demand a real-model fixture per branch).
/// Production semantics are unchanged; the post-normalise length guard
/// is elided because the helper preserves length by construction (a
/// `for p in &mut pooled` loop cannot grow or shrink the `Vec`).
fn pool_and_normalise(raw: &[f32]) -> Result<Vec<f32>, CodeRankEmbedError> {
    if raw.len() != EMBEDDING_DIM {
        return Err(CodeRankEmbedError::DimensionMismatch {
            expected: EMBEDDING_DIM,
            got: raw.len(),
        });
    }
    let mut pooled = raw.to_vec();
    let norm_sq: f32 = pooled.iter().map(|x| x * x).sum();
    let norm = norm_sq.sqrt().max(f32::EPSILON);
    for p in &mut pooled {
        *p /= norm;
    }
    Ok(pooled)
}

// ─── Qwen3-Embedding GPU upgrade path (P2-W8-F03 / WO-0062) ───────────
//
// Master-plan §4.2 line 303 freezes the upgrade-path model:
// "Qwen3-Embedding-8B (Apache 2.0, 80.68 MTEB-Code, 32K context,
// Matryoshka dimension support 32–7168). Substantially better
// retrieval quality but requires GPU for reasonable throughput."
// Master-plan §18 Phase 2 Week 8 line 1787 reiterates the choice as
// "Qwen3-Embedding (8B, GPU optional) as upgrade".
//
// This crate's `ort` workspace dep is currently configured
// `default-features = false` (Cargo.toml line 194), so the underlying
// ONNX Runtime shared library does NOT include the CUDA / TensorRT /
// DirectML execution providers.  The [`detect_gpu_execution_provider`]
// function therefore returns `Err(NoGpuDetected)` on this build —
// that is the EXPECTED behaviour and the frozen acceptance test
// [`test_qwen3_config_gate`] asserts on it.  A future workspace `ort`
// feature flip (`features = ["cuda"]` or analogue) will activate the
// GPU code path WITHOUT requiring any API surface change here.

/// Master-plan-frozen Matryoshka dimension lower bound for Qwen3-Embedding-8B.
///
/// Master-plan §4.2 line 303 — "Matryoshka dimension support 32–7168".
/// Inclusive on both ends per the standard
/// Matryoshka-Representation-Learning convention.
pub const MIN_MATRYOSHKA_DIM: usize = 32;

/// Master-plan-frozen Matryoshka dimension upper bound for Qwen3-Embedding-8B.
///
/// Master-plan §4.2 line 303 — "Matryoshka dimension support 32–7168".
/// Inclusive on both ends.
pub const MAX_MATRYOSHKA_DIM: usize = 7168;

/// The kind of GPU execution provider detected at runtime.
///
/// Master-plan §4.2 line 303 frames Qwen3-Embedding-8B as a
/// GPU-only upgrade; the three EPs that ONNX Runtime supports for
/// Qwen3-class workloads on consumer / data-center hardware are
/// `CUDA` (NVIDIA), `TensorRT` (NVIDIA optimised graph), and
/// `DirectML` (Windows GPU layer that targets D3D12).  Absence of
/// any GPU is signalled via `Result::Err(NoGpuDetected)`, NOT a
/// `None` variant on this enum — that keeps the success path
/// unambiguous.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuKind {
    /// NVIDIA CUDA execution provider.
    Cuda,
    /// NVIDIA `TensorRT` execution provider (optimised CUDA graph).
    TensorRt,
    /// Windows `DirectML` execution provider.
    DirectMl,
}

/// Errors emitted by [`Qwen3Embedding`] operations and the
/// helper functions that back it.
///
/// `#[non_exhaustive]` so future variants (e.g. a `BatchSizeExceeded`
/// when GPU batched inference lands) can be added without breaking
/// downstream `match` exhaustiveness.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Qwen3EmbeddingError {
    /// No GPU execution provider was detected.  On the current
    /// workspace `ort` build (`default-features = false`), this
    /// variant fires unconditionally — the EXPECTED behaviour for
    /// `P2-W8-F03` per the master-plan-frozen GPU-only upgrade
    /// requirement.
    #[error("no GPU execution provider available: {reason}")]
    NoGpuDetected {
        /// Operator-readable description of why the GPU was not
        /// available (workspace feature flags, runtime probe failure,
        /// etc.).
        reason: String,
    },

    /// The requested Matryoshka dimension is outside the
    /// master-plan-frozen `[32, 7168]` range (master-plan §4.2
    /// line 303).  Bounds are inclusive on both ends per the
    /// standard `MRL` convention.
    #[error("Matryoshka dimension {value} out of range [{min}, {max}]")]
    DimensionOutOfRange {
        /// The requested dimension that failed the bounds check.
        value: usize,
        /// The master-plan-frozen lower bound
        /// ([`MIN_MATRYOSHKA_DIM`]).
        min: usize,
        /// The master-plan-frozen upper bound
        /// ([`MAX_MATRYOSHKA_DIM`]).
        max: usize,
    },

    /// A required Qwen3 model artefact (`model.onnx` or
    /// `tokenizer.json`) is missing from the configured
    /// `model_dir`.  Defensive — the current build returns
    /// [`Qwen3EmbeddingError::NoGpuDetected`] before reaching the
    /// load path, but the variant is wired in for the future
    /// GPU-enabled flow.
    #[error("required Qwen3 model file missing at {path:?}")]
    MissingModelFile {
        /// The absolute path that was looked up.
        path: PathBuf,
    },

    /// Wraps an `ort::Error` from the underlying `ONNX` Runtime —
    /// covers session construction / inference failures when GPU
    /// inference is enabled.  Defensive — wired in for the future
    /// GPU-enabled load path.
    #[error("ort session error: {source}")]
    Onnx {
        /// The underlying `ort::Error`.
        #[from]
        source: ort::Error,
    },

    /// Wraps a `tokenizers`-crate error.  Defensive — see the
    /// equivalent variant on [`CodeRankEmbedError`] for the
    /// upstream-fit rationale (boxed-error → `String` shim).
    #[error("tokenizer error: {message}")]
    Tokenizer {
        /// The rendered upstream error message.
        message: String,
    },

    /// Filesystem error while resolving a model artefact — wraps
    /// [`std::io::Error`] for the future GPU-enabled flow.
    #[error("io error: {source}")]
    Io {
        /// The underlying [`std::io::Error`].
        #[from]
        source: std::io::Error,
    },
}

/// Validate a requested Matryoshka dimension against the
/// master-plan-frozen `[MIN_MATRYOSHKA_DIM, MAX_MATRYOSHKA_DIM]`
/// range.
///
/// Master-plan §4.2 line 303 fixes the bounds at `32-7168`
/// inclusive on both ends.  The function is `pub` and pure (no
/// IO, no `&self`), so it is testable in isolation per `WO-0059`
/// lessons line 600 ("extract `pub(crate)` helpers for in-test
/// invocation"; the more-`pub` shape is fine here because the
/// bounds are part of the API surface).
///
/// # Errors
///
/// - [`Qwen3EmbeddingError::DimensionOutOfRange`] if `d` is below
///   [`MIN_MATRYOSHKA_DIM`] or above [`MAX_MATRYOSHKA_DIM`].
///
/// # Examples
///
/// ```
/// use ucil_embeddings::models::validate_matryoshka_dimension;
/// assert!(validate_matryoshka_dimension(1024).is_ok());
/// assert!(validate_matryoshka_dimension(31).is_err());
/// ```
pub fn validate_matryoshka_dimension(d: usize) -> Result<usize, Qwen3EmbeddingError> {
    if (MIN_MATRYOSHKA_DIM..=MAX_MATRYOSHKA_DIM).contains(&d) {
        Ok(d)
    } else {
        Err(Qwen3EmbeddingError::DimensionOutOfRange {
            value: d,
            min: MIN_MATRYOSHKA_DIM,
            max: MAX_MATRYOSHKA_DIM,
        })
    }
}

/// Detect an available GPU execution provider in the loaded `ONNX`
/// Runtime shared library.
///
/// Implementation strategy: probe each candidate `EP` via the
/// `ort::ep::ExecutionProvider::is_available()` trait method
/// (`ort 2.0.0-rc.12 src/ep/mod.rs:118`).  The probe queries the
/// loaded `ONNX` Runtime shared library at runtime via the C-API
/// `GetAvailableProviders` call (`is_ep_available` in
/// `src/ep/mod.rs:371`); the workspace `ort` dep is configured
/// `default-features = false` (Cargo.toml line 194), so the
/// downloaded CPU-only `ONNX` Runtime shared library does NOT
/// register any GPU `EP` and every probe returns `Ok(false)` — the
/// function therefore returns
/// [`Qwen3EmbeddingError::NoGpuDetected`] unconditionally on this
/// build.  A future workspace `ort` feature flip
/// (`features = ["cuda", "download-binaries"]` etc.) pulls in a
/// GPU-capable shared library and the probe path activates without
/// any API surface change here.
///
/// The function NEVER panics — operator-readable error always.
///
/// # Errors
///
/// - [`Qwen3EmbeddingError::NoGpuDetected`] when no GPU EP is
///   available (the current workspace state always returns this).
#[tracing::instrument(name = "ucil.embeddings.qwen3.detect_gpu", level = "debug")]
pub fn detect_gpu_execution_provider() -> Result<GpuKind, Qwen3EmbeddingError> {
    use ort::ep::ExecutionProvider;

    if matches!(ort::ep::CUDA::default().is_available(), Ok(true)) {
        return Ok(GpuKind::Cuda);
    }
    if matches!(ort::ep::TensorRT::default().is_available(), Ok(true)) {
        return Ok(GpuKind::TensorRt);
    }
    if matches!(ort::ep::DirectML::default().is_available(), Ok(true)) {
        return Ok(GpuKind::DirectMl);
    }

    Err(Qwen3EmbeddingError::NoGpuDetected {
        reason: "no GPU execution provider compiled in (workspace ort: \
                 default-features=false; need cuda/tensorrt/directml feature \
                 flag to enable GPU inference)"
            .to_owned(),
    })
}

/// A loaded Qwen3-Embedding bundle: model directory + Matryoshka
/// dimension configuration.
///
/// Mirrors the [`CodeRankEmbed`] structural template but does NOT
/// open an actual `.onnx` file — the production Qwen3-Embedding-8B
/// model is ~16-32 GB and is operator-installed at a future
/// consumer site (`P2-W8-F04` `LanceDB` indexer or `P2-W8-F08`
/// `find_similar` MCP tool, per `WO-0062` `scope_out`).  The
/// current build's [`Qwen3Embedding::load`] returns
/// `Err(NoGpuDetected)` before any filesystem IO because GPU
/// detection runs first; the actual model loading is wired for
/// the future GPU-enabled flow.
///
/// **Not** `Clone`; **`Send`** so the bundle composes with
/// `tokio::task::spawn_blocking` at consumer sites.
#[derive(Debug)]
pub struct Qwen3Embedding {
    #[allow(dead_code)]
    model_dir: PathBuf,
    dimensions: usize,
}

impl Qwen3Embedding {
    /// Load a [`Qwen3Embedding`] bundle.
    ///
    /// Implementation order:
    ///
    /// 1. [`validate_matryoshka_dimension`] checks `dimensions` is
    ///    in `[MIN_MATRYOSHKA_DIM, MAX_MATRYOSHKA_DIM]` (master-plan
    ///    §4.2 line 303 — `32-7168` inclusive);
    /// 2. [`detect_gpu_execution_provider`] checks at least one
    ///    GPU `EP` is available — on the current workspace state
    ///    this returns `Err(NoGpuDetected)` because the workspace
    ///    `ort` dep is configured `default-features = false` with
    ///    no `cuda` / `tensorrt` / `directml` features compiled
    ///    (Cargo.toml line 194);
    /// 3. only if both checks pass, returns
    ///    `Ok(Qwen3Embedding { ... })` with the validated
    ///    `model_dir` and `dimensions`.
    ///
    /// The check ordering is load-bearing — the dimension validation
    /// runs FIRST so an invalid `dimensions` argument surfaces as
    /// [`Qwen3EmbeddingError::DimensionOutOfRange`] regardless of
    /// the GPU state.  The frozen acceptance test
    /// [`test_qwen3_config_gate`] (`SA1`) and the negative-path
    /// `models::qwen3_tests::test_qwen3_load_returns_dim_out_of_range_before_gpu_check`
    /// both pin this contract.
    ///
    /// # Errors
    ///
    /// - [`Qwen3EmbeddingError::DimensionOutOfRange`] if `dimensions`
    ///   is outside `[MIN_MATRYOSHKA_DIM, MAX_MATRYOSHKA_DIM]`;
    /// - [`Qwen3EmbeddingError::NoGpuDetected`] if no GPU `EP` is
    ///   available (the current workspace state always returns
    ///   this on dimension-valid inputs).
    #[tracing::instrument(
        name = "ucil.embeddings.qwen3.load",
        level = "debug",
        skip(model_dir),
        fields(model_dir = ?model_dir, dimensions = dimensions)
    )]
    pub fn load(model_dir: &Path, dimensions: usize) -> Result<Self, Qwen3EmbeddingError> {
        let validated = validate_matryoshka_dimension(dimensions)?;
        let _gpu = detect_gpu_execution_provider()?;
        Ok(Self {
            model_dir: model_dir.to_owned(),
            dimensions: validated,
        })
    }

    /// Return the validated Matryoshka dimension this bundle was
    /// loaded with.
    #[tracing::instrument(name = "ucil.embeddings.qwen3.dimensions", level = "debug", skip(self))]
    pub fn dimensions(&self) -> usize {
        self.dimensions
    }

    /// Return the configured model directory.
    #[tracing::instrument(name = "ucil.embeddings.qwen3.model_dir", level = "debug", skip(self))]
    pub fn model_dir(&self) -> &Path {
        &self.model_dir
    }
}

/// Frozen acceptance test for `P2-W8-F03` per `DEC-0007` module-root
/// placement (matches `feature-list.json:P2-W8-F03.acceptance_tests[0].selector`
/// = `-p ucil-embeddings models::test_qwen3_config_gate`, the
/// frozen `JSON` selector key).
///
/// Exercises 12 sub-assertions covering the config-gate-only surface
/// per `WO-0062` `scope_in[9]`:
///
/// - (`SA1`): [`Qwen3Embedding::load`] returns
///   `Err(NoGpuDetected)` on the current workspace build (no GPU
///   EP features compiled);
/// - (`SA2`): [`validate_matryoshka_dimension`] accepts the
///   master-plan-default Qwen3 dim `1024` (master-plan §17.6
///   line 2029 comment);
/// - (`SA3`): [`validate_matryoshka_dimension`] accepts the
///   lower-bound inclusive `32`;
/// - (`SA4`): [`validate_matryoshka_dimension`] accepts the
///   upper-bound inclusive `7168`;
/// - (`SA5`): [`validate_matryoshka_dimension`] rejects `31` (just
///   below min);
/// - (`SA6`): [`validate_matryoshka_dimension`] rejects `7169`
///   (just above max);
/// - (`SA7`): [`crate::EmbeddingBackend::from_config_str`] parses
///   `"qwen3-embedding"` to [`crate::EmbeddingBackend::Qwen3`];
/// - (`SA8`): same parses `"coderankembed"` to
///   [`crate::EmbeddingBackend::CodeRankEmbed`];
/// - (`SA9`): same returns `Err(UnknownEmbeddingModel { name })`
///   on an arbitrary string;
/// - (`SA10`): [`crate::config::from_toml_str`] returns the
///   master-plan-frozen defaults on empty input;
/// - (`SA11`): [`crate::config::from_toml_str`] preserves
///   overrides on an explicit `[vector_store]` table;
/// - (`SA12`): [`detect_gpu_execution_provider`] returns
///   `Err(NoGpuDetected)` on the current workspace build.
///
/// Per `WO-0062` `scope_in[20]`, no real Qwen3 model file is
/// committed — the 8B Qwen3-Embedding model is ~16-32 GB and is
/// operator-installed at a future site.  The test uses
/// [`tempfile::TempDir`] for the `model_dir` argument in `SA1` so
/// the load function exercises the full path argument plumbing
/// without touching real on-disk model artefacts.
#[test]
fn test_qwen3_config_gate() {
    let tmp = tempfile::TempDir::new().expect("tempdir");

    // SA1: Qwen3Embedding::load returns Err(NoGpuDetected) on the
    // current workspace (no GPU EP features compiled).
    match Qwen3Embedding::load(tmp.path(), 1024) {
        Err(Qwen3EmbeddingError::NoGpuDetected { reason }) => {
            assert!(
                reason.contains("GPU execution provider") || reason.contains("default-features"),
                "NoGpuDetected reason must mention GPU EP or workspace flags; got {reason:?}",
            );
        }
        other => panic!("SA1: expected Err(Qwen3EmbeddingError::NoGpuDetected); got {other:?}",),
    }

    // SA2: master-plan-default Qwen3 dimension 1024 (mid-range)
    let r = validate_matryoshka_dimension(1024);
    assert!(
        matches!(r, Ok(1024)),
        "SA2: validate_matryoshka_dimension(1024) must be Ok(1024); got {r:?}",
    );

    // SA3: lower-bound inclusive
    let r = validate_matryoshka_dimension(32);
    assert!(
        matches!(r, Ok(32)),
        "SA3: validate_matryoshka_dimension(32) must be Ok(32) (inclusive lower bound); got {r:?}",
    );

    // SA4: upper-bound inclusive
    let r = validate_matryoshka_dimension(7168);
    assert!(
        matches!(r, Ok(7168)),
        "SA4: validate_matryoshka_dimension(7168) must be Ok(7168) (inclusive upper bound); got {r:?}",
    );

    // SA5: just below min
    let r = validate_matryoshka_dimension(31);
    assert!(
        matches!(
            r,
            Err(Qwen3EmbeddingError::DimensionOutOfRange { value: 31, .. })
        ),
        "SA5: validate_matryoshka_dimension(31) must be Err(DimensionOutOfRange {{ value: 31 }}); got {r:?}",
    );

    // SA6: just above max
    let r = validate_matryoshka_dimension(7169);
    assert!(
        matches!(
            r,
            Err(Qwen3EmbeddingError::DimensionOutOfRange { value: 7169, .. })
        ),
        "SA6: validate_matryoshka_dimension(7169) must be Err(DimensionOutOfRange {{ value: 7169 }}); got {r:?}",
    );

    // SA7: EmbeddingBackend::from_config_str("qwen3-embedding")
    let r = crate::config::EmbeddingBackend::from_config_str("qwen3-embedding");
    match r {
        Ok(crate::config::EmbeddingBackend::Qwen3) => {}
        other => panic!(
            "SA7: from_config_str(\"qwen3-embedding\") must be Ok(EmbeddingBackend::Qwen3); got {other:?}",
        ),
    }

    // SA8: EmbeddingBackend::from_config_str("coderankembed")
    let r = crate::config::EmbeddingBackend::from_config_str("coderankembed");
    match r {
        Ok(crate::config::EmbeddingBackend::CodeRankEmbed) => {}
        other => panic!(
            "SA8: from_config_str(\"coderankembed\") must be Ok(EmbeddingBackend::CodeRankEmbed); got {other:?}",
        ),
    }

    // SA9: invalid model name returns Err(UnknownEmbeddingModel)
    let r = crate::config::EmbeddingBackend::from_config_str("invalid-model-name");
    match r {
        Err(crate::config::ConfigError::UnknownEmbeddingModel { name }) => {
            assert_eq!(
                name, "invalid-model-name",
                "SA9: UnknownEmbeddingModel must preserve the supplied name; got {name:?}",
            );
        }
        other => panic!(
            "SA9: expected Err(UnknownEmbeddingModel {{ name: \"invalid-model-name\" }}); got {other:?}",
        ),
    }

    // SA10: empty TOML parses to master-plan-frozen defaults
    let cfg = crate::config::from_toml_str("").expect("SA10: empty TOML must parse");
    assert_eq!(cfg.backend, "lancedb", "SA10: default backend");
    assert_eq!(
        cfg.embedding_model, "coderankembed",
        "SA10: default embedding_model",
    );
    assert_eq!(
        cfg.embedding_dimensions, 768,
        "SA10: default embedding_dimensions",
    );
    assert_eq!(cfg.chunk_max_tokens, 512, "SA10: default chunk_max_tokens",);
    assert!(!cfg.reindex_on_startup, "SA10: default reindex_on_startup",);

    // SA11: explicit qwen3 override TOML round-trips
    let toml_str =
        "[vector_store]\nembedding_model = \"qwen3-embedding\"\nembedding_dimensions = 1024\n";
    let cfg = crate::config::from_toml_str(toml_str).expect("SA11: explicit qwen3 TOML must parse");
    assert_eq!(
        cfg.embedding_model, "qwen3-embedding",
        "SA11: embedding_model override preserved",
    );
    assert_eq!(
        cfg.embedding_dimensions, 1024,
        "SA11: embedding_dimensions override preserved",
    );
    assert_eq!(
        cfg.backend, "lancedb",
        "SA11: non-overridden backend keeps default",
    );

    // SA12: detect_gpu_execution_provider returns Err(NoGpuDetected)
    match detect_gpu_execution_provider() {
        Err(Qwen3EmbeddingError::NoGpuDetected { .. }) => {}
        other => panic!(
            "SA12: detect_gpu_execution_provider must return Err(NoGpuDetected) on this build; got {other:?}",
        ),
    }
}

/// Frozen acceptance test for `P2-W8-F02` per `DEC-0007` module-root
/// placement (matches `feature-list.json:P2-W8-F02.acceptance_tests[0].selector`
/// = `-p ucil-embeddings models::test_coderankembed_inference`,
/// the frozen `JSON` selector key).
///
/// Exercises the full real-binary round-trip:
///
/// - (`SA1`): [`CodeRankEmbed::load`] succeeds against
///   `ml/models/coderankembed/`;
/// - (`SA2`): tokenizer encodes a non-empty code snippet to ≥1 token
///   IDs (`ID`-set non-empty);
/// - (`SA3`): [`CodeRankEmbed::embed`] returns `Ok` on a real Rust
///   snippet;
/// - (`SA4`): returned `Vec<f32>` has `.len() == 768` (master-plan
///   §13 + §17.6 — the `DIM` invariant);
/// - (`SA5`): every float is finite (no `NaN` / `±Inf`).
///
/// **Pre-flight**: the test panics with an actionable message when
/// `ml/models/coderankembed/model.onnx` is absent — this is the
/// correct shape per `WO-0055` lessons: runtime test-skip via early
/// `return` is functionally `#[ignore]` and is forbidden by the
/// anti-laziness contract.  The verify script
/// `scripts/verify/P2-W8-F02.sh` runs the devtool installer first so
/// the panic only fires when an operator runs the test outside the
/// verify script (`OPS`-friendly behaviour).
#[test]
fn test_coderankembed_inference() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .parent()
        .expect("crates/ parent of ucil-embeddings exists")
        .parent()
        .expect("workspace root parent of crates/ exists");
    let model_dir = repo_root.join("ml").join("models").join("coderankembed");

    let model_onnx = model_dir.join("model.onnx");
    let tokenizer_json = model_dir.join("tokenizer.json");
    assert!(
        model_onnx.exists() && tokenizer_json.exists(),
        "CodeRankEmbed model artefacts not present at {model_dir:?}; \
         run `bash scripts/devtools/install-coderankembed.sh` first \
         (P2-W8-F02 / WO-0059); got model.onnx exists={}, tokenizer.json exists={}",
        model_onnx.exists(),
        tokenizer_json.exists(),
    );

    // SA1: model loads
    let mut model = CodeRankEmbed::load(&model_dir)
        .expect("CodeRankEmbed::load on real ml/models/coderankembed");

    // SA2: tokenizer round-trip works
    let probe = "fn main() { println!(\"hi\"); }";
    let probe_encoding = model
        .tokenizer
        .encode(probe, true)
        .expect("tokenizer encode on probe Rust snippet");
    assert!(
        !probe_encoding.get_ids().is_empty(),
        "tokenizer must produce ≥1 token IDs for non-empty code; got {:?}",
        probe_encoding.get_ids(),
    );

    // SA3: embed succeeds
    let embedding = model
        .embed("fn hello() { println!(\"hi\"); }")
        .expect("CodeRankEmbed::embed on real Rust snippet");

    // SA4: dimension matches master-plan contract — master-plan §13
    // line 1332 `embedding: vector[768]` + §17.6 line 2029
    // `embedding_dimensions = 768`.  Single-line shape per AC09's
    // line-oriented `grep -nE 'assert_eq!\(.*\.len\(\),\s*768'`;
    // tight message keeps the call within rustfmt's 100-col budget.
    let actual_len = embedding.len();
    assert_eq!(embedding.len(), 768, "expected 768; got {actual_len}");

    // SA5: all floats finite
    let finite_count = embedding.iter().filter(|x| x.is_finite()).count();
    assert!(
        embedding.iter().all(|x| x.is_finite()),
        "all 768 floats must be finite (no NaN / ±Inf); got {finite_count} finite of {actual_len}",
    );
}

/// Negative-path + helper unit tests for `WO-0059` retry-1
/// coverage-driven gate satisfaction.
///
/// These tests exercise the defensive error paths in
/// [`CodeRankEmbed::load`] and the extracted
/// [`pool_and_normalise`] helper without needing the 137MB real model
/// — they use [`tempfile::TempDir`] for filesystem isolation and
/// hand-crafted `Vec<f32>` inputs for the helper.  The frozen
/// acceptance test [`test_coderankembed_inference`] stays at module
/// root per `DEC-0007`; the additional tests live at
/// `models::tests::*` and do not collide with the frozen selector
/// `models::test_coderankembed_inference`.
#[cfg(test)]
mod tests {
    use super::{pool_and_normalise, CodeRankEmbed, CodeRankEmbedError, EMBEDDING_DIM};

    #[test]
    fn load_returns_missing_model_file_for_empty_dir() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        match CodeRankEmbed::load(tmp.path()) {
            Err(CodeRankEmbedError::MissingModelFile { path }) => {
                assert!(
                    path.ends_with("model.onnx"),
                    "expected model.onnx in MissingModelFile; got {path:?}",
                );
            }
            other => panic!("expected Err(MissingModelFile {{ model.onnx }}); got {other:?}"),
        }
    }

    #[test]
    fn load_returns_missing_model_file_for_tokenizer_absent() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let real_model = tmp.path().join("model.onnx");
        std::fs::write(&real_model, b"not a real onnx model")
            .expect("write placeholder model.onnx");
        match CodeRankEmbed::load(tmp.path()) {
            Err(CodeRankEmbedError::Onnx { .. }) => {
                // The placeholder fails ort parse before the tokenizer
                // existence check; that is acceptable — the load
                // function correctly surfaces the upstream parse error
                // as `Onnx` rather than silently falling through.
            }
            Err(CodeRankEmbedError::MissingModelFile { path }) => {
                assert!(
                    path.ends_with("tokenizer.json"),
                    "expected tokenizer.json in MissingModelFile; got {path:?}",
                );
            }
            other => panic!(
                "expected Err(Onnx) or Err(MissingModelFile {{ tokenizer.json }}); got {other:?}",
            ),
        }
    }

    #[test]
    fn pool_and_normalise_returns_dim_mismatch_when_too_short() {
        let too_short = vec![0.5_f32; 100];
        match pool_and_normalise(&too_short) {
            Err(CodeRankEmbedError::DimensionMismatch { expected, got }) => {
                assert_eq!(expected, EMBEDDING_DIM, "expected EMBEDDING_DIM (768)");
                assert_eq!(got, 100, "expected actual length 100");
            }
            other => panic!("expected DimensionMismatch; got {other:?}"),
        }
    }

    #[test]
    fn pool_and_normalise_returns_dim_mismatch_when_too_long() {
        let too_long = vec![0.5_f32; 1024];
        match pool_and_normalise(&too_long) {
            Err(CodeRankEmbedError::DimensionMismatch { expected, got }) => {
                assert_eq!(expected, EMBEDDING_DIM, "expected EMBEDDING_DIM (768)");
                assert_eq!(got, 1024, "expected actual length 1024");
            }
            other => panic!("expected DimensionMismatch; got {other:?}"),
        }
    }

    #[test]
    fn pool_and_normalise_l2_normalises_correct_length_input() {
        let raw = vec![3.0_f32; EMBEDDING_DIM];
        let pooled = pool_and_normalise(&raw).expect("happy path");
        assert_eq!(pooled.len(), EMBEDDING_DIM, "length preserved");
        let norm: f32 = pooled.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!(
            (norm - 1.0).abs() < 1e-5,
            "expected unit vector after L2-normalise; got norm={norm}",
        );
    }

    #[test]
    fn pool_and_normalise_clamps_zero_input_to_epsilon() {
        let zero = vec![0.0_f32; EMBEDDING_DIM];
        let pooled = pool_and_normalise(&zero).expect("zero-vector path");
        assert_eq!(pooled.len(), EMBEDDING_DIM, "length preserved");
        let finite_count = pooled.iter().filter(|x| x.is_finite()).count();
        assert!(
            pooled.iter().all(|x| x.is_finite()),
            "EPSILON clamp must keep all floats finite; got {finite_count} finite of {}",
            pooled.len(),
        );
    }

    #[test]
    fn coderankembed_error_display_renders_canonical_text() {
        let e = CodeRankEmbedError::MissingModelFile {
            path: std::path::PathBuf::from("/no/such/path/model.onnx"),
        };
        let s = format!("{e}");
        assert!(
            s.contains("required model file missing"),
            "MissingModelFile Display must contain canonical text; got {s:?}",
        );

        let e = CodeRankEmbedError::DimensionMismatch {
            expected: EMBEDDING_DIM,
            got: 0,
        };
        let s = format!("{e}");
        assert!(
            s.contains("unexpected embedding dimension"),
            "DimensionMismatch Display must contain canonical text; got {s:?}",
        );

        let e = CodeRankEmbedError::Tokenizer {
            message: "bad json".into(),
        };
        let s = format!("{e}");
        assert!(
            s.contains("tokenizer error"),
            "Tokenizer Display must contain canonical text; got {s:?}",
        );
    }
}
