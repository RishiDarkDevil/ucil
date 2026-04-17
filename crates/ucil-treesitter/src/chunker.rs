//! AST-aware source-code chunker.
//!
//! [`Chunker`] turns a parsed [`tree_sitter::Tree`] into a `Vec<Chunk>`
//! suitable for embedding + retrieval.  Each chunk holds a *complete*
//! function, method, or class (per master plan §12.2 line 1339 — "AST-aware
//! via tree-sitter. Each chunk is a complete function/method/class. Never
//! split mid-function. Max 512 tokens. Larger functions: signature +
//! first-paragraph doc comment.").
//!
//! This module implements feature `P1-W2-F03` — master plan §18 Phase 1
//! Week 2 line 1734 ("Implement ucil-treesitter: multi-language parser,
//! symbol extraction, AST-aware chunking") — and sits at
//! `crates/ucil-treesitter/src/chunker.rs` per the directory layout in
//! master plan §14 line 1648 (`lib.rs`, `parser.rs`, `symbols.rs`,
//! `chunker.rs`, `tag_cache.rs`).  The sibling Phase 3 chunker in
//! `ucil-embeddings` (master plan §14 line 1657) is where a real
//! tokenizer lands; this module stays deliberately simple.
//!
//! # Chunk shape
//!
//! [`Chunk`] mirrors the `code_chunks_schema` in master plan §12.2 line
//! 1325: `id`, `file_path`, `language`, `start_line`, `end_line`,
//! `content`, `symbol_name`, `symbol_kind`, `token_count`.  `id` is
//! composed as `"{file_path}:{start_line}:{end_line}"` and every emitted
//! chunk satisfies `token_count <= MAX_TOKENS` (the `chunk_max_tokens =
//! 512` cap from master plan §12.4 line 2030, exposed as [`MAX_TOKENS`]).
//!
//! # Chunking strategy
//!
//! For every [`Language`] covered by [`SymbolExtractor`] (Rust, Python,
//! TypeScript, JavaScript, Go) the chunker requests an extracted-symbol
//! list and emits one chunk per `Function / Method / Class / Struct /
//! Enum / Trait / Interface`.  Class chunks AND their nested method
//! chunks are both emitted — overlap is intentional per the master-plan
//! phrasing "complete function/method/class" (a caller who wants a flat
//! list of methods can filter by kind; a caller who wants the enclosing
//! class still receives a cohesive chunk).
//!
//! For fallback languages (Java, C, C++, Ruby, Bash, JSON) the chunker
//! walks the parse tree's top-level *named* children and emits one
//! chunk per child whose kind is not in a small deny-list (stray
//! `comment` / `line_comment` / `block_comment` / `*_list` wrapper
//! nodes).  `symbol_name` and `symbol_kind` are `None` on the
//! fallback-language path.
//!
//! # Oversize handling
//!
//! When a chunk's byte-estimated `token_count` would exceed
//! [`MAX_TOKENS`], the chunker collapses it to a signature-only chunk:
//! the function / method's signature line (captured by
//! [`SymbolExtractor`] via [`ExtractedSymbol::signature`]) followed,
//! when present, by the first paragraph of its
//! [`ExtractedSymbol::doc_comment`].  Sliding-window splitting is
//! deliberately NOT implemented — master plan §12.2 line 1339 prescribes
//! signature + first-paragraph doc comment as the oversize policy; the
//! real-tokenizer chunker of `ucil-embeddings` (master plan §14 line
//! 1657, Phase 3) is where sliding-window would land if a future ADR
//! motivates it.
//!
//! # Token-count heuristic
//!
//! `token_count = max(1, ⌈content.len() / 4⌉)` — OpenAI's standard
//! English-text byte heuristic, exposed via [`BYTES_PER_TOKEN`] inside
//! the module (private).  This is intentional; the real tokenizer is
//! scoped to `ucil-embeddings` in Phase 3 (master plan §14 line 1657).
//! A byte-based estimate is sufficient to enforce the "no chunk larger
//! than 512 tokens" invariant at this layer — any minor drift between
//! the estimate and a real tokenizer is absorbed by the downstream
//! embedder's own chunk-boundary-respecting tokenizer.
//!
//! # Tracing
//!
//! [`Chunker::chunk`] opens a `tracing` span `ucil.treesitter.chunk` at
//! `DEBUG` level per master plan §15.2 (`ucil.<layer>.<op>` naming
//! convention).

