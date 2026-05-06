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
//!
//! [`EmbeddingChunker`] (master-plan §12.2 line 1339, `P2-W8-F05`)
//! is the real-tokenizer chunker downstream of
//! [`ucil_treesitter::Chunker`].  It parses a source file via
//! tree-sitter, emits AST-aware boundary chunks, then re-tokenizes
//! each chunk with the real `HuggingFace` `BPE` tokenizer and
//! enforces the master-plan-frozen [`MAX_CHUNK_TOKENS`] cap.
//! Oversize chunks collapse to a signature-only fallback per the
//! master-plan §12.2 line 1339 contract.  Consumer wiring is
//! deferred to `P2-W8-F04` (`LanceDB` indexer) and `P2-W8-F08`
//! (`find_similar` `MCP` tool).
//!
//! [`Qwen3Embedding`] (master-plan §4.2 line 303 + §18 Phase 2 Week
//! 8 line 1787, `P2-W8-F03`) is the GPU-only upgrade path.  Loading
//! a [`Qwen3Embedding`] bundle runs two checks in order: the
//! Matryoshka dimension is validated against
//! `[MIN_MATRYOSHKA_DIM, MAX_MATRYOSHKA_DIM]` (32–7168 inclusive),
//! then [`detect_gpu_execution_provider`] probes the loaded `ONNX`
//! Runtime shared library for an available GPU `EP`.  The current
//! workspace `ort` build is `default-features = false`, so the GPU
//! probe returns [`Qwen3EmbeddingError::NoGpuDetected`]
//! unconditionally — that is the EXPECTED behaviour and is what
//! `models::test_qwen3_config_gate` asserts on.  The
//! [`VectorStoreConfig`] / [`EmbeddingBackend`] pair (master-plan
//! §17.6 lines 2026-2030) parses the `[vector_store]` `TOML`
//! section that drives the daemon-side dispatcher between the two
//! models.  Daemon plumbing is deferred to `P2-W8-F04` (`LanceDB`
//! indexer).

#![deny(warnings)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod chunker;
pub mod config;
pub mod models;
pub mod onnx_inference;
pub use chunker::{EmbeddingChunk, EmbeddingChunker, EmbeddingChunkerError, MAX_CHUNK_TOKENS};
pub use config::{ConfigError, EmbeddingBackend, VectorStoreConfig};
pub use models::{
    detect_gpu_execution_provider, validate_matryoshka_dimension, CodeRankEmbed,
    CodeRankEmbedError, GpuKind, Qwen3Embedding, Qwen3EmbeddingError, EMBEDDING_DIM,
    MAX_MATRYOSHKA_DIM, MIN_MATRYOSHKA_DIM,
};
pub use onnx_inference::{OnnxSession, OnnxSessionError};
