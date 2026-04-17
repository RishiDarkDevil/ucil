//! `ucil-treesitter` — tree-sitter integration: multi-language parser, symbol extraction,
//! AST-aware chunking.
//!
//! This `lib.rs` only re-exports public sub-modules; all logic lives in sub-modules.

#![deny(warnings)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod parser;
pub mod symbols;
pub mod tag_cache;

pub use parser::{Language, ParseError, Parser, SUPPORTED_LANGUAGES};
pub use symbols::{ExtractedSymbol, SymbolExtractor, SymbolKind};
pub use tag_cache::{TagCache, TagCacheError};
