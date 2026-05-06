//! Real-tokenizer embedding chunker — `P2-W8-F05`.
//!
//! Master-plan §18 Phase 2 Week 8 line 1786 frames the embedding
//! pipeline; master-plan §12.2 line 1339 freezes the chunking
//! contract verbatim:
//!
//! > Chunking: AST-aware via tree-sitter.  Each chunk is a complete
//! > function/method/class.  Never split mid-function.  Max `512`
//! > tokens.  Larger functions: signature + first-paragraph doc
//! > comment.
//!
//! [`EmbeddingChunker`] is the layer that enforces the `512`-token
//! cap with the **real** `HuggingFace` `BPE` `Tokenizer`, downstream
//! of [`ucil_treesitter::Chunker`] which uses a byte-estimated
//! heuristic (`max(1, ⌈len / 4⌉)`).  See
//! `crates/ucil-treesitter/src/chunker.rs:56-58` for the explicit
//! hand-off note: the byte estimate is sufficient at the
//! tree-sitter layer, and any drift between that estimate and a
//! real tokenizer is absorbed here.
//!
//! # Pipeline
//!
//! `chunk()` is a four-step pipeline:
//!
//! 1. parse `source` via [`ucil_treesitter::Parser`];
//! 2. emit AST-aware boundary chunks via [`ucil_treesitter::Chunker`];
//! 3. **re-tokenize** each chunk's content via the real
//!    [`tokenizers::Tokenizer`] — the chunk's `token_count` is the
//!    real token count, NOT the upstream byte estimate;
//! 4. for any chunk whose real-tokenizer count exceeds
//!    [`MAX_CHUNK_TOKENS`], collapse to a signature-only chunk
//!    (master-plan §12.2 line 1339 oversize policy: signature +
//!    first-paragraph doc comment).  The signature heuristic here
//!    is "first ≤3 non-blank lines of `content`" because
//!    [`ucil_treesitter::Chunk`] does not surface `signature`
//!    separately; a future ADR may motivate exposing
//!    `Chunk::signature` for a cleaner cut, but the heuristic is
//!    sufficient for the master-plan-prescribed policy.
//!
//! # Constructor surface
//!
//! Two factories are exposed deliberately, per the
//! `WO-0059`-extracted-helper-for-testability lesson
//! (phase-log line 609):
//!
//! - [`EmbeddingChunker::from_tokenizer_path`] — production path:
//!   loads a `HuggingFace` `tokenizer.json` from disk via
//!   [`tokenizers::Tokenizer::from_file`].
//! - [`EmbeddingChunker::from_tokenizer`] — synthetic-tokenizer
//!   injection seam for tests, per `DEC-0008` (`UCIL`-internal
//!   trait/struct boundaries; the synthetic-tokenizer is built via
//!   the real `tokenizers` crate API with a synthetic vocab — NOT
//!   a critical-dep mock).
//!
//! # Downstream consumers (deferred)
//!
//! `EmbeddingChunker` is the chunk **producer** only.  It does NOT
//! invoke any embedding inference (`OnnxSession::infer` /
//! `CodeRankEmbed::embed`).  The chunk-then-embed pipeline lives at
//! the consumer sites:
//!
//! - `P2-W8-F04` — `LanceDB` chunk indexer (consumes chunks as the
//!   embedding-input stream);
//! - `P2-W8-F08` — `find_similar` `MCP` tool (uses chunked
//!   snippets at query time).
//!
//! Both are out of scope for this work-order (`WO-0060`).
//!
//! # Tracing
//!
//! [`EmbeddingChunker::from_tokenizer_path`] opens a span
//! `ucil.embeddings.chunker.load`; [`EmbeddingChunker::chunk`]
//! opens `ucil.embeddings.chunker.chunk` — both at `DEBUG` per
//! master-plan §15.2 `ucil.<layer>.<op>` naming.

// `EmbeddingChunker` / `EmbeddingChunk` / `EmbeddingChunkerError` share
// the module name prefix ("chunker" → "EmbeddingChunker"); suppress
// the pedantic lint, mirroring the escape used in
// `ucil-treesitter::chunker`.
#![allow(clippy::module_name_repetitions)]

use std::path::{Path, PathBuf};

use thiserror::Error;
use tokenizers::Tokenizer;
use ucil_treesitter::{
    Chunk as TsChunk, ChunkError as TsChunkError, Chunker as TsChunker, Language,
    ParseError as TsParseError, Parser as TsParser,
};

// ── Constants ──────────────────────────────────────────────────────────────

