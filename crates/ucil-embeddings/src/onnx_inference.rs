//! ONNX Runtime session abstraction — the foundational embedding-inference primitive.
//!
//! Master-plan §18 Phase 2 Week 8 line 1786 specifies "ucil-embeddings crate:
//! ONNX Runtime (`ort` crate) inference"; this module lands the foundational
//! [`OnnxSession`] type that all subsequent W8 features build on top of:
//!
//! - `P2-W8-F02` — `CodeRankEmbed` default model (`OnnxSession::from_path` over
//!   the downloaded model artefact);
//! - `P2-W8-F03` — Qwen3-Embedding GPU upgrade path (adds `cuda` /
//!   `tensorrt` execution-provider wiring on top of [`OnnxSession`]);
//! - `P2-W8-F05` — chunker + tokenizer pipeline (consumes
//!   [`OnnxSession::infer`] for embedding production);
//! - `P2-W8-F06` — throughput benchmark (drives [`OnnxSession::infer`]
//!   under load).
//!
//! ONNX Runtime is sync-by-default — `ort::session::Session::run` takes
//! `&mut self`, so [`OnnxSession::infer`] mirrors that contract. The
//! natural async wrap for the daemon is
//! `tokio::task::spawn_blocking(move || session.infer(&token_ids))` and
//! lands at the F02 / F05 consumer site where the `tokio` runtime is in
//! play. F01 keeps the API sync so the test runs as a plain `#[test]` and
//! the surface is testable without a runtime.
//!
//! The struct caches the input / output name lists at load time so
//! callers can introspect tensor metadata without re-querying the
//! underlying session on every call (see [`OnnxSession::input_names`] /
//! [`OnnxSession::output_names`]).
//!
//! The `CodeRankEmbed` model download itself is intentionally deferred to
//! `P2-W8-F02`; F01 ships a tiny synthetic `minimal.onnx` fixture
//! (`tests/data/minimal.onnx`) for the round-trip test only.

use std::path::Path;

use ndarray::Array2;
use ort::session::Session;
use ort::value::Tensor;

/// Errors emitted by [`OnnxSession`] operations.
///
/// Variants are `#[non_exhaustive]` so additional cases (e.g. dtype
/// mismatch on a future typed-output API) can be added without breaking
/// downstream `match` exhaustiveness.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum OnnxSessionError {
    /// Wraps an `ort` runtime error — covers session creation,
    /// `commit_from_file` parse / IO failures, and `Session::run`
    /// errors via the auto-conversion `?`.
    #[error("ort session error: {source}")]
    Ort {
        /// The underlying `ort::Error`.
        #[from]
        source: ort::Error,
    },

    /// Shape-construction error from `ndarray::Array2::from_shape_vec`.
    /// The literal shape passed by [`OnnxSession::infer`] always matches
    /// the input slice length, but the upstream API is fallible so the
    /// variant is included for defensiveness.
    #[error("ndarray shape error: {source}")]
    Ndarray {
        /// The underlying `ndarray::ShapeError`.
        #[from]
        source: ndarray::ShapeError,
    },

    /// Filesystem error while reading the model bytes — surfaced for
    /// callers that want to distinguish missing files from corrupt
    /// models.
    #[error("io error reading model file: {source}")]
    Io {
        /// The underlying `std::io::Error`.
        #[from]
        source: std::io::Error,
    },

    /// The loaded model declares no input matching the expected name
    /// (or has no inputs at all when [`OnnxSession::infer`] tried to
    /// pick the first one).
    #[error("model has no input named {name:?}")]
    MissingInput {
        /// The name that was looked up.
        name: String,
    },

    /// The loaded model declares no output matching the expected name
    /// (or has no outputs at all).
    #[error("model has no output named {name:?}")]
    MissingOutput {
        /// The name that was looked up.
        name: String,
    },
}

/// A loaded ONNX Runtime session plus a cache of its declared input /
/// output tensor names.
///
/// Holding the names alongside the [`Session`] lets callers introspect
/// model metadata (e.g. picking the first input for tokenised IDs)
/// without paying the FFI round-trip cost on every `infer` call.
///
/// Not `Clone` — the underlying `ort::session::Session` owns runtime
/// resources (a CPU execution-provider arena, OS handles for the
/// `download-binaries` shared library) which are not safe to duplicate.
/// Consumers that need shared ownership should wrap in `Arc<Mutex<_>>`
/// (mutation is required because [`Session::run`] takes `&mut self`).
#[derive(Debug)]
pub struct OnnxSession {
    session: Session,
    input_names: Vec<String>,
    output_names: Vec<String>,
}

