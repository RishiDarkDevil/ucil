//! AST-aware code chunker for UCIL's two-tier retrieval system.
//!
//! A [`Chunker`] converts a parse tree into a flat list of [`Chunk`]s.
//! Each chunk spans exactly one symbol extracted by [`SymbolExtractor`].
//! Oversized symbols (> 512 approximate tokens) are truncated to ≤ 2 048
//! characters so that downstream embedding models never receive a token
//! budget violation.
//!
//! # Token counting
//!
//! Tokens are approximated as `chars / 4`.  This matches the rough average
//! for GPT-family tokenizers on English-heavy source code and avoids
//! pulling in a full tokenizer at Phase 1.  Exact tiktoken-based counting
//! is deferred to Phase 2 when `ucil-embeddings` lands.

// Module name matches the public types (ChunkError → chunker, etc.).
#![allow(clippy::module_name_repetitions)]

use thiserror::Error;
use tree_sitter::Tree;

use crate::{
    parser::Language,
    symbols::{SymbolExtractor, SymbolKind},
};

// ── Maximum chunk size ─────────────────────────────────────────────────────

/// Maximum number of approximate tokens per chunk.
const MAX_TOKENS: u32 = 512;
/// Maximum characters per chunk (= `MAX_TOKENS * 4`).
const MAX_CHARS: usize = 2048;

// ── Error type ─────────────────────────────────────────────────────────────

/// Errors returned by [`Chunker::chunk`].
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ChunkError {
    /// The caller did not supply a parse tree for the given source.
    ///
    /// This variant is reserved for future API variants that accept raw
    /// source strings and need to report a missing/failed parse.
    #[error("a parse tree is required but was absent: {reason}")]
    ParseRequired {
        /// Human-readable explanation of why the parse tree was needed.
        reason: String,
    },
}

// ── Output type ────────────────────────────────────────────────────────────

/// A single AST-aware code chunk ready for embedding and storage.
///
/// Each chunk corresponds to exactly one named symbol (function, class,
/// struct, …) and carries enough metadata to reconstruct its provenance.
#[derive(Debug, Clone)]
pub struct Chunk {
    /// Unique chunk identifier in the format `{file_path}:{start_line}:{end_line}`.
    pub id: String,
    /// Relative or absolute path of the source file this chunk came from.
    pub file_path: String,
    /// Zero-based row of the first character of the symbol node.
    pub start_line: u32,
    /// Zero-based row of the last character of the symbol node.
    pub end_line: u32,
    /// Source text of the symbol, possibly truncated if the original exceeds
    /// [`MAX_CHARS`].
    pub content: String,
    /// Lower-case name of the programming language (e.g. `"rust"`, `"python"`).
    pub language: String,
    /// The declared name of the symbol (e.g. `"my_func"`, `"MyStruct"`).
    pub symbol_name: String,
    /// String representation of the [`SymbolKind`] (e.g. `"function"`, `"class"`).
    pub symbol_kind: String,
    /// Approximate token count: `content.chars().count() / 4`.
    /// Always `≤ MAX_TOKENS` (= 512) after truncation.
    pub token_count: u32,
}

// ── Public entry-point ─────────────────────────────────────────────────────

/// Stateless AST-aware chunker.
pub struct Chunker;

impl Chunker {
    /// Decompose `source` / `tree` into a [`Vec<Chunk>`], one chunk per symbol.
    ///
    /// * Symbols are extracted via [`SymbolExtractor::extract`].
    /// * Content is the source lines spanning each symbol's node.
    /// * Chunks that exceed [`MAX_TOKENS`] approximate tokens are truncated to
    ///   [`MAX_CHARS`] characters.
    /// * An empty source or a source with no recognisable symbols yields an
    ///   empty `Vec` (not an error).
    ///
    /// # Errors
    ///
    /// Currently always returns `Ok`.  The [`ChunkError::ParseRequired`]
    /// variant is reserved for a future overload that accepts raw source text.
    ///
    /// # Examples
    ///
    /// ```
    /// use ucil_treesitter::parser::{Language, Parser};
    /// use ucil_treesitter::chunker::Chunker;
    ///
    /// let mut p = Parser::new();
    /// let src = "fn main() {}";
    /// let tree = p.parse(src, Language::Rust).unwrap();
    /// let chunks = Chunker::chunk(&tree, src, &Language::Rust, "src/main.rs").unwrap();
    /// assert_eq!(chunks.len(), 1);
    /// assert_eq!(chunks[0].symbol_name, "main");
    /// ```
    pub fn chunk(
        tree: &Tree,
        source: &str,
        lang: &Language,
        file_path: &str,
    ) -> Result<Vec<Chunk>, ChunkError> {
        let symbols = SymbolExtractor::extract(tree, source, lang);
        if symbols.is_empty() {
            return Ok(Vec::new());
        }

        // Pre-split source into lines for O(symbols) content extraction.
        let lines: Vec<&str> = source.lines().collect();
        let lang_str = lang_to_str(lang);

        let mut chunks = Vec::with_capacity(symbols.len());

        for sym in symbols {
            let start = sym.start_line as usize;
            // end_line is inclusive and zero-based; slice end is exclusive.
            let end = (sym.end_line as usize + 1).min(lines.len());
            let raw_content = if start < end {
                lines[start..end].join("\n")
            } else {
                String::new()
            };

            // Approximate token count before truncation.
            let raw_tokens = (raw_content.chars().count() as u32).saturating_div(4);

            let (content, token_count) = if raw_tokens > MAX_TOKENS {
                let truncated: String = raw_content.chars().take(MAX_CHARS).collect();
                let tc = (truncated.chars().count() as u32).saturating_div(4);
                (truncated, tc)
            } else {
                (raw_content, raw_tokens)
            };

            let id = format!("{}:{}:{}", file_path, sym.start_line, sym.end_line);

            chunks.push(Chunk {
                id,
                file_path: file_path.to_owned(),
                start_line: sym.start_line,
                end_line: sym.end_line,
                content,
                language: lang_str.to_owned(),
                symbol_name: sym.name,
                symbol_kind: symbol_kind_to_str(sym.kind).to_owned(),
                token_count,
            });
        }

        Ok(chunks)
    }
}