/// Authoritative cap on a single chunk's `token_count` — master-plan
/// §12.4 line 2030 (`chunk_max_tokens = 512`).
///
/// This is the real-tokenizer cap enforced by [`EmbeddingChunker`]; the
/// sibling [`ucil_treesitter::MAX_TOKENS`] exposes the byte-estimated
/// equivalent at the upstream tree-sitter layer.  The two values are
/// intentionally identical (`512`); the duplication is deliberate
/// because `ucil-embeddings` is the layer where the **real** tokenizer
/// cap is enforced (any drift between the byte estimate and a real
/// tokenizer is absorbed at this layer).
pub const MAX_CHUNK_TOKENS: u32 = 512;

/// Byte budget used by the single-line oversize hard-truncation
/// safety-net in [`collapse_to_signature`].
///
/// Mirrors the `BYTES_PER_TOKEN = 4` heuristic from
/// `ucil_treesitter::chunker` — a `512`-token cap implies an
/// approximate byte budget of `2048` bytes, which is sufficient for
/// any signature line in practice.
const SIGNATURE_BYTE_BUDGET: usize = (MAX_CHUNK_TOKENS as usize) * 4;

/// Maximum number of non-blank lines retained in the signature
/// heuristic when an `EmbeddingChunk` is collapsed to a signature-only
/// chunk.
///
/// `3` covers the common Rust / `TypeScript` / `Python` patterns of
/// `pub fn name(\n    arg1: T1,\n    arg2: T2,\n) -> Ret {`, which
/// spans up to three non-blank lines before the body opens.
const SIGNATURE_LINE_BUDGET: usize = 3;

// ── Errors ─────────────────────────────────────────────────────────────────

/// Failures surfaced by [`EmbeddingChunker`] operations.
///
/// `#[non_exhaustive]` per `.claude/rules/rust-style.md` so future
/// variants (e.g. a `BatchOverflow` arm if `P2-W8-F04` introduces
/// batched chunking) can be added without a `semver` break.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum EmbeddingChunkerError {
    /// The tree-sitter parse step failed — typically a grammar
    /// load failure or a parser timeout.  Ordinary syntax errors
    /// produce error nodes inside the tree, NOT this variant.
    #[error("tree-sitter parse failed: {source}")]
    Parse {
        /// Underlying [`ucil_treesitter::ParseError`].
        #[from]
        source: TsParseError,
    },

    /// The AST-aware boundary chunker upstream of this layer
    /// failed.  Should not happen on trees produced by
    /// [`ucil_treesitter::Parser`], but the variant keeps callers
    /// from `unreachable!()`-ing on defensive `match` arms.
    #[error("tree-sitter chunking failed: {source}")]
    Chunk {
        /// Underlying [`ucil_treesitter::ChunkError`].
        #[from]
        source: TsChunkError,
    },

    /// A `tokenizer.json` failed to deserialise.  The pinned
    /// `tokenizers 0.23` `Tokenizer::from_file` returns a
    /// `Box<dyn Error>` — not a `Send + Sync` type — so it is
    /// captured here as a `String` (mirrors the precedent set in
    /// `crates/ucil-embeddings/src/models.rs:271-274`).
    #[error("tokenizer load/decode failed: {message}")]
    Tokenizer {
        /// Stringified `tokenizers::Error` (e.g. JSON parse
        /// failure, missing UNK, etc.).
        message: String,
    },

    /// An early existence check for the on-disk `tokenizer.json`
    /// failed.  Surfaced before the more opaque
    /// [`Self::Tokenizer`] variant for an operator-friendly
    /// diagnostic.
    #[error("tokenizer file does not exist: {path:?}")]
    MissingTokenizerFile {
        /// The path that was checked.
        path: PathBuf,
    },

    /// Encoding a chunk's content via [`tokenizers::Tokenizer::encode`]
    /// failed.  Carries the offending content's byte length plus
    /// the underlying error string for an operator-actionable
    /// diagnostic (mirrors the `WO-0051` lessons line 405 pattern).
    #[error("failed to encode chunk content (len={content_len}): {message}")]
    EncodingFailure {
        /// Byte length of the content that failed to encode.
        content_len: usize,
        /// Stringified `tokenizers::Error`.
        message: String,
    },
}

// ── EmbeddingChunk ─────────────────────────────────────────────────────────