impl OnnxSession {
    /// Load an ONNX model from disk and construct a session ready for
    /// [`infer`](Self::infer) calls.
    ///
    /// `model_path` is forwarded to
    /// `ort::session::Session::builder().commit_from_file(...)` which
    /// (a) memory-maps the model bytes, (b) parses the protobuf graph,
    /// (c) wires the default CPU execution provider. The session
    /// stays bound to the workspace `ort = "=2.0.0-rc.12"` ABI; cross
    /// the `ort` major-version boundary requires a session re-load.
    ///
    /// # Errors
    ///
    /// - [`OnnxSessionError::Ort`] if the file is missing, the
    ///   protobuf is malformed, or the ONNX graph references an
    ///   unsupported operator for the linked-in runtime version.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::path::Path;
    /// use ucil_embeddings::OnnxSession;
    ///
    /// let session = OnnxSession::from_path(Path::new("model.onnx"))
    ///     .expect("model load");
    /// assert!(!session.input_names().is_empty());
    /// ```
    #[tracing::instrument(
        name = "ucil.embeddings.onnx_session.load",
        level = "debug",
        skip(model_path),
        fields(model_path = ?model_path)
    )]
    pub fn from_path(model_path: &Path) -> Result<Self, OnnxSessionError> {
        let session = Session::builder()?.commit_from_file(model_path)?;
        let input_names: Vec<String> = session
            .inputs()
            .iter()
            .map(|input| input.name().to_owned())
            .collect();
        let output_names: Vec<String> = session
            .outputs()
            .iter()
            .map(|output| output.name().to_owned())
            .collect();
        Ok(Self {
            session,
            input_names,
            output_names,
        })
    }

    /// Return the cached list of declared input tensor names in
    /// graph-declaration order. The slice is populated once at
    /// [`from_path`](Self::from_path) and never re-queried.
    #[must_use]
    pub fn input_names(&self) -> &[String] {
        &self.input_names
    }

    /// Return the cached list of declared output tensor names in
    /// graph-declaration order.
    #[must_use]
    pub fn output_names(&self) -> &[String] {
        &self.output_names
    }

    /// Run inference for a single batch row of `token_ids` and return
    /// the first output tensor flattened into a `Vec<f32>`.
    ///
    /// Wraps the input slice into a 2-D `[1, token_ids.len()]` tensor
    /// (single-batch row vector) via `ndarray::Array2::from_shape_vec`
    /// — defensive-only since the literal shape always matches, but the
    /// ndarray API is fallible. The tensor is then handed to
    /// `ort::value::Tensor::from_array` as a `(shape, vec)` pair so the
    /// data is owned by `ort` for the duration of the call.
    ///
    /// `&mut self` mirrors the upstream
    /// `ort::session::Session::run(&mut self, ...)` signature; consumers
    /// that need shared inference must wrap in `Mutex` or serialise via
    /// a channel.
    ///
    /// # Errors
    ///
    /// - [`OnnxSessionError::Ndarray`] if the shape construction fails;
    /// - [`OnnxSessionError::MissingInput`] if the model declares no
    ///   inputs (degenerate graph);
    /// - [`OnnxSessionError::MissingOutput`] if the model declares no
    ///   outputs (degenerate graph);
    /// - [`OnnxSessionError::Ort`] if the session run itself fails or
    ///   the output tensor's element type is not `f32`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use std::path::Path;
    /// # use ucil_embeddings::OnnxSession;
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut session = OnnxSession::from_path(Path::new("model.onnx"))?;
    /// let embedding: Vec<f32> = session.infer(&[1i64, 2, 3])?;
    /// assert!(!embedding.is_empty());
    /// # Ok(())
    /// # }
    /// ```
    #[tracing::instrument(
        name = "ucil.embeddings.onnx_session.infer",
        level = "debug",
        skip(self, token_ids),
        fields(token_count = token_ids.len())
    )]
    pub fn infer(&mut self, token_ids: &[i64]) -> Result<Vec<f32>, OnnxSessionError> {
        let array: Array2<i64> = Array2::from_shape_vec((1, token_ids.len()), token_ids.to_vec())?;
        let input_name = self
            .input_names
            .first()
            .ok_or_else(|| OnnxSessionError::MissingInput {
                name: String::from("<first>"),
            })?
            .clone();
        let output_name = self
            .output_names
            .first()
            .ok_or_else(|| OnnxSessionError::MissingOutput {
                name: String::from("<first>"),
            })?
            .clone();
        let shape = [array.shape()[0], array.shape()[1]];
        let (data, _offset) = array.into_raw_vec_and_offset();
        let tensor = Tensor::<i64>::from_array((shape, data))?;
        let outputs = self
            .session
            .run(ort::inputs![input_name.as_str() => tensor])?;
        let output_value =
            outputs
                .get(output_name.as_str())
                .ok_or_else(|| OnnxSessionError::MissingOutput {
                    name: output_name.clone(),
                })?;
        let (_shape, slice) = output_value.try_extract_tensor::<f32>()?;
        Ok(slice.to_vec())
    }
}

#[test]
fn test_onnx_session_loads_minimal_model() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let model_path = manifest_dir.join("tests").join("data").join("minimal.onnx");

    let mut session =
        OnnxSession::from_path(&model_path).expect("OnnxSession::from_path on minimal model");

    let inputs = session.input_names();
    assert!(
        !inputs.is_empty(),
        "minimal model must declare at least one input; got {inputs:?}",
    );
    assert_eq!(
        inputs[0], "input_ids",
        "first input name must match the minimal model contract; got {:?}",
        inputs[0]
    );

    let outputs = session.output_names();
    assert!(
        !outputs.is_empty(),
        "minimal model must declare at least one output; got {outputs:?}",
    );

    let result = session
        .infer(&[1i64, 2, 3])
        .expect("infer on minimal model");
    let result_len = result.len();
    assert!(
        !result.is_empty(),
        "infer must produce a non-empty Vec<f32>; got len={result_len}",
    );
    assert_eq!(
        result_len, 3,
        "minimal model with Cast over [1,3] returns 3 floats; got len={result_len}",
    );
}