// `Chunker` / `ChunkError` / `Chunk` intentionally share a name prefix
// with the module ("chunker" → "Chunker" / "Chunk" / "ChunkError");
// suppress the pedantic lint, mirroring the escape used by `parser.rs`,
// `symbols.rs`, and `tag_cache.rs`.
#![allow(clippy::module_name_repetitions)]

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tree_sitter::{Node, Tree};

use crate::parser::Language;
use crate::symbols::{ExtractedSymbol, SymbolExtractor, SymbolKind};

// ── Constants ──────────────────────────────────────────────────────────────

/// Authoritative cap on a single chunk's `token_count` — master plan §12.4
/// line 2030 (`chunk_max_tokens = 512`).
///
/// Every [`Chunk`] emitted by [`Chunker::chunk`] satisfies
/// `chunk.token_count <= MAX_TOKENS`.
pub const MAX_TOKENS: u32 = 512;

/// Byte-per-token estimator coefficient.
///
/// `token_count = max(1, ⌈content.len() / BYTES_PER_TOKEN⌉)` — OpenAI's
/// standard English-text heuristic.  The real tokenizer is owned by
/// `ucil-embeddings` (Phase 3, master plan §14 line 1657); this constant
/// is intentionally private to the module so downstream code does NOT
/// build on the estimate and silently diverge from a real tokenizer.
const BYTES_PER_TOKEN: usize = 4;

// ── Errors ─────────────────────────────────────────────────────────────────

/// Failures surfaced by [`Chunker::chunk`].
///
/// Marked `#[non_exhaustive]` so future variants (e.g. a "symbol
/// extractor query failed" arm) can be added without a semver break.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ChunkError {
    /// A symbol's extracted line range was malformed — `end_line`
    /// preceded `start_line`, or `start_line == 0`.  Should not happen
    /// for trees produced by [`crate::parser::Parser`] +
    /// [`SymbolExtractor`], but the variant keeps callers from reaching
    /// for `unreachable!()` when pattern-matching defensively.
    #[error("invalid line range: start={start} end={end}")]
    InvalidLineRange {
        /// Reported start line (1-based).
        start: u32,
        /// Reported end line (1-based).
        end: u32,
    },

    /// A byte-range slice landed mid-codepoint — not expected in the
    /// current line-based implementation (which slices on `&str`
    /// boundaries only), but retained as a variant so the error enum is
    /// stable should a future byte-addressable path reinstate the risk.
    #[error("UTF-8 boundary error: {0}")]
    Utf8Boundary(#[from] std::str::Utf8Error),
}

// ── Language (de)serialization helper ──────────────────────────────────────
//
// `symbols.rs` (WO-0017) already defines a `mod language_serde` that maps
// a `Language` to / from a lowercase string tag, but it is private to
// that module and `symbols.rs` is frozen byte-for-byte by this
// work-order's acceptance criteria — so we DUPLICATE the 40-line adapter
// here rather than promote the existing one.  The two modules stay in
// lock-step because both cover every variant of the
// `#[non_exhaustive]` [`Language`] enum.

mod language_serde {
    use serde::{Deserialize as _, Deserializer, Serializer};

    use crate::parser::Language;

    const KNOWN: &[&str] = &[
        "rust",
        "python",
        "typescript",
        "javascript",
        "go",
        "java",
        "c",
        "cpp",
        "ruby",
        "bash",
        "json",
    ];

    // Serde's `#[serde(with = "…")]` contract fixes the signature as
    // `fn(&T, S) -> …`, so we cannot take `Language` by value here.
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub fn serialize<S>(lang: &Language, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(tag_of(*lang))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Language, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.as_str() {
            "rust" => Ok(Language::Rust),
            "python" => Ok(Language::Python),
            "typescript" => Ok(Language::TypeScript),
            "javascript" => Ok(Language::JavaScript),
            "go" => Ok(Language::Go),
            "java" => Ok(Language::Java),
            "c" => Ok(Language::C),
            "cpp" => Ok(Language::Cpp),
            "ruby" => Ok(Language::Ruby),
            "bash" => Ok(Language::Bash),
            "json" => Ok(Language::Json),
            _ => Err(serde::de::Error::unknown_variant(s.as_str(), KNOWN)),
        }
    }

    const fn tag_of(lang: Language) -> &'static str {
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
}

// ── Chunk ──────────────────────────────────────────────────────────────────