// ── Conversion helpers ─────────────────────────────────────────────────────

fn lang_to_str(lang: &Language) -> &'static str {
    match lang {
        Language::Rust => "rust",
        Language::Python => "python",
        Language::TypeScript => "typescript",
        Language::JavaScript => "javascript",
        Language::Go => "go",
        Language::Java => "java",
        Language::C => "c",
        Language::Cpp => "cpp",
        Language::Ruby => "ruby",
        Language::Bash => "bash",
        Language::Json => "json",
    }
}

fn symbol_kind_to_str(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Function => "function",
        SymbolKind::Class => "class",
        SymbolKind::Method => "method",
        SymbolKind::Struct => "struct",
        SymbolKind::Enum => "enum",
        SymbolKind::Trait => "trait",
        SymbolKind::Const => "const",
        SymbolKind::TypeAlias => "type_alias",
        SymbolKind::Module => "module",
        SymbolKind::Interface => "interface",
        SymbolKind::Constructor => "constructor",
        SymbolKind::Field => "field",
    }
}

// ── Unit tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;

    fn chunk_src(src: &str, lang: Language, path: &str) -> Vec<Chunk> {
        let mut p = Parser::new();
        let tree = p.parse(src, lang).expect("parse must succeed");
        Chunker::chunk(&tree, src, &lang, path).expect("chunk must succeed")
    }

    /// Two short Rust functions must each produce one chunk ≤ 512 tokens.
    #[test]
    fn chunk_rust_two_functions() {
        let src = "fn foo() {}\nfn bar(x: i32) -> i32 { x + 1 }\n";
        let chunks = chunk_src(src, Language::Rust, "src/lib.rs");
        assert_eq!(chunks.len(), 2, "expected 2 chunks, got {:?}", chunks.len());
        for c in &chunks {
            assert!(
                c.token_count <= MAX_TOKENS,
                "chunk '{}' has token_count {} > {}",
                c.symbol_name,
                c.token_count,
                MAX_TOKENS
            );
        }
    }

    /// A function whose body exceeds MAX_CHARS must be truncated so that
    /// its `token_count` stays ≤ MAX_TOKENS and `content.len() ≤ MAX_CHARS`.
    #[test]
    fn chunk_oversized_function() {
        // Build a Rust function body that is clearly > 2048 chars.
        // Each line is "    let _x = 42;\n" = 18 chars; 200 lines = 3600 chars.
        let body: String = "    let _x = 42;\n".repeat(200);
        let src = format!("fn big_func() {{\n{}}}\n", body);
        assert!(
            src.len() > MAX_CHARS,
            "test fixture must be oversized (len={})",
            src.len()
        );
        let chunks = chunk_src(&src, Language::Rust, "src/big.rs");
        assert_eq!(chunks.len(), 1, "expected exactly 1 chunk");
        let c = &chunks[0];
        assert!(
            c.token_count <= MAX_TOKENS,
            "oversized chunk must be truncated to ≤{MAX_TOKENS} tokens, got {}",
            c.token_count
        );
        assert!(
            c.content.chars().count() <= MAX_CHARS,
            "content.chars().count() must be ≤{MAX_CHARS}, got {}",
            c.content.chars().count()
        );
    }

    /// Empty source must return an empty `Vec` without error.
    #[test]
    fn chunk_empty_source() {
        let chunks = chunk_src("", Language::Rust, "src/empty.rs");
        assert!(
            chunks.is_empty(),
            "empty source must yield no chunks, got {chunks:?}"
        );
    }

    /// Every chunk's `id` must follow the `{file_path}:{start_line}:{end_line}` format.
    #[test]
    fn chunk_ids_match_expected_format() {
        let src = "fn hello() {}\n";
        let path = "src/hello.rs";
        let chunks = chunk_src(src, Language::Rust, path);
        assert_eq!(chunks.len(), 1, "expected 1 chunk");
        let c = &chunks[0];
        let expected_id = format!("{}:{}:{}", path, c.start_line, c.end_line);
        assert_eq!(
            c.id, expected_id,
            "chunk id must be '{path}:start_line:end_line'"
        );
        // Also verify the format string components are correct types.
        assert_eq!(c.file_path, path);
    }
}