/// A single embedding-tokenizer-aware chunk of source code.
///
/// Mirrors the embedding-relevant subset of the master-plan §12.2
/// lines 1324-1336 `code_chunks_schema` (`file_path`, `start_line`,
/// `end_line`, `content`, `token_count`).  The full
/// `LanceDB`-write-time schema (`id` / `language` / `symbol_name` /
/// `symbol_kind` / `embedding` / `file_hash` / `indexed_at`) is
/// populated downstream at the `P2-W8-F04` indexer-write boundary;
/// at this layer the chunker emits only the embedding-input fields.
///
/// # Invariants
///
/// - `start_line >= 1`
/// - `end_line >= start_line`
/// - `token_count <= MAX_CHUNK_TOKENS` (`512`)
/// - `token_count` reflects the **real** `HuggingFace` tokenizer
///   count, NOT the upstream `ucil_treesitter::Chunk::token_count`
///   byte estimate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingChunk {
    /// Path of the source file this chunk was extracted from.
    pub file_path: PathBuf,
    /// 1-based line number of the chunk's first line in the source
    /// file.  Always `>= 1`.
    pub start_line: u32,
    /// 1-based line number of the chunk's last line in the source
    /// file.  Always `>= start_line`.
    pub end_line: u32,
    /// The chunk's source text — either the full span or, for an
    /// oversize chunk, a signature-only fallback body.
    pub content: String,
    /// Real-tokenizer token count over `content`.  Always
    /// `<= MAX_CHUNK_TOKENS`.
    pub token_count: u32,
}

// ── EmbeddingChunker ───────────────────────────────────────────────────────

/// Real-tokenizer embedding chunker.
///
/// Owns a [`tokenizers::Tokenizer`] (the production
/// `HuggingFace` `BPE` tokenizer for `CodeRankEmbed` or
/// equivalent) plus a [`ucil_treesitter::Parser`] used to drive
/// the upstream AST-aware boundary chunker.
///
/// # Constructor choice
///
/// - For production callers (e.g. the future `P2-W8-F04`
///   `LanceDB` indexer), use [`EmbeddingChunker::from_tokenizer_path`]
///   to load the upstream `tokenizer.json` from disk.
/// - For tests, use [`EmbeddingChunker::from_tokenizer`] to inject
///   a synthetic tokenizer (e.g. the `WordLevel` + `WhitespaceSplit`
///   shape constructed via JSON in this module's frozen acceptance
///   test).  Per `DEC-0008`, this is NOT a mock of a critical
///   dependency — the synthetic tokenizer is built via the real
///   `tokenizers` crate API with a synthetic vocab.
///
/// **Not** `Clone` — the embedded `tokenizers::Tokenizer` may not
/// be cheaply clonable; consumers needing shared chunking wrap
/// in `Arc<Mutex<EmbeddingChunker>>`.  [`Self::chunk`] takes
/// `&mut self` because the upstream `Parser::parse` mutates parser
/// state (mirrors the `ort 2.x` `Session::run` `&mut self`
/// precedent set in `WO-0058` lessons line 561).
pub struct EmbeddingChunker {
    tokenizer: Tokenizer,
    parser: TsParser,
}

// Manual `Debug` because `ucil_treesitter::Parser` does not derive
// `Debug` (its inner `tree_sitter::Parser` is opaque).  The synthetic
// formatter elides the parser entirely and reports only the tokenizer
// presence so a `tracing::debug!(?chunker, ...)` call site still
// produces meaningful output.
impl std::fmt::Debug for EmbeddingChunker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmbeddingChunker")
            .field("tokenizer", &"<HuggingFace Tokenizer>")
            .field("parser", &"<ucil_treesitter::Parser>")
            .finish()
    }
}

impl EmbeddingChunker {
    /// Construct an [`EmbeddingChunker`] from an in-memory
    /// [`tokenizers::Tokenizer`].
    ///
    /// This is the test-injection seam per `DEC-0008` and the
    /// `WO-0059`-extracted-helper-for-testability discipline
    /// (phase-log line 609).  The constructor is `pub` so the
    /// `mod tests {}` block lower in this file (and downstream
    /// `crates/ucil-embeddings/tests/...` integration tests, when
    /// they land in `P2-W8-F04`) can inject a synthetic tokenizer
    /// without touching disk.
    ///
    /// Production callers SHOULD prefer
    /// [`EmbeddingChunker::from_tokenizer_path`].
    #[must_use]
    pub fn from_tokenizer(tokenizer: Tokenizer) -> Self {
        Self {
            tokenizer,
            parser: TsParser::new(),
        }
    }