/// A single AST-aware chunk of source code.
///
/// Mirrors the `code_chunks_schema` in master plan §12.2 line 1325:
/// `id`, `file_path`, `language`, `start_line`, `end_line`, `content`,
/// `symbol_name`, `symbol_kind`, `token_count`.
///
/// # Invariants
///
/// - `id == format!("{}:{}:{}", file_path.display(), start_line, end_line)`
/// - `start_line >= 1 && end_line >= start_line`
/// - `token_count <= MAX_TOKENS`
/// - `token_count == max(1, ⌈content.len() / 4⌉)`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chunk {
    /// Unique chunk id — `"{file_path}:{start_line}:{end_line}"`.
    pub id: String,
    /// Path of the source file this chunk was extracted from.
    pub file_path: PathBuf,
    /// Language the source was parsed as.
    #[serde(with = "language_serde")]
    pub language: Language,
    /// 1-based line number of the chunk's first line.
    pub start_line: u32,
    /// 1-based line number of the chunk's last line.
    pub end_line: u32,
    /// The chunk's source text — either the full span or, for oversized
    /// functions, a signature-only replacement (master plan §12.2 line
    /// 1339).
    pub content: String,
    /// Name of the underlying symbol (`Some(_)` on the symbol-based
    /// path; `None` on the fallback-language path).
    pub symbol_name: Option<String>,
    /// Kind of the underlying symbol (`Some(_)` on the symbol-based
    /// path; `None` on the fallback-language path).
    pub symbol_kind: Option<SymbolKind>,
    /// Byte-estimated token count — `max(1, ⌈content.len() / 4⌉)`.
    pub token_count: u32,
}

// ── Chunker ────────────────────────────────────────────────────────────────

/// Stateless AST-aware chunker.
///
/// Calls are cheap and may be issued concurrently.  Construct via
/// [`Chunker::new`] (or [`Chunker::default`]) and call
/// [`chunk`][Self::chunk] once per source file.
///
/// # Examples
///
/// ```
/// use std::path::Path;
///
/// use ucil_treesitter::parser::{Language, Parser};
/// use ucil_treesitter::chunker::Chunker;
///
/// let mut p = Parser::new();
/// let src = "fn main() {}";
/// let tree = p.parse(src, Language::Rust).unwrap();
/// let chunks = Chunker::new()
///     .chunk(&tree, src, Path::new("main.rs"), Language::Rust)
///     .unwrap();
/// assert_eq!(chunks.len(), 1);
/// assert_eq!(chunks[0].symbol_name.as_deref(), Some("main"));
/// ```
#[derive(Debug, Default, Clone, Copy)]
pub struct Chunker;

impl Chunker {
    /// Create a new `Chunker`.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Emit AST-aware chunks for `tree` / `source` in the given
    /// `language`.
    ///
    /// For the five languages covered by [`SymbolExtractor`] — Rust,
    /// Python, TypeScript, JavaScript, Go — each `Function / Method /
    /// Class / Struct / Enum / Trait / Interface` becomes one chunk.
    /// For every other language the top-level AST children of the
    /// parse tree become chunks.
    ///
    /// Oversize chunks (byte-estimated `token_count > MAX_TOKENS`)
    /// collapse to a signature-only chunk on the symbol-based path;
    /// the fallback path hard-truncates to the byte cap.
    ///
    /// # Errors
    ///
    /// - [`ChunkError::InvalidLineRange`] — a symbol or AST node's
    ///   reported end line preceded its start line.  Should not happen
    ///   with a well-formed tree-sitter tree.
    /// - [`ChunkError::Utf8Boundary`] — reserved for a future
    ///   byte-range slicing path; the current line-based implementation
    ///   does not produce this error.
    #[tracing::instrument(
        name = "ucil.treesitter.chunk",
        level = "debug",
        skip(self, tree, source),
        fields(language = ?language, file = %file_path.display())
    )]
    pub fn chunk(
        &self,
        tree: &Tree,
        source: &str,
        file_path: &Path,
        language: Language,
    ) -> Result<Vec<Chunk>, ChunkError> {
        let chunks = match language {
            Language::Rust
            | Language::Python
            | Language::TypeScript
            | Language::JavaScript
            | Language::Go => chunk_via_symbols(tree, source, file_path, language)?,
            Language::Java
            | Language::C
            | Language::Cpp
            | Language::Ruby
            | Language::Bash
            | Language::Json => chunk_via_ast(tree, source, file_path, language)?,
        };
        tracing::debug!(count = chunks.len(), "emitted chunks");
        Ok(chunks)
    }
}

// ── Symbol-based chunking ──────────────────────────────────────────────────

