//! `ucil-embeddings` — embedding inference via ONNX Runtime, chunking, and model management.
//!
//! This `lib.rs` only re-exports public sub-modules; all logic lives in sub-modules.
//!
//! The foundational [`OnnxSession`] (master-plan §18 Phase 2 Week 8 line
//! 1786, `P2-W8-F01`) is the entry point all subsequent W8 features
//! consume — `P2-W8-F02` (`CodeRankEmbed` default model), `P2-W8-F03`
//! (Qwen3-Embedding GPU path), `P2-W8-F05` (chunker + tokenizer
//! pipeline), `P2-W8-F06` (throughput benchmark) all build on top of
//! this session abstraction.
//!
//! [`CodeRankEmbed`] (master-plan §18 Phase 2 Week 8 line 1787,
//! `P2-W8-F02`) is the default CPU embedding model: loads the
//! Int8-quantised `CodeRankEmbed` `ONNX` export + the `HuggingFace`
//! `tokenizer.json` from `ml/models/coderankembed/`, mean-pools +
//! L2-normalises the token-level hidden states, and emits a 768-dim
//! `Vec<f32>` per the master-plan-frozen [`EMBEDDING_DIM`] constant.
//! Downstream features `P2-W8-F03` (Qwen3 GPU upgrade), `P2-W8-F04`
//! (`LanceDB` chunk indexer), `P2-W8-F05` (chunker that produces the
//! `&str` snippet stream), and `P2-W8-F08` (`find_similar` MCP tool)
//! all compose over this primitive.

#![deny(warnings)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod models;
pub mod onnx_inference;
pub use models::{CodeRankEmbed, CodeRankEmbedError, EMBEDDING_DIM};
pub use onnx_inference::{OnnxSession, OnnxSessionError};