    /// Construct an [`EmbeddingChunker`] by loading the `HuggingFace`
    /// `tokenizer.json` at `tokenizer_path` from disk.
    ///
    /// # Errors
    ///
    /// - [`EmbeddingChunkerError::MissingTokenizerFile`] if
    ///   `tokenizer_path` does not exist (early existence check
    ///   before the more opaque
    ///   [`tokenizers::Tokenizer::from_file`] error);
    /// - [`EmbeddingChunkerError::Tokenizer`] if the file exists
    ///   but fails to deserialise (corrupt JSON, missing `UNK`,
    ///   `ABI` mismatch, etc.).
    #[tracing::instrument(
        name = "ucil.embeddings.chunker.load",
        level = "debug",
        skip(tokenizer_path),
        fields(path = ?tokenizer_path)
    )]
    pub fn from_tokenizer_path(tokenizer_path: &Path) -> Result<Self, EmbeddingChunkerError> {
        if !tokenizer_path.exists() {
            return Err(EmbeddingChunkerError::MissingTokenizerFile {
                path: tokenizer_path.to_path_buf(),
            });
        }
        let tokenizer =
            Tokenizer::from_file(tokenizer_path).map_err(|e| EmbeddingChunkerError::Tokenizer {
                message: format!("{e}"),
            })?;
        Ok(Self::from_tokenizer(tokenizer))
    }

    /// Run the full chunking pipeline over `source` for `language` and
    /// return real-tokenizer-aware [`EmbeddingChunk`]s in source
    /// order.
    ///
    /// Implementation lives lower in this file in
    /// `chunk_impl` (commit-2 ships the skeleton; commit-3 lands the
    /// full pipeline).  See module-level rustdoc for the four-step
    /// contract.
    ///
    /// # Errors
    ///
    /// - [`EmbeddingChunkerError::Parse`] if the upstream tree-sitter
    ///   parse fails;
    /// - [`EmbeddingChunkerError::Chunk`] if the upstream AST chunker
    ///   fails;
    /// - [`EmbeddingChunkerError::EncodingFailure`] if the real
    ///   tokenizer rejects a chunk's content.
    #[tracing::instrument(
        name = "ucil.embeddings.chunker.chunk",
        level = "debug",
        skip(self, source),
        fields(file = %file_path.display(), language = ?language, source_len = source.len())
    )]
    pub fn chunk(
        &mut self,
        file_path: &Path,
        source: &str,
        language: Language,
    ) -> Result<Vec<EmbeddingChunk>, EmbeddingChunkerError> {
        let tree = self.parser.parse(source, language)?;
        let ast_chunks = TsChunker::new().chunk(&tree, source, file_path, language)?;
        let mut out = Vec::with_capacity(ast_chunks.len());
        for ast_chunk in &ast_chunks {
            out.push(self.retokenize_chunk(ast_chunk)?);
        }
        // Source-order ordering: `ucil_treesitter::Chunker` returns
        // chunks in tree-sitter query-match order, which interleaves
        // class definitions and their nested method symbols.
        // Re-sort by `start_line` so the embedding-input stream is
        // deterministic and source-monotonic — required by the
        // `LanceDB` indexer (`P2-W8-F04`) for stable chunk ids.
        out.sort_by_key(|c| c.start_line);
        tracing::debug!(count = out.len(), "emitted embedding chunks");
        Ok(out)
    }

    /// Re-tokenize a single [`ucil_treesitter::Chunk`] with the
    /// real `HuggingFace` tokenizer; collapse to signature-only
    /// when the real-tokenizer count exceeds [`MAX_CHUNK_TOKENS`].
    ///
    /// Visibility is `pub(crate)` (promoted from `pub(super)` /
    /// private) to enable in-test invocation from
    /// `mod tests {}` lower in this file — per `WO-0055`
    /// lessons line 457 the visibility promotion is documented
    /// here so the rationale is not lost on a future refactor.
    ///
    /// # Errors
    ///
    /// - [`EmbeddingChunkerError::EncodingFailure`] if the
    ///   real tokenizer rejects either the full content or the
    ///   collapsed signature.
    pub(crate) fn retokenize_chunk(
        &self,
        ast_chunk: &TsChunk,
    ) -> Result<EmbeddingChunk, EmbeddingChunkerError> {
        let real_count = encode_token_count(&self.tokenizer, &ast_chunk.content)?;
        if real_count <= MAX_CHUNK_TOKENS {
            return Ok(EmbeddingChunk {
                file_path: ast_chunk.file_path.clone(),
                start_line: ast_chunk.start_line,
                end_line: ast_chunk.end_line,
                content: ast_chunk.content.clone(),
                token_count: real_count,
            });
        }
        self.collapse_to_signature(ast_chunk)
    }

    /// Build the signature-only fallback for an oversize chunk —
    /// master-plan §12.2 line 1339 ("signature + first-paragraph
    /// doc comment").
    ///
    /// The signature is the first ≤[`SIGNATURE_LINE_BUDGET`] non-blank
    /// lines of `ast_chunk.content`; if the resulting token count
    /// still exceeds [`MAX_CHUNK_TOKENS`] (e.g. a single 5000-byte
    /// signature line on adversarial input), the content is
    /// hard-truncated to [`SIGNATURE_BYTE_BUDGET`] bytes on a
    /// `char`-boundary cut.
    ///
    /// # Errors
    ///
    /// - [`EmbeddingChunkerError::EncodingFailure`] if either the
    ///   first-pass signature OR the hard-truncated fallback fails
    ///   to encode.
    pub(crate) fn collapse_to_signature(
        &self,
        ast_chunk: &TsChunk,
    ) -> Result<EmbeddingChunk, EmbeddingChunkerError> {
        let mut signature = first_non_blank_lines(&ast_chunk.content, SIGNATURE_LINE_BUDGET);
        let mut signature_count = encode_token_count(&self.tokenizer, &signature)?;
        if signature_count > MAX_CHUNK_TOKENS {
            // Iterative byte-budget shrink — start at
            // [`SIGNATURE_BYTE_BUDGET`] (the `4`-bytes-per-token
            // heuristic) and halve until the real-tokenizer count
            // satisfies the cap.  Adversarial inputs (e.g. a
            // 5000-byte single-line content under a tokenizer with a
            // tight token-per-byte ratio) require iteration because
            // the `4`-bytes-per-token heuristic is an upper bound,
            // not a guarantee.
            let mut budget = SIGNATURE_BYTE_BUDGET;
            loop {
                signature = truncate_to_byte_budget(&signature, budget);
                signature_count = encode_token_count(&self.tokenizer, &signature)?;
                if signature_count <= MAX_CHUNK_TOKENS || budget == 0 {
                    break;
                }
                budget /= 2;
            }
        }
        Ok(EmbeddingChunk {
            file_path: ast_chunk.file_path.clone(),
            start_line: ast_chunk.start_line,
            end_line: ast_chunk.end_line,
            content: signature,
            token_count: signature_count,
        })
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// Encode `text` via `tokenizer` and return the resulting token count.
///
/// `add_special_tokens = false` — special tokens (`CLS`, `SEP`,
/// padding) are not part of the chunk-cap accounting; the production
/// `CodeRankEmbed` model adds them at inference time, but at chunk
/// time we only count the content tokens.
fn encode_token_count(tokenizer: &Tokenizer, text: &str) -> Result<u32, EmbeddingChunkerError> {
    let encoding =
        tokenizer
            .encode(text, false)
            .map_err(|e| EmbeddingChunkerError::EncodingFailure {
                content_len: text.len(),
                message: format!("{e}"),
            })?;
    Ok(u32::try_from(encoding.get_ids().len()).unwrap_or(u32::MAX))
}

/// Collect up to `budget` non-blank lines of `content`, joined with
/// `\n`.  Never returns more than `budget` lines.
fn first_non_blank_lines(content: &str, budget: usize) -> String {
    let mut taken = 0usize;
    let mut out = String::new();
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if taken > 0 {
            out.push('\n');
        }
        out.push_str(line);
        taken += 1;
        if taken >= budget {
            break;
        }
    }
    if out.is_empty() {
        // Adversarial all-blank content — fall back to the original
        // string so the downstream encoder has *something* to count
        // against.  The hard-truncation step still applies.
        return content.to_owned();
    }
    out
}