fn chunk_via_symbols(
    tree: &Tree,
    source: &str,
    file_path: &Path,
    language: Language,
) -> Result<Vec<Chunk>, ChunkError> {
    let extractor = SymbolExtractor::new();
    let symbols = extractor.extract(tree, source, file_path, language);
    let mut out = Vec::with_capacity(symbols.len());
    for sym in &symbols {
        if !is_chunk_worthy_kind(sym.kind) {
            continue;
        }
        out.push(chunk_from_symbol(sym, source, file_path, language)?);
    }
    Ok(out)
}

/// Kinds whose symbols produce a chunk — the "complete function /
/// method / class" set from master plan §12.2 line 1339 plus the
/// adjacent container types (struct / enum / trait / interface) that
/// are stand-alone definitions.  [`SymbolKind::TypeAlias`] /
/// [`SymbolKind::Constant`] / [`SymbolKind::Module`] intentionally do
/// NOT chunk — they are scalar declarations, not spanning definitions.
const fn is_chunk_worthy_kind(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Function
            | SymbolKind::Method
            | SymbolKind::Class
            | SymbolKind::Struct
            | SymbolKind::Enum
            | SymbolKind::Trait
            | SymbolKind::Interface
    )
}

fn chunk_from_symbol(
    sym: &ExtractedSymbol,
    source: &str,
    file_path: &Path,
    language: Language,
) -> Result<Chunk, ChunkError> {
    let mut content = slice_lines(source, sym.start_line, sym.end_line)?;
    let mut token_count = count_tokens(&content);
    if token_count > MAX_TOKENS {
        content = signature_only_chunk_content(sym, source);
        token_count = count_tokens(&content);
        // Signature is capped at 200 bytes in `symbols.rs`; a first-doc
        // paragraph is typically short, but truncate defensively if an
        // adversarial source produces a huge first paragraph.
        if token_count > MAX_TOKENS {
            content = truncate_to_token_cap(&content);
            token_count = count_tokens(&content);
        }
    }
    Ok(Chunk {
        id: make_chunk_id(file_path, sym.start_line, sym.end_line),
        file_path: file_path.to_path_buf(),
        language,
        start_line: sym.start_line,
        end_line: sym.end_line,
        content,
        symbol_name: Some(sym.name.clone()),
        symbol_kind: Some(sym.kind),
        token_count,
    })
}

// ── AST fallback chunking ──────────────────────────────────────────────────

fn chunk_via_ast(
    tree: &Tree,
    source: &str,
    file_path: &Path,
    language: Language,
) -> Result<Vec<Chunk>, ChunkError> {
    let nodes = walk_top_level_chunk_worthy_nodes(tree);
    let mut out = Vec::with_capacity(nodes.len());
    for node in nodes {
        out.push(chunk_from_ast_node(&node, source, file_path, language)?);
    }
    Ok(out)
}

fn chunk_from_ast_node(
    node: &Node<'_>,
    source: &str,
    file_path: &Path,
    language: Language,
) -> Result<Chunk, ChunkError> {
    // tree-sitter rows are 0-based; the chunk API (and master-plan
    // §12.2) is 1-based — shift once here.
    let start = usize_row_to_line(node.start_position().row);
    let end = usize_row_to_line(node.end_position().row);
    let mut content = slice_lines(source, start, end)?;
    let mut token_count = count_tokens(&content);
    if token_count > MAX_TOKENS {
        // Fallback path has no [`ExtractedSymbol`] — no signature to
        // fall back on, so hard-truncate to the cap.  The master-plan
        // oversize policy (signature + first-doc paragraph) is defined
        // for named functions / classes; fallback-language chunks have
        // no name to speak of.
        content = truncate_to_token_cap(&content);
        token_count = count_tokens(&content);
    }
    Ok(Chunk {
        id: make_chunk_id(file_path, start, end),
        file_path: file_path.to_path_buf(),
        language,
        start_line: start,
        end_line: end,
        content,
        symbol_name: None,
        symbol_kind: None,
        token_count,
    })
}

/// Return the direct *named* children of `tree`'s root node, minus a
/// small deny-list of trivial node kinds (comments, `*_list` wrapper
/// nodes) that carry no standalone semantic weight.
fn walk_top_level_chunk_worthy_nodes(tree: &Tree) -> Vec<Node<'_>> {
    let root = tree.root_node();
    let mut cursor = root.walk();
    root.named_children(&mut cursor)
        .filter(|n| !is_trivial_node_kind(n.kind()))
        .collect()
}

