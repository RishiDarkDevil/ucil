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

#![deny(warnings)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod onnx_inference;
pub use onnx_inference::{OnnxSession, OnnxSessionError};