/// Hard-truncate `content` to `byte_budget` bytes, snapping the cut to
/// the nearest `char` boundary so the result is still a valid `&str`.
fn truncate_to_byte_budget(content: &str, byte_budget: usize) -> String {
    if content.len() <= byte_budget {
        return content.to_owned();
    }
    let mut cut = byte_budget;
    while cut > 0 && !content.is_char_boundary(cut) {
        cut -= 1;
    }
    content[..cut].to_owned()
}

// ── Module-root frozen acceptance test ─────────────────────────────────────
//
// Per `DEC-0007` (frozen-selector module-root placement), the
// acceptance test for `P2-W8-F05` lives at module root — NOT inside
// `mod tests {}` — so the frozen selector
// `chunker::test_embedding_chunker_real_fixture` resolves directly
// against the rustc test binary.  The frozen
// `feature-list.json:P2-W8-F05.acceptance_tests[0].selector` is
// `-p ucil-embeddings chunker::`.

/// Build a synthetic `HuggingFace` tokenizer for the frozen
/// acceptance test + the `mod tests {}` unit tests.
///
/// The tokenizer is constructed via the real `tokenizers` crate API
/// (NOT a mock per `DEC-0008`): a `WordLevel` model with a single
/// vocab entry (`<unk>` → `0`) plus a `WhitespaceSplit` pre-tokenizer.
/// Because every word maps to the `UNK` id, the encoded id stream's
/// length equals the number of whitespace-separated tokens — i.e.
/// `encoding.get_ids().len() == content.split_whitespace().count()`.
/// This deterministic property powers the AC09 sub-assertion
/// (`token_count` uses real tokenizer, not byte estimate).
#[cfg(test)]
fn build_synthetic_tokenizer() -> Tokenizer {
    // Construct via JSON deserialisation — avoids depending on the
    // private `ahash::AHashMap` type that `WordLevelBuilder::vocab`
    // takes; the synthesised JSON mirrors the `tokenizer.json`
    // schema produced by `HuggingFace` `transformers` exports.
    let tokenizer_json = r#"{
        "version": "1.0",
        "truncation": null,
        "padding": null,
        "added_tokens": [],
        "normalizer": null,
        "pre_tokenizer": {"type": "WhitespaceSplit"},
        "post_processor": null,
        "decoder": null,
        "model": {
            "type": "WordLevel",
            "vocab": {"<unk>": 0},
            "unk_token": "<unk>"
        }
    }"#;
    tokenizer_json
        .parse::<Tokenizer>()
        .expect("synthetic tokenizer must parse")
}