/// Kinds of direct root-children we never emit on the fallback path.
///
/// `comment` / `line_comment` / `block_comment` / `doc_comment` are
/// stray textual nodes; any `*_list` kind is a grammar-internal
/// wrapper that does not correspond to a stand-alone declaration.
fn is_trivial_node_kind(kind: &str) -> bool {
    matches!(
        kind,
        "comment" | "line_comment" | "block_comment" | "doc_comment"
    ) || kind.ends_with("_list")
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// Slice `source` to the inclusive 1-based line range
/// `[start_line, end_line]`.  Returns the joined sub-string (trailing
/// newline dropped, per `str::lines`'s semantics).
///
/// Errors on `end_line < start_line`, `start_line == 0`, or an empty
/// source when any non-zero line is requested.
fn slice_lines(source: &str, start_line: u32, end_line: u32) -> Result<String, ChunkError> {
    if start_line == 0 || end_line < start_line {
        return Err(ChunkError::InvalidLineRange {
            start: start_line,
            end: end_line,
        });
    }
    let lines: Vec<&str> = source.lines().collect();
    if lines.is_empty() {
        return Err(ChunkError::InvalidLineRange {
            start: start_line,
            end: end_line,
        });
    }
    let total = u32::try_from(lines.len()).unwrap_or(u32::MAX);
    let start_idx = (start_line - 1) as usize;
    if start_idx >= lines.len() {
        return Err(ChunkError::InvalidLineRange {
            start: start_line,
            end: end_line,
        });
    }
    let end_clamped = end_line.min(total);
    let end_idx = (end_clamped - 1) as usize;
    Ok(lines[start_idx..=end_idx].join("\n"))
}

/// Count tokens for `content` using the byte heuristic
/// `max(1, ⌈len / BYTES_PER_TOKEN⌉)`.
fn count_tokens(content: &str) -> u32 {
    let raw = content.len().div_ceil(BYTES_PER_TOKEN).max(1);
    u32::try_from(raw).unwrap_or(u32::MAX)
}

/// Build a signature-only chunk body — the function / method signature
/// optionally followed by the first paragraph of the symbol's
/// doc comment, separated by a blank line.
///
/// When `sym.signature` is `None` (e.g. a `Class` symbol that fell
/// through to oversize handling), the first non-blank line of the
/// symbol's source slice is used as the signature stand-in.
fn signature_only_chunk_content(sym: &ExtractedSymbol, source: &str) -> String {
    let sig = sym
        .signature
        .clone()
        .unwrap_or_else(|| first_non_blank_line(source).to_owned());
    match sym.doc_comment.as_deref() {
        Some(doc) => {
            let para = first_doc_paragraph(doc).trim_end();
            if para.is_empty() {
                sig
            } else {
                format!("{sig}\n\n{para}")
            }
        }
        None => sig,
    }
}

/// Return the first non-blank line of `source`, or `""` if every line
/// is blank.
fn first_non_blank_line(source: &str) -> &str {
    source.lines().find(|l| !l.trim().is_empty()).unwrap_or("")
}

/// Return the first paragraph of a doc comment — everything up to the
/// first blank-line delimiter.  A blank line is a line that is either
/// empty after trimming, or contains only doc-comment prefix
/// characters (`/`, `!`, `*`, whitespace).  The returned slice is
/// right-trimmed.
fn first_doc_paragraph(doc: &str) -> &str {
    match find_paragraph_break(doc) {
        Some(idx) => doc[..idx].trim_end(),
        None => doc.trim_end(),
    }
}

/// Scan `doc` for the first "paragraph break" — either a plain blank
/// line (`\n\n`) or a doc-comment prefix-only line (`///`, `*`, `//!`,
/// etc.).  Returns the byte index where the break begins, or `None`
/// if the whole text is one paragraph.
fn find_paragraph_break(doc: &str) -> Option<usize> {
    if let Some(idx) = doc.find("\n\n") {
        return Some(idx);
    }
    let mut cursor = 0usize;
    let mut started = false;
    for line in doc.split_inclusive('\n') {
        let trimmed = line.trim();
        let is_delimiter = trimmed.is_empty() || is_doc_prefix_only(trimmed);
        if is_delimiter && started {
            return Some(cursor);
        }
        if !is_delimiter {
            started = true;
        }
        cursor += line.len();
    }
    None
}

/// Is `s` a doc-comment "prefix-only" line — `///`, `*`, `//!`, `/**`,
/// `*/`, etc. — carrying no substantive text?
fn is_doc_prefix_only(s: &str) -> bool {
    if s.is_empty() {
        return true;
    }
    s.chars()
        .all(|c| c == '/' || c == '!' || c == '*' || c.is_whitespace())
}

/// Hard-truncate `content` to the `MAX_TOKENS * BYTES_PER_TOKEN` byte
/// cap, snapping the cut to the nearest `char` boundary so the result
/// is still a valid `&str`.
fn truncate_to_token_cap(content: &str) -> String {
    let cap_bytes = (MAX_TOKENS as usize) * BYTES_PER_TOKEN;
    if content.len() <= cap_bytes {
        return content.to_owned();
    }
    let mut cut = cap_bytes;
    while cut > 0 && !content.is_char_boundary(cut) {
        cut -= 1;
    }
    content[..cut].to_owned()
}

/// Format a chunk id as `"{file_path}:{start_line}:{end_line}"` — the
/// master-plan §12.2 line 1325 convention.
fn make_chunk_id(file_path: &Path, start_line: u32, end_line: u32) -> String {
    format!("{}:{start_line}:{end_line}", file_path.display())
}

/// Saturating row (0-based, `usize`) → line (1-based, `u32`) shift.
fn usize_row_to_line(row: usize) -> u32 {
    u32::try_from(row).unwrap_or(u32::MAX - 1).saturating_add(1)
}

// ── Module-root unit tests ─────────────────────────────────────────────────
//
// Per DEC-0005 (WO-0006 module-coherence commits), unit tests live at
// module root — NOT wrapped in `#[cfg(test)] mod tests { … }` — so the
// frozen acceptance selector `chunker::` resolves every test as
// `ucil_treesitter::chunker::<test_name>`.

#[cfg(test)]
use crate::parser::Parser;

#[cfg(test)]
fn parse_and_chunk(src: &str, lang: Language, path: &str) -> Vec<Chunk> {
    let mut parser = Parser::new();
    let tree = parser.parse(src, lang).expect("parse must succeed");
    Chunker::new()
        .chunk(&tree, src, Path::new(path), lang)
        .expect("chunk must succeed")
}

#[cfg(test)]
#[test]
fn chunker_emits_chunk_per_rust_function() {
    let src = "fn a() {}\nfn b() {}\nfn c() {}\n";
    let chunks = parse_and_chunk(src, Language::Rust, "multi.rs");
    let fns: Vec<&Chunk> = chunks
        .iter()
        .filter(|c| c.symbol_kind == Some(SymbolKind::Function))
        .collect();
    assert_eq!(fns.len(), 3, "expected 3 Function chunks, got {chunks:?}");
    let names: std::collections::HashSet<String> = fns
        .iter()
        .map(|c| c.symbol_name.clone().unwrap_or_default())
        .collect();
    let expected: std::collections::HashSet<String> =
        ["a", "b", "c"].iter().map(|s| (*s).to_owned()).collect();
    assert_eq!(names, expected);
    for c in &fns {
        assert!(
            !c.content.is_empty(),
            "chunk content must be non-empty: {c:?}"
        );
        assert!(
            c.token_count <= MAX_TOKENS,
            "token_count must respect MAX_TOKENS: {c:?}"
        );
    }
}

#[cfg(test)]
#[test]
fn chunker_emits_chunk_per_python_class_and_method() {
    let src =
        "class Foo:\n    def bar(self):\n        return 1\n    def baz(self):\n        return 2\n";
    let chunks = parse_and_chunk(src, Language::Python, "mod.py");
    let class = chunks
        .iter()
        .find(|c| {
            c.symbol_kind == Some(SymbolKind::Class) && c.symbol_name.as_deref() == Some("Foo")
        })
        .expect("expected Class(Foo) chunk");
    let methods: Vec<&Chunk> = chunks
        .iter()
        .filter(|c| c.symbol_kind == Some(SymbolKind::Method))
        .collect();
    let method_names: std::collections::HashSet<String> = methods
        .iter()
        .map(|c| c.symbol_name.clone().unwrap_or_default())
        .collect();
    assert!(
        method_names.contains("bar"),
        "expected Method(bar); chunks={chunks:?}"
    );
    assert!(
        method_names.contains("baz"),
        "expected Method(baz); chunks={chunks:?}"
    );
    assert!(
        chunks.len() >= 3,
        "expected ≥3 chunks (class + ≥2 methods), got {}: {chunks:?}",
        chunks.len()
    );
    for m in &methods {
        assert!(
            m.start_line >= class.start_line && m.end_line <= class.end_line,
            "method chunk {m:?} must be nested in class chunk {class:?}"
        );
    }
}

#[cfg(test)]
#[test]
fn chunker_emits_chunk_per_typescript_class_and_interface() {
    let src = "class Foo {}\ninterface Bar { x: number; }\n";
    let chunks = parse_and_chunk(src, Language::TypeScript, "a.ts");
    assert!(
        chunks
            .iter()
            .any(|c| c.symbol_kind == Some(SymbolKind::Class)
                && c.symbol_name.as_deref() == Some("Foo")),
        "expected Class(Foo) chunk; got {chunks:?}"
    );
    assert!(
        chunks
            .iter()
            .any(|c| c.symbol_kind == Some(SymbolKind::Interface)
                && c.symbol_name.as_deref() == Some("Bar")),
        "expected Interface(Bar) chunk; got {chunks:?}"
    );
}

#[cfg(test)]
#[test]
fn chunker_emits_chunk_per_go_func_and_type() {
    let src = "package p\nfunc Foo() {}\ntype Bar struct {}\n";
    let chunks = parse_and_chunk(src, Language::Go, "a.go");
    assert!(
        chunks
            .iter()
            .any(|c| c.symbol_kind == Some(SymbolKind::Function)
                && c.symbol_name.as_deref() == Some("Foo")),
        "expected Function(Foo) chunk; got {chunks:?}"
    );
    assert!(
        chunks
            .iter()
            .any(|c| c.symbol_kind == Some(SymbolKind::Struct)
                && c.symbol_name.as_deref() == Some("Bar")),
        "expected Struct(Bar) chunk; got {chunks:?}"
    );
}

#[cfg(test)]
#[test]
fn chunker_oversized_function_becomes_signature_only_chunk() {
    // Synthesise a Rust fn with a >2 KiB filler string so the slice
    // exceeds MAX_TOKENS (2048 bytes = 512 tokens * 4).
    let filler = "A".repeat(4096);
    let src = format!("fn huge(x: i32) -> i32 {{\n    let _f = \"{filler}\";\n    x\n}}\n",);
    let chunks = parse_and_chunk(&src, Language::Rust, "huge.rs");
    let huge = chunks
        .iter()
        .find(|c| c.symbol_name.as_deref() == Some("huge"))
        .expect("expected `huge` chunk");
    assert!(
        huge.token_count <= MAX_TOKENS,
        "oversize chunk must collapse under MAX_TOKENS, got token_count={}",
        huge.token_count
    );
    assert!(
        huge.content.starts_with("fn huge("),
        "signature-only content must start with `fn huge(`, got {:?}",
        huge.content
    );
    assert!(
        !huge.content.contains(&filler),
        "signature-only content must NOT contain the 4 KiB filler"
    );
}

#[cfg(test)]
#[test]
fn chunker_oversized_function_with_doc_comment_keeps_first_paragraph() {
    // Multi-paragraph doc + oversize body.
    let filler = "B".repeat(4096);
    let src = format!(
        "/// first paragraph line 1\n\
         /// first paragraph line 2\n\
         ///\n\
         /// second paragraph line 1\n\
         fn huge2(x: i32) -> i32 {{\n    let _f = \"{filler}\";\n    x\n}}\n",
    );
    let chunks = parse_and_chunk(&src, Language::Rust, "doc.rs");
    let huge = chunks
        .iter()
        .find(|c| c.symbol_name.as_deref() == Some("huge2"))
        .expect("expected `huge2` chunk");
    assert!(
        huge.token_count <= MAX_TOKENS,
        "oversize chunk must collapse under MAX_TOKENS, got {}",
        huge.token_count
    );
    assert!(
        huge.content.starts_with("fn huge2("),
        "signature-only content must start with `fn huge2(`, got {:?}",
        huge.content
    );
    assert!(
        huge.content.contains("first paragraph line 1"),
        "signature-only content must contain first doc paragraph, got {:?}",
        huge.content
    );
    assert!(
        huge.content.contains("first paragraph line 2"),
        "signature-only content must contain full first paragraph, got {:?}",
        huge.content
    );
    assert!(
        !huge.content.contains("second paragraph line 1"),
        "signature-only content must NOT contain the second paragraph, got {:?}",
        huge.content
    );
    // A blank-line separator ("\n\n") must sit between the signature
    // and the first doc paragraph.
    assert!(
        huge.content.contains("\n\n"),
        "signature-only content must have a blank-line separator, got {:?}",
        huge.content
    );
}

#[cfg(test)]
#[test]
fn chunker_id_format_matches_file_and_line_range() {
    let src = "fn a() {}\nfn b() {}\nstruct S {}\n";
    let chunks = parse_and_chunk(src, Language::Rust, "id.rs");
    assert!(
        chunks.len() >= 3,
        "expected ≥3 chunks for id-format property test, got {chunks:?}"
    );
    for c in &chunks {
        let expected = format!("{}:{}:{}", c.file_path.display(), c.start_line, c.end_line);
        assert_eq!(c.id, expected, "id format mismatch for {c:?}");
    }
}

#[cfg(test)]
#[test]
fn chunker_language_field_populated_correctly() {
    let cases: &[(Language, &str)] = &[
        (Language::Rust, "fn foo() {}\n"),
        (Language::Python, "def foo():\n    pass\n"),
        (Language::TypeScript, "function foo() {}\n"),
        (Language::Go, "package p\nfunc Foo() {}\n"),
    ];
    for (lang, src) in cases {
        let chunks = parse_and_chunk(src, *lang, "x.src");
        assert!(
            !chunks.is_empty(),
            "expected ≥1 chunk for {lang:?} / {src:?}"
        );
        for c in &chunks {
            assert_eq!(
                c.language, *lang,
                "chunk language must match input for {lang:?}: {c:?}"
            );
        }
    }
}

#[cfg(test)]
#[test]
fn chunker_symbol_name_and_kind_none_for_fallback_language_top_level() {
    let src = "public class Foo {}\n";
    let chunks = parse_and_chunk(src, Language::Java, "Foo.java");
    assert!(
        !chunks.is_empty(),
        "expected ≥1 fallback chunk for Java; got {chunks:?}"
    );
    for c in &chunks {
        assert!(
            c.symbol_name.is_none(),
            "fallback chunk must have symbol_name=None, got {c:?}"
        );
        assert!(
            c.symbol_kind.is_none(),
            "fallback chunk must have symbol_kind=None, got {c:?}"
        );
    }
}

#[cfg(test)]
#[test]
fn chunker_empty_source_returns_empty_vec() {
    let mut parser = Parser::new();
    let tree = parser
        .parse("", Language::Rust)
        .expect("parse must succeed");
    let chunks = Chunker::new()
        .chunk(&tree, "", Path::new("empty.rs"), Language::Rust)
        .expect("chunk must succeed");
    assert_eq!(
        chunks,
        Vec::<Chunk>::new(),
        "empty source must yield empty Vec"
    );
}

#[cfg(test)]
#[test]
fn chunker_token_count_matches_byte_estimate() {
    let src = "package p\nfunc F() {}\n";
    let chunks = parse_and_chunk(src, Language::Go, "a.go");
    assert!(!chunks.is_empty(), "expected ≥1 chunk, got {chunks:?}");
    for c in &chunks {
        let len = c.content.len();
        let expected = u32::try_from(len.div_ceil(4).max(1)).unwrap();
        assert_eq!(
            c.token_count, expected,
            "token_count for content of len {len} must equal max(1, ⌈len/4⌉) = {expected}; chunk={c:?}"
        );
    }
}

#[cfg(test)]
#[test]
fn chunker_chunks_never_split_mid_function() {
    let src = "fn foo(x: i32) -> i32 {\n    let a = x + 1;\n    let b = a * 2;\n    let c = b - 1;\n    c\n}\n";
    let chunks = parse_and_chunk(src, Language::Rust, "nosplit.rs");
    let foo = chunks
        .iter()
        .find(|c| c.symbol_name.as_deref() == Some("foo"))
        .expect("expected foo chunk");
    assert!(
        foo.content.starts_with("fn foo"),
        "chunk must begin at `fn foo`, got {:?}",
        foo.content
    );
    let open = foo.content.matches('{').count();
    let close = foo.content.matches('}').count();
    assert_eq!(
        open, close,
        "brace balance must hold (no mid-function cut), got {open} open vs {close} close in {:?}",
        foo.content
    );
    assert!(
        foo.content.trim_end().ends_with('}'),
        "chunk must end at the matching closing brace, got {:?}",
        foo.content
    );
}

#[cfg(test)]
#[test]
fn chunker_line_ranges_are_well_formed() {
    let src = "fn a() {}\nfn b() {}\nstruct S {}\nenum E { A, B }\n";
    let chunks = parse_and_chunk(src, Language::Rust, "lines.rs");
    let total = u32::try_from(src.lines().count()).unwrap();
    assert!(!chunks.is_empty(), "expected ≥1 chunk for {src:?}");
    for c in &chunks {
        assert!(c.start_line >= 1, "start_line must be ≥1 for {c:?}");
        assert!(
            c.end_line >= c.start_line,
            "end_line must be ≥ start_line for {c:?}"
        );
        assert!(
            c.end_line <= total,
            "end_line must be ≤ total lines ({total}) for {c:?}"
        );
        assert!(
            c.token_count <= MAX_TOKENS,
            "token_count must respect MAX_TOKENS for {c:?}"
        );
    }
}
