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
/// (chunker output validation), and `P2-W8-F08` (`find_similar` MCP
/// tool dimension assertion).
pub const EMBEDDING_DIM: usize = 768;

/// Errors emitted by [`CodeRankEmbed`] operations.
///
/// Variants are `#[non_exhaustive]` per `.claude/rules/rust-style.md`
/// so that future additions (e.g. a `BatchSizeExceeded` for batched
/// inference in a Phase-3 WO) do not break downstream `match`
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
    /// in `WO-0059` `forbidden_paths`).
    #[error("ort session error: {source}")]
    Onnx {
        /// The underlying `ort::Error`.
        #[from]
        source: ort::Error,
    },

    /// Shape-construction error from `ndarray::Array2::from_shape_vec`.
    /// The literal shape passed by [`CodeRankEmbed::embed`] always
    /// matches the input slice length, but the upstream API is
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
    /// `String` insulates this crate's public API from
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
    /// "operator forgot to install" condition.
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
/// non-duplicable runtime resources (CPU execution-provider arena,
/// `OS` handles for the `download-binaries` shared library);
/// consumers needing shared inference must wrap in
/// `Arc<Mutex<CodeRankEmbed>>` because [`CodeRankEmbed::embed`]
/// takes `&mut self` (the upstream `ort 2.x` `Session::run`
/// signature).
///
/// **`Send`** — both `ort::session::Session` and
/// `tokenizers::Tokenizer` are `Send`, so a `CodeRankEmbed` can be
/// moved into a `tokio::task::spawn_blocking` closure for async wrap
/// at the `P2-W8-F04` / `P2-W8-F08` consumer sites.
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
    /// `model_dir` MUST contain two artefacts (laid down by
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
    /// `sentence_embedding` output, L2-normalise, and return a
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
    /// L2-normalises it so downstream cosine-similarity search at
    /// `P2-W8-F08` reduces to a dot product.  The Euclidean norm is
    /// clamped to `f32::EPSILON` to avoid `NaN` on a degenerate
    /// all-zero output.  When the model output's flat length is not
    /// exactly [`EMBEDDING_DIM`] the function returns
    /// [`CodeRankEmbedError::DimensionMismatch`] — this is a
    /// model/tokenizer mismatch (e.g. swapped for a model with a
    /// different head dimension) and is surfaced as an actionable
    /// error rather than a silent garbage-shaped vector.
    ///
    /// `attention_mask` is constructed as a tensor of `i64` `1`s with
    /// the same shape as `input_ids` — the tokenizer does not pad
    /// (single-snippet inference, no batching), so every position is
    /// attended.  When `P2-W8-F05` (chunker) lands and feeds batched
    /// inputs, the mask will need 0-padding for the right tail; the
    /// mask construction is centralised here in anticipation.
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

        if raw.len() != EMBEDDING_DIM {
            return Err(CodeRankEmbedError::DimensionMismatch {
                expected: EMBEDDING_DIM,
                got: raw.len(),
            });
        }
        let mut pooled = raw;

        let norm_sq: f32 = pooled.iter().map(|x| x * x).sum();
        let norm = norm_sq.sqrt().max(f32::EPSILON);
        for p in &mut pooled {
            *p /= norm;
        }

        if pooled.len() != EMBEDDING_DIM {
            return Err(CodeRankEmbedError::DimensionMismatch {
                expected: EMBEDDING_DIM,
                got: pooled.len(),
            });
        }
        Ok(pooled)
    }
}

/// Frozen acceptance test for `P2-W8-F02` per `DEC-0007` module-root
/// placement (matches `feature-list.json:P2-W8-F02.acceptance_tests[0].selector`
/// = `-p ucil-embeddings models::test_coderankembed_inference`).
///
/// Exercises the full real-binary round-trip:
///
/// - **SA1**: [`CodeRankEmbed::load`] succeeds against
///   `ml/models/coderankembed/`;
/// - **SA2**: tokenizer encodes a non-empty code snippet to ≥1 token IDs;
/// - **SA3**: [`CodeRankEmbed::embed`] returns `Ok` on a real Rust
///   snippet;
/// - **SA4**: returned `Vec<f32>` has `.len() == 768` (master-plan
///   §13 + §17.6);
/// - **SA5**: every float is finite (no `NaN` / `±Inf`).
///
/// **Pre-flight**: the test panics with an actionable message when
/// `ml/models/coderankembed/model.onnx` is absent — this is the
/// correct shape per `WO-0055` lessons: runtime test-skip via early
/// `return` is functionally `#[ignore]` and is forbidden by the
/// anti-laziness contract.  The verify script
/// `scripts/verify/P2-W8-F02.sh` runs the devtool installer first so
/// the panic only fires when an operator runs the test outside the
/// verify script.
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