/// Frozen acceptance test for `P2-W8-F05` per
/// `feature-list.json:P2-W8-F05.acceptance_tests[0]`.
///
/// Asserts (in order):
///
/// 1. `chunks` is non-empty after running over `tests/data/sample.rs`;
/// 2. every chunk's `token_count <= MAX_CHUNK_TOKENS`;
/// 3. `chunks[0].token_count` matches the synthetic tokenizer's
///    whitespace-word count over `chunks[0].content` (NOT the byte
///    estimate from `ucil_treesitter::Chunk::token_count`);
/// 4. metadata round-trips correctly (`file_path`, `start_line`,
///    `end_line`, non-empty `content`);
/// 5. chunks are returned in source order (`start_line` ascending).
#[cfg(test)]
#[test]
fn test_embedding_chunker_real_fixture() {
    let tokenizer = build_synthetic_tokenizer();
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_path = manifest_dir.join("tests").join("data").join("sample.rs");
    let source = std::fs::read_to_string(&fixture_path)
        .expect("sample.rs fixture must be present at tests/data/sample.rs");

    let mut chunker = EmbeddingChunker::from_tokenizer(tokenizer);
    let chunks = chunker
        .chunk(&fixture_path, &source, Language::Rust)
        .expect("chunk over sample.rs must succeed");

    // (SA1) Chunk count is positive.
    assert!(
        !chunks.is_empty(),
        "sample.rs must produce at least one chunk; got {chunks:?}"
    );

    // (SA2) Every chunk respects the cap.
    for chunk in &chunks {
        assert!(
            chunk.token_count <= MAX_CHUNK_TOKENS,
            "chunk {chunk:?} violates token cap (token_count={}, MAX_CHUNK_TOKENS={MAX_CHUNK_TOKENS})",
            chunk.token_count
        );
    }

    // (SA3) Token count uses the REAL synthetic tokenizer, NOT the
    // byte estimate.  The synthetic tokenizer counts
    // whitespace-separated tokens; the byte estimate would be
    // `max(1, ⌈len/4⌉)` which materially differs for typical Rust
    // source.
    let first = &chunks[0];
    let expected = u32::try_from(first.content.split_whitespace().count())
        .expect("whitespace-word count must fit in u32");
    assert_eq!(
        first.token_count, expected,
        "token_count must reflect real tokenizer (whitespace word count); got {} expected {} for content {:?}",
        first.token_count, expected, first.content
    );

    // (SA4) Metadata preserved.
    assert_eq!(
        first.file_path, fixture_path,
        "file_path metadata must round-trip; got {:?}",
        first.file_path
    );
    assert!(
        first.start_line >= 1,
        "start_line must be >= 1; got {} in {first:?}",
        first.start_line
    );
    assert!(
        first.end_line >= first.start_line,
        "end_line must be >= start_line; got start={} end={} in {first:?}",
        first.start_line,
        first.end_line
    );
    assert!(
        !first.content.is_empty(),
        "content must be non-empty; got empty content in {first:?}"
    );

    // (SA5) Source-order ordering.
    for window in chunks.windows(2) {
        assert!(
            window[0].start_line <= window[1].start_line,
            "chunks must be in source order; got {:?} then {:?}",
            window[0],
            window[1]
        );
    }
}

// ── Unit tests ─────────────────────────────────────────────────────────────
//
// Per `WO-0059` lessons line 601 (preserve module-root frozen-selector
// + add coverage-driving negative-path tests), the unit tests below
// live inside `#[cfg(test)] mod tests {}` while the frozen acceptance
// test stays at module root.  These tests pin every
// `EmbeddingChunkerError` variant to a concrete reachability path so
// the coverage gate (`bash scripts/verify/coverage-gate.sh
// ucil-embeddings 85 75`) stays ≥85% line floor on this module per
// `WO-0059` lessons line 606 (coverage-floor pre-flight).

#[cfg(test)]
mod tests {
    use super::*;

    use std::io::Write;

    use tempfile::NamedTempFile;

    /// Construct a synthetic `ucil_treesitter::Chunk` literal for the
    /// `retokenize_chunk` / `collapse_to_signature` unit tests.
    /// Lives in `mod tests {}` because nothing else in the file
    /// constructs a `TsChunk` directly — `chunk()` defers to
    /// `ucil_treesitter::Chunker`.
    fn synthetic_ts_chunk(content: &str, start: u32, end: u32) -> TsChunk {
        TsChunk {
            id: format!("synth.rs:{start}:{end}"),
            file_path: PathBuf::from("synth.rs"),
            language: Language::Rust,
            start_line: start,
            end_line: end,
            content: content.to_owned(),
            symbol_name: Some("synthetic".to_owned()),
            symbol_kind: None,
            // Byte-estimate carry-through; real chunker never reads
            // this field (it re-tokenizes).
            token_count: u32::try_from(content.len().div_ceil(4).max(1)).unwrap_or(u32::MAX),
        }
    }

    #[test]
    fn from_tokenizer_path_returns_missing_tokenizer_file_for_absent_path() {
        let absent = PathBuf::from("/definitely/not/a/real/tokenizer-WO-0060.json");
        let result = EmbeddingChunker::from_tokenizer_path(&absent);
        match result {
            Err(EmbeddingChunkerError::MissingTokenizerFile { path }) => {
                assert_eq!(
                    path, absent,
                    "MissingTokenizerFile path must round-trip; got {path:?}"
                );
            }
            other => panic!("expected MissingTokenizerFile for absent path; got {other:?}"),
        }
    }

    #[test]
    fn from_tokenizer_path_returns_tokenizer_for_invalid_json() {
        // 21 bytes of garbage in a tempfile — the file exists, so the
        // existence check passes, but `Tokenizer::from_file`
        // deserialise fails.
        let mut tmp = NamedTempFile::new().expect("tempfile must create");
        tmp.write_all(b"{not valid json garb")
            .expect("write must succeed");
        tmp.flush().expect("flush must succeed");
        let path = tmp.path().to_path_buf();
        let result = EmbeddingChunker::from_tokenizer_path(&path);
        match result {
            Err(EmbeddingChunkerError::Tokenizer { message }) => {
                assert!(
                    !message.is_empty(),
                    "Tokenizer error message must be non-empty; got {message:?}"
                );
            }
            other => panic!("expected Tokenizer error for invalid json; got {other:?}"),
        }
    }

    #[test]
    fn retokenize_chunk_collapses_oversize_to_signature() {
        // 1000 whitespace-separated words → 1000 synthetic tokens > 512.
        let mut filler = String::new();
        for i in 0..1000 {
            if i > 0 {
                filler.push(' ');
            }
            filler.push_str("word");
        }
        // Wrap in a fn signature so the first non-blank line is the
        // declaration.  The signature heuristic keeps the first 3
        // non-blank lines, so the body's filler does NOT survive.
        let content = format!("fn huge() -> i32 {{\n    let _ = \"{filler}\";\n    0\n}}");
        let ast_chunk = synthetic_ts_chunk(&content, 1, 4);

        let chunker = EmbeddingChunker::from_tokenizer(build_synthetic_tokenizer());
        let result = chunker
            .retokenize_chunk(&ast_chunk)
            .expect("retokenize_chunk must succeed");

        assert!(
            result.token_count <= MAX_CHUNK_TOKENS,
            "oversize chunk must collapse below cap; got token_count={} for content {:?}",
            result.token_count,
            result.content
        );
        assert!(
            result.content.starts_with("fn huge"),
            "signature-only fallback must start with `fn huge`; got {:?}",
            result.content
        );
        assert!(
            !result.content.contains(&filler),
            "signature-only fallback must NOT contain the 1000-word filler; got len={}",
            result.content.len()
        );
        assert_eq!(
            result.start_line, ast_chunk.start_line,
            "metadata must round-trip (start_line)"
        );
        assert_eq!(
            result.end_line, ast_chunk.end_line,
            "metadata must round-trip (end_line)"
        );
    }

    #[test]
    fn collapse_to_signature_handles_single_line_oversize_content() {
        // 5000 whitespace-separated single-character tokens on one
        // line — exercises the byte-budget hard-truncation safety-net.
        let single_line: String = (0..5000)
            .map(|_| 'x')
            .map(|c| format!("{c}"))
            .collect::<Vec<_>>()
            .join(" ");
        // No newlines — `first_non_blank_lines` returns the whole
        // line, then signature_count > MAX_CHUNK_TOKENS triggers
        // hard-truncation to SIGNATURE_BYTE_BUDGET bytes.
        let ast_chunk = synthetic_ts_chunk(&single_line, 1, 1);

        let chunker = EmbeddingChunker::from_tokenizer(build_synthetic_tokenizer());
        let result = chunker
            .collapse_to_signature(&ast_chunk)
            .expect("collapse_to_signature must succeed");

        assert!(
            result.token_count <= MAX_CHUNK_TOKENS,
            "single-line oversize content must hard-truncate below cap; got token_count={}",
            result.token_count
        );
        assert!(
            result.content.len() <= SIGNATURE_BYTE_BUDGET,
            "single-line content must respect byte budget {} bytes; got len={}",
            SIGNATURE_BYTE_BUDGET,
            result.content.len()
        );
    }

    #[test]
    fn embedding_chunk_round_trips_metadata() {
        let chunk = EmbeddingChunk {
            file_path: PathBuf::from("foo.rs"),
            start_line: 12,
            end_line: 34,
            content: "fn foo() {}".to_owned(),
            token_count: 4,
        };
        let cloned = chunk.clone();
        assert_eq!(chunk, cloned, "Clone + PartialEq must round-trip");
        assert_eq!(chunk.file_path, PathBuf::from("foo.rs"));
        assert_eq!(chunk.start_line, 12);
        assert_eq!(chunk.end_line, 34);
        assert_eq!(chunk.content, "fn foo() {}");
        assert_eq!(chunk.token_count, 4);
        // Smoke-test Debug — should not panic.
        let dbg = format!("{chunk:?}");
        assert!(
            dbg.contains("foo.rs"),
            "Debug must include file_path; got {dbg:?}"
        );
    }

    #[test]
    fn first_non_blank_lines_returns_only_non_blank_within_budget() {
        let content = "\n\n  \nfirst\n\nsecond\n  \nthird\nfourth\n";
        let out = first_non_blank_lines(content, 3);
        assert_eq!(out, "first\nsecond\nthird");
    }

    #[test]
    fn first_non_blank_lines_falls_back_to_original_for_all_blank_input() {
        let content = "\n  \n\t\n";
        let out = first_non_blank_lines(content, 3);
        assert_eq!(
            out, content,
            "all-blank input must fall back to original; got {out:?}"
        );
    }

    #[test]
    fn truncate_to_byte_budget_returns_unchanged_when_under_budget() {
        let content = "hello";
        let out = truncate_to_byte_budget(content, 100);
        assert_eq!(out, content);
    }

    #[test]
    fn truncate_to_byte_budget_snaps_to_char_boundary() {
        // Multi-byte char "é" (2 bytes) followed by ASCII.
        let content = "aé".repeat(10); // 10 * 3 bytes = 30 bytes
        let out = truncate_to_byte_budget(&content, 5);
        // The cut at byte 5 lands mid-codepoint; truncate should snap
        // back to byte 4 (boundary after second "aé").
        assert!(
            out.len() <= 5,
            "truncated len must be <= budget 5; got {}",
            out.len()
        );
        // Result must still be valid UTF-8 (implicit if it was a `&str`).
        assert!(
            out.chars().count() <= 4,
            "truncated char count must be <= 4; got {}",
            out.chars().count()
        );
    }

    #[test]
    fn encode_token_count_returns_word_count_for_synthetic_tokenizer() {
        let tokenizer = build_synthetic_tokenizer();
        let count = encode_token_count(&tokenizer, "hello world from chunker test")
            .expect("encode must succeed");
        assert_eq!(
            count, 5,
            "synthetic tokenizer must count 5 whitespace words; got {count}"
        );
    }

    #[test]
    fn chunk_returns_empty_vec_for_empty_source() {
        // Empty source produces zero AST chunks; the embedding chunker
        // returns an empty vec (the upstream `ucil_treesitter::Chunker`
        // also returns empty for empty source per the `chunker_empty_source_returns_empty_vec`
        // unit test in `crates/ucil-treesitter/src/chunker.rs`).
        let mut chunker = EmbeddingChunker::from_tokenizer(build_synthetic_tokenizer());
        let chunks = chunker
            .chunk(Path::new("empty.rs"), "", Language::Rust)
            .expect("chunk over empty source must succeed");
        assert!(
            chunks.is_empty(),
            "empty source must yield empty Vec; got {chunks:?}"
        );
    }

    #[test]
    fn chunk_propagates_treesitter_chunk_error_via_from_impls() {
        // Sanity: confirm the From-conversion paths compile and route
        // correctly.  Constructed directly from a `ChunkError`
        // variant — exercises the `#[from]` arm on
        // `EmbeddingChunkerError::Chunk`.
        let upstream = ucil_treesitter::ChunkError::InvalidLineRange { start: 5, end: 3 };
        let wrapped: EmbeddingChunkerError = upstream.into();
        match wrapped {
            EmbeddingChunkerError::Chunk { source } => {
                let msg = format!("{source}");
                assert!(
                    msg.contains("invalid line range"),
                    "Chunk variant must carry the upstream error message; got {msg:?}"
                );
            }
            other => panic!("expected Chunk variant; got {other:?}"),
        }
    }
}
