//! Symbol extraction from tree-sitter parse trees.
//!
//! Given a [`tree_sitter::Tree`], the original source, a file path, and a
//! [`Language`], [`SymbolExtractor::extract`] walks the tree using
//! language-specific tree-sitter queries and returns a list of named
//! [`ExtractedSymbol`] values covering function / method / class / struct /
//! enum / trait / interface / type-alias / constant / module definitions.
//!
//! This module implements feature `P1-W2-F02` — master plan §18 Phase 1
//! Week 2 ("Implement ucil-treesitter: multi-language parser, symbol
//! extraction, AST-aware chunking") and §2.1 Layer 1 (daemon core
//! tree-sitter integration).
//!
//! # Language support
//!
//! | Language       | Support    | Node kinds captured                                              |
//! |----------------|------------|------------------------------------------------------------------|
//! | Rust           | primary    | `function_item`, `function_signature_item`, `struct_item`,       |
//! |                |            | `enum_item`, `trait_item`, impl-methods, `const_item`,           |
//! |                |            | `type_item`, `mod_item`                                          |
//! | Python         | primary    | `function_definition` (Function vs Method via ancestor),         |
//! |                |            | `class_definition`                                               |
//! | TypeScript     | primary    | `function_declaration`, `class_declaration`,                     |
//! |                |            | `method_definition`, `interface_declaration`,                    |
//! |                |            | `type_alias_declaration`                                         |
//! | JavaScript     | primary    | `function_declaration`, `class_declaration`, `method_definition` |
//! | Go             | primary    | `function_declaration`, `method_declaration`,                    |
//! |                |            | `type_declaration` (struct / interface / alias)                  |
//! | Java, C, C++,  | fallback   | Always returns an empty `Vec` — richer extraction is a future    |
//! | Ruby, Bash,    |            | work-order.                                                      |
//! | JSON           |            |                                                                  |
//!
//! The fallback arm guarantees [`SymbolExtractor::extract`] never panics on
//! any [`Language`] variant — this invariant is asserted by the unit test
//! `symbols_extract_all_supported_languages_no_panic`.
//!
//! # Heuristics
//!
//! ## Signature capture
//!
//! For `Function` / `Method` symbols, the signature is the node's source
//! substring from `node.start_byte()` up to (but not including) the first
//! `{` or `;`, trimmed, collapsed to a single line, and truncated at 200
//! chars.  This captures `fn add(x: i32, y: i32) -> i32` for a Rust
//! function definition and the trait-method declaration `fn m(&self)` for
//! a body-less signature.  For every other [`SymbolKind`], `signature` is
//! `None`.
//!
//! ## Doc-comment capture (opportunistic)
//!
//! - **Rust** — walks backwards from the symbol's preceding sibling and
//!   collects contiguous `line_comment` nodes whose text starts with
//!   `///`; the joined text (with newlines between lines, original
//!   prefix retained) is the doc comment.
//! - **Python** — inspects the function / class body and, if its first
//!   statement is an `expression_statement` whose only child is a
//!   `string` literal, returns that string's raw source text as the doc
//!   comment.
//! - **TypeScript / JavaScript** — walks backwards for the immediately
//!   preceding `comment` sibling and returns its text if it starts with
//!   `/**`.
//!
//! All three are opportunistic: if no matching comment is found, the
//! `doc_comment` field is `None`.
//!
//! # Tracing
//!
//! [`SymbolExtractor::extract`] opens a `tracing` span
//! `ucil.treesitter.extract_symbols` at `DEBUG` level per master plan
//! §15.2 (`ucil.<layer>.<op>` naming convention).  No per-match span is
//! opened — only one span per call.

// `SymbolExtractor` / `SymbolKind` / `ExtractedSymbol` intentionally share
// a name prefix with the module ("symbols" → "SymbolKind", etc.); suppress
// the pedantic lint, mirroring the escape used by `parser.rs`.
#![allow(clippy::module_name_repetitions)]

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use streaming_iterator::StreamingIterator as _;
use tree_sitter::{Node, Query, QueryCursor, Tree};

use crate::parser::Language;

// ── Constants ──────────────────────────────────────────────────────────────

/// Upper bound on `signature` length, in bytes.
///
/// Signatures are best-effort display strings, not structured data; a tight
/// cap keeps them compact for UI surfaces and avoids pathological output
/// on minified sources.
const SIG_CAP: usize = 200;

// ── Types ──────────────────────────────────────────────────────────────────

/// Kind of a symbol extracted from source code.
///
/// Variants cover the six primary kinds enumerated in master plan §18
/// Phase 1 Week 2 (function, method, class, struct, enum, trait) plus the
/// minimum headroom needed for TypeScript / Go coverage (interface, type
/// alias, constant, module).
///
/// This enum is [`#[non_exhaustive]`] so future grammar integrations may
/// add variants without a semver break.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    /// A free / top-level function (`fn foo()` in Rust,
    /// `def foo()` in Python, `function foo()` in JS/TS, `func Foo()`
    /// in Go).
    Function,
    /// A method — a function defined inside a class body, `impl`
    /// block, or trait body (`impl Foo { fn bar(&self) }`,
    /// `class Foo: def bar(self)`, Go `func (r Recv) Bar()`).
    Method,
    /// A class definition (`class Foo:` in Python, `class Foo {}` in
    /// TS/JS).
    Class,
    /// A Rust `struct` type (also used for Go `type Foo struct {}`).
    Struct,
    /// A Rust `enum` type.
    Enum,
    /// A Rust `trait` definition.
    Trait,
    /// A TypeScript / Go `interface` declaration.
    Interface,
    /// A type alias (`type Foo = …` in Rust / TypeScript / Go).
    TypeAlias,
    /// A constant declaration (`const FOO: T = …` in Rust).
    Constant,
    /// A module declaration (`mod foo { … }` in Rust).
    Module,
}

/// A named symbol extracted from a parse tree.
///
/// Line / column numbers are 1-based (row 0 of tree-sitter's zero-based
/// coordinates becomes `start_line == 1`) so the values are directly
/// usable by editor integrations that expect 1-based positions.
///
/// `file_path` is copied from the `extract` call so every symbol carries
/// enough context to be stored in a downstream index without an auxiliary
/// lookup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractedSymbol {
    /// The symbol's declared identifier (e.g. `"my_func"`, `"MyStruct"`).
    pub name: String,
    /// What kind of syntactic construct this symbol is.
    pub kind: SymbolKind,
    /// Path of the source file this symbol was extracted from.
    pub file_path: PathBuf,
    /// Language the source was parsed as.
    #[serde(with = "language_serde")]
    pub language: Language,
    /// 1-based line number of the symbol's first character.
    pub start_line: u32,
    /// 1-based column number of the symbol's first character.
    pub start_col: u32,
    /// 1-based line number of the symbol's last character.
    pub end_line: u32,
    /// 1-based column number of the symbol's last character.
    pub end_col: u32,
    /// Best-effort signature string, capped at 200 chars — populated for
    /// `Function` / `Method` kinds only; `None` for all other kinds.
    pub signature: Option<String>,
    /// Best-effort doc-comment text — populated when the language-specific
    /// heuristic (see module docs) finds a doc comment attached to the
    /// symbol; `None` otherwise.
    pub doc_comment: Option<String>,
}

// ── Language (de)serialization helper ──────────────────────────────────────
//
// The [`Language`] enum lives in `parser.rs` which is frozen byte-for-byte
// by this work-order's acceptance criteria, so we implement Serde support
// out-of-band via a `#[serde(with = "…")]` module targeting only the
// `ExtractedSymbol::language` field.  Language values serialize to a
// lowercase string tag matching `SymbolKind`'s `rename_all = "snake_case"`
// style.

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

// ── Public entry-point ─────────────────────────────────────────────────────

/// Stateless symbol extractor backed by per-language tree-sitter queries.
///
/// `SymbolExtractor` holds no state; calls are cheap and may be issued
/// concurrently.  Construct one via [`SymbolExtractor::new`] (or
/// [`SymbolExtractor::default`]) and call [`extract`][Self::extract] once
/// per source file.
///
/// # Examples
///
/// ```
/// use std::path::Path;
///
/// use ucil_treesitter::parser::{Language, Parser};
/// use ucil_treesitter::symbols::{SymbolExtractor, SymbolKind};
///
/// let mut p = Parser::new();
/// let src = "fn main() {}";
/// let tree = p.parse(src, Language::Rust).unwrap();
/// let extractor = SymbolExtractor::new();
/// let syms = extractor.extract(&tree, src, Path::new("main.rs"), Language::Rust);
/// assert_eq!(syms.len(), 1);
/// assert_eq!(syms[0].name, "main");
/// assert_eq!(syms[0].kind, SymbolKind::Function);
/// ```
#[derive(Debug, Default, Clone, Copy)]
pub struct SymbolExtractor;

impl SymbolExtractor {
    /// Create a new `SymbolExtractor`.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Extract named symbols from `tree` / `source` for the given `lang`.
    ///
    /// `file_path` is copied verbatim into every returned symbol so the
    /// caller can build a language-tagged index without a second pass.
    ///
    /// Returns an empty `Vec` for fallback languages (Java, C, C++, Ruby,
    /// Bash, JSON) — these are reserved for future richer extraction.
    /// [`extract`] never panics on any [`Language`] variant.
    ///
    /// [`extract`]: Self::extract
    #[must_use]
    #[tracing::instrument(
        name = "ucil.treesitter.extract_symbols",
        level = "debug",
        skip_all,
        fields(language = ?lang, file = %file_path.display())
    )]
    pub fn extract(
        &self,
        tree: &Tree,
        source: &str,
        file_path: &Path,
        lang: Language,
    ) -> Vec<ExtractedSymbol> {
        let symbols = match lang {
            Language::Rust => extract_rust(tree, source, file_path),
            Language::Python => extract_python(tree, source, file_path),
            Language::TypeScript | Language::JavaScript => {
                extract_ts_js(tree, source, file_path, lang)
            }
            Language::Go => extract_go(tree, source, file_path),
            Language::Java
            | Language::C
            | Language::Cpp
            | Language::Ruby
            | Language::Bash
            | Language::Json => Vec::new(),
        };
        tracing::debug!(count = symbols.len(), "extracted symbols");
        symbols
    }
}

// ── Shared helpers ─────────────────────────────────────────────────────────

/// Return the source text covered by `node`, clamped to `source.len()`.
fn node_text<'src>(node: Node<'_>, source: &'src str) -> &'src str {
    let start = node.start_byte().min(source.len());
    let end = node.end_byte().min(source.len());
    &source[start..end]
}

/// Convert a tree-sitter row / column `usize` to `u32`, saturating at
/// `u32::MAX`.
///
/// Source files do not realistically exceed `u32::MAX` lines or columns;
/// the saturation is a safety net that avoids a panic should a rogue file
/// ever exceed it.
#[inline]
fn usize_to_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// Return `true` if any ancestor of `node` has `kind() == kind`.
fn has_ancestor_kind(mut node: Node<'_>, kind: &str) -> bool {
    while let Some(parent) = node.parent() {
        if parent.kind() == kind {
            return true;
        }
        node = parent;
    }
    false
}

/// Build a best-effort single-line signature for a function / method node.
///
/// Returns the source substring from `node.start_byte()` up to (but not
/// including) the first `{` or `;`, trimmed, collapsed to a single line,
/// and truncated at [`SIG_CAP`] bytes.
fn signature_from_node(node: Node<'_>, source: &str) -> String {
    let text = node_text(node, source);
    let end = text.find(['{', ';']).unwrap_or(text.len());
    let head = &text[..end];
    let one_line: String = head
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let trimmed = one_line.trim();
    if trimmed.len() <= SIG_CAP {
        trimmed.to_owned()
    } else {
        // Truncate on a char boundary to stay `str`-safe.
        let mut cut = SIG_CAP;
        while cut > 0 && !trimmed.is_char_boundary(cut) {
            cut -= 1;
        }
        trimmed[..cut].to_owned()
    }
}

/// Iterate a compiled query against `root` and return the (sym-node, name)
/// pairs it matches.
///
/// `sym_capture` names the capture that binds the full symbol span;
/// `name_capture` names the capture that binds the identifier text.  If
/// either capture name is absent the function returns an empty `Vec`.
fn run_query<'tree>(
    query: &Query,
    root: Node<'tree>,
    source: &str,
    sym_capture: &str,
    name_capture: &str,
) -> Vec<(Node<'tree>, String)> {
    let Some(sym_idx) = query.capture_index_for_name(sym_capture) else {
        return Vec::new();
    };
    let Some(name_idx) = query.capture_index_for_name(name_capture) else {
        return Vec::new();
    };

    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, root, source.as_bytes());

    let mut out = Vec::new();
    // `QueryMatches` implements `StreamingIterator`, not `Iterator`.
    // `Node<'tree>` is `Copy`, so we can safely copy nodes out of each
    // match before the iterator advances its internal buffer.
    while let Some(m) = matches.next() {
        let sym_node = m
            .captures
            .iter()
            .find(|c| c.index == sym_idx)
            .map(|c| c.node);
        let name_node = m
            .captures
            .iter()
            .find(|c| c.index == name_idx)
            .map(|c| c.node);
        if let (Some(sym), Some(name_n)) = (sym_node, name_node) {
            let name = node_text(name_n, source).to_owned();
            if !name.is_empty() {
                out.push((sym, name));
            }
        }
    }
    out
}

/// Free-form metadata attached to an extracted symbol — the optional
/// `signature` and `doc_comment` fields, bundled so [`make_symbol`] stays
/// under the clippy `too_many_arguments` cap.
#[derive(Debug, Default)]
struct SymbolMeta {
    signature: Option<String>,
    doc_comment: Option<String>,
}

impl SymbolMeta {
    const fn none() -> Self {
        Self {
            signature: None,
            doc_comment: None,
        }
    }
}

/// Build an [`ExtractedSymbol`] for `sym_node`, given the non-node scalars
/// the caller already has on hand.
fn make_symbol(
    sym_node: Node<'_>,
    name: String,
    kind: SymbolKind,
    file_path: &Path,
    lang: Language,
    meta: SymbolMeta,
) -> ExtractedSymbol {
    let start = sym_node.start_position();
    let end = sym_node.end_position();
    ExtractedSymbol {
        name,
        kind,
        file_path: file_path.to_path_buf(),
        language: lang,
        start_line: usize_to_u32(start.row).saturating_add(1),
        start_col: usize_to_u32(start.column).saturating_add(1),
        end_line: usize_to_u32(end.row).saturating_add(1),
        end_col: usize_to_u32(end.column).saturating_add(1),
        signature: meta.signature,
        doc_comment: meta.doc_comment,
    }
}

// ── Rust extraction ────────────────────────────────────────────────────────

fn extract_rust(tree: &Tree, source: &str, file_path: &Path) -> Vec<ExtractedSymbol> {
    let lang: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
    let mut out = Vec::new();

    // Function bodies (`fn foo() { … }`) — classified Method when nested
    // inside an `impl_item` or `trait_item`, Function otherwise.
    rust_collect_function_items(&lang, tree, source, file_path, "function_item", &mut out);
    // Trait-method signatures (`fn foo(&self);`) — always `Method`.
    rust_collect_function_items(
        &lang,
        tree,
        source,
        file_path,
        "function_signature_item",
        &mut out,
    );

    // Other top-level items with an identifier-typed `name` field.
    let simple_rules: &[(&str, &str, SymbolKind)] = &[
        ("struct_item", "type_identifier", SymbolKind::Struct),
        ("enum_item", "type_identifier", SymbolKind::Enum),
        ("trait_item", "type_identifier", SymbolKind::Trait),
        ("const_item", "identifier", SymbolKind::Constant),
        ("type_item", "type_identifier", SymbolKind::TypeAlias),
        ("mod_item", "identifier", SymbolKind::Module),
    ];
    for (node_kind, ident_kind, kind) in simple_rules {
        let q_str = format!("({node_kind} name: ({ident_kind}) @name) @sym");
        let Ok(query) = Query::new(&lang, &q_str) else {
            tracing::warn!(query = %q_str, "Rust query compile failed");
            continue;
        };
        for (sym_node, name) in run_query(&query, tree.root_node(), source, "sym", "name") {
            let meta = SymbolMeta {
                signature: None,
                doc_comment: rust_doc_comment_preceding(sym_node, source),
            };
            out.push(make_symbol(
                sym_node,
                name,
                *kind,
                file_path,
                Language::Rust,
                meta,
            ));
        }
    }

    out
}

fn rust_collect_function_items(
    lang: &tree_sitter::Language,
    tree: &Tree,
    source: &str,
    file_path: &Path,
    node_kind: &str,
    out: &mut Vec<ExtractedSymbol>,
) {
    let q_str = format!("({node_kind} name: (identifier) @name) @sym");
    let Ok(query) = Query::new(lang, &q_str) else {
        tracing::warn!(query = %q_str, "Rust function query compile failed");
        return;
    };
    for (sym_node, name) in run_query(&query, tree.root_node(), source, "sym", "name") {
        let kind = if has_ancestor_kind(sym_node, "impl_item")
            || has_ancestor_kind(sym_node, "trait_item")
        {
            SymbolKind::Method
        } else {
            SymbolKind::Function
        };
        let meta = SymbolMeta {
            signature: Some(signature_from_node(sym_node, source)),
            doc_comment: rust_doc_comment_preceding(sym_node, source),
        };
        out.push(make_symbol(
            sym_node,
            name,
            kind,
            file_path,
            Language::Rust,
            meta,
        ));
    }
}

/// Walk backward from `node`'s preceding sibling and collect contiguous
/// `line_comment` nodes whose raw source starts with `///`.  Returns the
/// joined doc-comment text in original order (with newlines between
/// lines, each line's trailing newline stripped, and the `///` prefix
/// retained verbatim) or `None` if no doc comment is present.
fn rust_doc_comment_preceding(node: Node<'_>, source: &str) -> Option<String> {
    let mut lines: Vec<&str> = Vec::new();
    let mut cursor = node.prev_sibling();
    while let Some(sib) = cursor {
        if sib.kind() != "line_comment" {
            break;
        }
        // Trim any trailing `\r`/`\n` the grammar includes in the node's
        // span so the joined output is readable ("/// a\n/// b", not
        // "/// a\n\n/// b\n").
        let text = node_text(sib, source).trim_end_matches(['\r', '\n']);
        if !text.starts_with("///") {
            break;
        }
        lines.push(text);
        cursor = sib.prev_sibling();
    }
    if lines.is_empty() {
        None
    } else {
        lines.reverse();
        Some(lines.join("\n"))
    }
}

// ── Python extraction ──────────────────────────────────────────────────────

fn extract_python(tree: &Tree, source: &str, file_path: &Path) -> Vec<ExtractedSymbol> {
    let lang: tree_sitter::Language = tree_sitter_python::LANGUAGE.into();
    let mut out = Vec::new();

    // Class definitions.
    if let Ok(q) = Query::new(&lang, "(class_definition name: (identifier) @name) @sym") {
        for (sym_node, name) in run_query(&q, tree.root_node(), source, "sym", "name") {
            let meta = SymbolMeta {
                signature: None,
                doc_comment: python_docstring(sym_node).map(|s| node_text(s, source).to_owned()),
            };
            out.push(make_symbol(
                sym_node,
                name,
                SymbolKind::Class,
                file_path,
                Language::Python,
                meta,
            ));
        }
    }

    // Function definitions — Method when nested in a class, Function
    // otherwise.
    if let Ok(q) = Query::new(&lang, "(function_definition name: (identifier) @name) @sym") {
        for (sym_node, name) in run_query(&q, tree.root_node(), source, "sym", "name") {
            let kind = if has_ancestor_kind(sym_node, "class_definition") {
                SymbolKind::Method
            } else {
                SymbolKind::Function
            };
            let meta = SymbolMeta {
                signature: Some(signature_from_node(sym_node, source)),
                doc_comment: python_docstring(sym_node).map(|s| node_text(s, source).to_owned()),
            };
            out.push(make_symbol(
                sym_node,
                name,
                kind,
                file_path,
                Language::Python,
                meta,
            ));
        }
    }

    out
}

/// Return the `string` node of a Python function/class body's first
/// statement if it is an `expression_statement` wrapping a string
/// literal (i.e. a docstring).
fn python_docstring(def_node: Node<'_>) -> Option<Node<'_>> {
    let body = def_node.child_by_field_name("body")?;
    let first = body.named_child(0)?;
    if first.kind() != "expression_statement" {
        return None;
    }
    let expr = first.named_child(0)?;
    if expr.kind() == "string" {
        Some(expr)
    } else {
        None
    }
}

// ── TypeScript / JavaScript extraction ─────────────────────────────────────

/// Is `SymbolKind::Function` / `SymbolKind::Method` — kinds whose
/// `signature` field is populated.  All other kinds get `signature: None`.
const fn kind_has_signature(kind: SymbolKind) -> bool {
    matches!(kind, SymbolKind::Function | SymbolKind::Method)
}

/// Immutable context shared by every `ts_js_collect` call inside a single
/// [`extract_ts_js`] invocation — bundled into a struct so the helper
/// stays under the clippy `too_many_arguments` cap.
struct TsJsCtx<'a> {
    ts_lang: &'a tree_sitter::Language,
    tree: &'a Tree,
    source: &'a str,
    file_path: &'a Path,
    lang: Language,
}

/// Collect every match of `query_str` against `ctx.tree`'s root into
/// `out`, classifying each match as `kind`.
fn ts_js_collect(
    ctx: &TsJsCtx<'_>,
    query_str: &str,
    kind: SymbolKind,
    out: &mut Vec<ExtractedSymbol>,
) {
    let Ok(q) = Query::new(ctx.ts_lang, query_str) else {
        tracing::warn!(query = %query_str, "TS/JS query compile failed");
        return;
    };
    for (sym_node, name) in run_query(&q, ctx.tree.root_node(), ctx.source, "sym", "name") {
        let meta = SymbolMeta {
            signature: if kind_has_signature(kind) {
                Some(signature_from_node(sym_node, ctx.source))
            } else {
                None
            },
            doc_comment: ts_doc_comment_preceding(sym_node, ctx.source),
        };
        out.push(make_symbol(
            sym_node,
            name,
            kind,
            ctx.file_path,
            ctx.lang,
            meta,
        ));
    }
}

fn extract_ts_js(
    tree: &Tree,
    source: &str,
    file_path: &Path,
    lang: Language,
) -> Vec<ExtractedSymbol> {
    let ts_lang: tree_sitter::Language = match lang {
        Language::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
        _ => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
    };
    let is_js = matches!(lang, Language::JavaScript);
    let ctx = TsJsCtx {
        ts_lang: &ts_lang,
        tree,
        source,
        file_path,
        lang,
    };
    let mut out = Vec::new();

    // `function foo() { … }`
    ts_js_collect(
        &ctx,
        "(function_declaration name: (identifier) @name) @sym",
        SymbolKind::Function,
        &mut out,
    );

    // `class Foo { … }` — TS uses `type_identifier`, JS uses `identifier`.
    let class_query = if is_js {
        "(class_declaration name: (identifier) @name) @sym"
    } else {
        "(class_declaration name: (type_identifier) @name) @sym"
    };
    ts_js_collect(&ctx, class_query, SymbolKind::Class, &mut out);

    // `class Foo { bar() {} }` — methods.
    ts_js_collect(
        &ctx,
        "(method_definition name: (property_identifier) @name) @sym",
        SymbolKind::Method,
        &mut out,
    );

    // TypeScript-only: interface + type alias declarations.
    if !is_js {
        ts_js_collect(
            &ctx,
            "(interface_declaration name: (type_identifier) @name) @sym",
            SymbolKind::Interface,
            &mut out,
        );
        ts_js_collect(
            &ctx,
            "(type_alias_declaration name: (type_identifier) @name) @sym",
            SymbolKind::TypeAlias,
            &mut out,
        );
    }

    out
}

/// Return the text of `node`'s immediately preceding `comment` sibling if
/// it starts with `/**` (`TSDoc` / `JSDoc`); otherwise `None`.
fn ts_doc_comment_preceding(node: Node<'_>, source: &str) -> Option<String> {
    let sib = node.prev_sibling()?;
    if sib.kind() != "comment" {
        return None;
    }
    let text = node_text(sib, source);
    if text.starts_with("/**") {
        Some(text.to_owned())
    } else {
        None
    }
}

// ── Go extraction ──────────────────────────────────────────────────────────

fn extract_go(tree: &Tree, source: &str, file_path: &Path) -> Vec<ExtractedSymbol> {
    let lang: tree_sitter::Language = tree_sitter_go::LANGUAGE.into();
    let mut out = Vec::new();

    if let Ok(q) = Query::new(
        &lang,
        "(function_declaration name: (identifier) @name) @sym",
    ) {
        for (sym_node, name) in run_query(&q, tree.root_node(), source, "sym", "name") {
            let meta = SymbolMeta {
                signature: Some(signature_from_node(sym_node, source)),
                doc_comment: None,
            };
            out.push(make_symbol(
                sym_node,
                name,
                SymbolKind::Function,
                file_path,
                Language::Go,
                meta,
            ));
        }
    }

    if let Ok(q) = Query::new(
        &lang,
        "(method_declaration name: (field_identifier) @name) @sym",
    ) {
        for (sym_node, name) in run_query(&q, tree.root_node(), source, "sym", "name") {
            let meta = SymbolMeta {
                signature: Some(signature_from_node(sym_node, source)),
                doc_comment: None,
            };
            out.push(make_symbol(
                sym_node,
                name,
                SymbolKind::Method,
                file_path,
                Language::Go,
                meta,
            ));
        }
    }

    // `type Bar struct {}` / `type Bar interface {}` / `type Bar Foo`.
    // `type_spec` is the child of `type_declaration` carrying the name +
    // the RHS (struct_type / interface_type / other).  Inspect the RHS to
    // classify.
    if let Ok(q) = Query::new(&lang, "(type_spec name: (type_identifier) @name) @sym") {
        for (sym_node, name) in run_query(&q, tree.root_node(), source, "sym", "name") {
            let rhs_kind = sym_node
                .child_by_field_name("type")
                .map(|n| n.kind().to_owned());
            let kind = match rhs_kind.as_deref() {
                Some("struct_type") => SymbolKind::Struct,
                Some("interface_type") => SymbolKind::Interface,
                _ => SymbolKind::TypeAlias,
            };
            out.push(make_symbol(
                sym_node,
                name,
                kind,
                file_path,
                Language::Go,
                SymbolMeta::none(),
            ));
        }
    }

    out
}

// ── Module-root unit tests ─────────────────────────────────────────────────
//
// Per DEC-0005 (WO-0006 module-coherence commits), unit tests live at
// module root — NOT wrapped in `#[cfg(test)] mod tests { … }` — so the
// frozen acceptance selector `symbols::` resolves every test as
// `ucil_treesitter::symbols::<test_name>`.

#[cfg(test)]
use crate::parser::{Parser, SUPPORTED_LANGUAGES};

#[cfg(test)]
fn parse_and_extract(src: &str, lang: Language, path: &str) -> Vec<ExtractedSymbol> {
    let mut parser = Parser::new();
    let tree = parser.parse(src, lang).expect("parse must succeed");
    SymbolExtractor::new().extract(&tree, src, Path::new(path), lang)
}

#[cfg(test)]
#[test]
fn symbols_extract_rust_fn_and_struct_and_enum() {
    let src = "fn foo() {} struct Bar {} enum Baz { A, B }";
    let syms = parse_and_extract(src, Language::Rust, "x.rs");
    assert_eq!(
        syms.len(),
        3,
        "expected exactly 3 symbols, got {:?}",
        syms.iter().map(|s| (&s.name, s.kind)).collect::<Vec<_>>()
    );
    let pairs: std::collections::HashSet<(String, SymbolKind)> =
        syms.iter().map(|s| (s.name.clone(), s.kind)).collect();
    let expected: std::collections::HashSet<(String, SymbolKind)> = [
        ("foo".to_owned(), SymbolKind::Function),
        ("Bar".to_owned(), SymbolKind::Struct),
        ("Baz".to_owned(), SymbolKind::Enum),
    ]
    .into_iter()
    .collect();
    assert_eq!(pairs, expected);
    for s in &syms {
        assert_eq!(s.language, Language::Rust);
        assert_eq!(s.file_path, PathBuf::from("x.rs"));
    }
}

#[cfg(test)]
#[test]
fn symbols_extract_rust_trait_and_method() {
    let src = "trait T { fn m(&self); } impl T for () { fn m(&self) {} }";
    let syms = parse_and_extract(src, Language::Rust, "trait.rs");
    let traits: Vec<&ExtractedSymbol> = syms
        .iter()
        .filter(|s| s.kind == SymbolKind::Trait)
        .collect();
    assert_eq!(traits.len(), 1, "expected one Trait, got {traits:?}");
    assert_eq!(traits[0].name, "T");

    let has_method_m = syms
        .iter()
        .any(|s| s.kind == SymbolKind::Method && s.name == "m");
    assert!(
        has_method_m,
        "expected at least one Method named 'm', got {syms:?}"
    );
}

#[cfg(test)]
#[test]
fn symbols_extract_python_def_and_class_and_method() {
    let src = "def foo():\n    pass\n\nclass Bar:\n    def baz(self):\n        pass\n";
    let syms = parse_and_extract(src, Language::Python, "mod.py");
    let find = |kind: SymbolKind, name: &str| syms.iter().any(|s| s.kind == kind && s.name == name);
    assert!(
        find(SymbolKind::Function, "foo"),
        "expected Function(foo), got {syms:?}"
    );
    assert!(
        find(SymbolKind::Class, "Bar"),
        "expected Class(Bar), got {syms:?}"
    );
    assert!(
        find(SymbolKind::Method, "baz"),
        "expected Method(baz), got {syms:?}"
    );
    // `baz` must NOT be classified as a bare Function — ancestor-scan
    // disambiguates it as a Method.
    assert!(
        !syms
            .iter()
            .any(|s| s.kind == SymbolKind::Function && s.name == "baz"),
        "'baz' should be classified as Method (inside class_definition), got {syms:?}"
    );
}

#[cfg(test)]
#[test]
fn symbols_extract_typescript_function_and_class_and_interface() {
    let src = "function foo(){} class B {} interface I { x: number; }";
    let syms = parse_and_extract(src, Language::TypeScript, "a.ts");
    let has = |kind: SymbolKind, name: &str| syms.iter().any(|s| s.kind == kind && s.name == name);
    assert!(
        has(SymbolKind::Function, "foo"),
        "expected Function(foo), got {syms:?}"
    );
    assert!(
        has(SymbolKind::Class, "B"),
        "expected Class(B), got {syms:?}"
    );
    assert!(
        has(SymbolKind::Interface, "I"),
        "expected Interface(I), got {syms:?}"
    );
    for s in &syms {
        assert_eq!(s.language, Language::TypeScript);
    }
}

#[cfg(test)]
#[test]
fn symbols_extract_javascript_function_declaration() {
    let src = "function foo() { return 1; }";
    let syms = parse_and_extract(src, Language::JavaScript, "a.js");
    let funcs: Vec<&ExtractedSymbol> = syms
        .iter()
        .filter(|s| s.kind == SymbolKind::Function && s.name == "foo")
        .collect();
    assert_eq!(
        funcs.len(),
        1,
        "expected exactly one Function(foo), got {syms:?}"
    );
    assert_eq!(funcs[0].language, Language::JavaScript);
}

#[cfg(test)]
#[test]
fn symbols_extract_go_func_and_type() {
    let src = "package p\nfunc Foo() {}\ntype Bar struct {}\n";
    let syms = parse_and_extract(src, Language::Go, "a.go");
    let has = |kind: SymbolKind, name: &str| syms.iter().any(|s| s.kind == kind && s.name == name);
    assert!(
        has(SymbolKind::Function, "Foo"),
        "expected Function(Foo), got {syms:?}"
    );
    assert!(
        has(SymbolKind::Struct, "Bar"),
        "expected Struct(Bar), got {syms:?}"
    );
}

#[cfg(test)]
#[test]
fn symbols_extract_line_ranges_are_well_formed() {
    let src = "fn foo() {} struct Bar {} enum Baz { A, B }";
    let syms = parse_and_extract(src, Language::Rust, "x.rs");
    assert!(!syms.is_empty(), "expected ≥1 symbol");
    for s in &syms {
        assert!(s.start_line >= 1, "{s:?} start_line must be ≥1");
        assert!(
            s.end_line >= s.start_line,
            "{s:?} end_line must be ≥ start_line"
        );
        assert!(s.start_col >= 1, "{s:?} start_col must be ≥1");
        assert!(s.end_col >= 1, "{s:?} end_col must be ≥1");
    }
}

#[cfg(test)]
#[test]
fn symbols_extract_signature_captures_rust_fn_declaration() {
    let src = "fn add(x: i32, y: i32) -> i32 { x + y }";
    let syms = parse_and_extract(src, Language::Rust, "math.rs");
    let add = syms
        .iter()
        .find(|s| s.name == "add")
        .expect("add symbol must be extracted");
    let sig = add
        .signature
        .as_ref()
        .expect("add must have a Some signature");
    assert!(
        sig.contains("fn add(x: i32, y: i32) -> i32"),
        "signature {sig:?} must contain the declaration header"
    );
    assert!(
        !sig.contains('{'),
        "signature {sig:?} must not contain the body-opening brace"
    );
    assert!(
        !sig.contains("x + y"),
        "signature {sig:?} must not contain body text"
    );
}

#[cfg(test)]
#[test]
fn symbols_extract_doc_comment_for_rust_fn() {
    let src = "/// double a number\nfn dbl(x: i32) -> i32 { x * 2 }";
    let syms = parse_and_extract(src, Language::Rust, "d.rs");
    let dbl = syms
        .iter()
        .find(|s| s.name == "dbl")
        .expect("dbl must be extracted");
    assert_eq!(
        dbl.doc_comment.as_deref(),
        Some("/// double a number"),
        "doc_comment is returned raw (prefix retained); got {:?}",
        dbl.doc_comment
    );
}

#[cfg(test)]
#[test]
fn symbols_extract_empty_source_returns_empty_vec() {
    for &lang in SUPPORTED_LANGUAGES {
        let syms = parse_and_extract("", lang, "empty.src");
        assert!(
            syms.is_empty(),
            "empty source for {lang:?} must return empty Vec, got {syms:?}"
        );
    }
}

#[cfg(test)]
#[test]
fn symbols_extract_all_supported_languages_no_panic() {
    for &lang in SUPPORTED_LANGUAGES {
        // Comment-only placeholder — valid for every language's parser at
        // minimum as content-free input (tree-sitter returns an error-free
        // tree or a tree whose root is-error without panicking).
        let syms = parse_and_extract("// placeholder", lang, "placeholder.txt");
        // Length is not asserted — each grammar decides independently
        // whether `// placeholder` parses to any symbols.  The invariant
        // is merely "no panic".  Consume the returned Vec so it isn't
        // optimised away and the extract call is actually driven.
        let _count = syms.len();
    }
}

#[cfg(test)]
#[test]
fn symbols_extract_fallback_languages_return_empty() {
    // `// placeholder` happens to be valid C/C++/Java/Ruby (where `//`
    // begins a line comment); for Bash / JSON the parser returns error
    // nodes — in all cases `extract` returns `Vec::new()` by fallback.
    for lang in [
        Language::Java,
        Language::C,
        Language::Cpp,
        Language::Ruby,
        Language::Bash,
        Language::Json,
    ] {
        let syms = parse_and_extract("// placeholder", lang, "fallback.src");
        assert!(
            syms.is_empty(),
            "fallback language {lang:?} must return empty Vec, got {syms:?}"
        );
    }
}

#[cfg(test)]
#[test]
fn symbols_extract_language_field_populated_correctly() {
    let cases: &[(Language, &str)] = &[
        (Language::Rust, "fn foo() {}"),
        (Language::Python, "def foo():\n    pass\n"),
        (Language::TypeScript, "function foo(){}"),
        (Language::Go, "package p\nfunc Foo() {}\n"),
    ];
    for (lang, src) in cases {
        let syms = parse_and_extract(src, *lang, "x.src");
        assert!(
            !syms.is_empty(),
            "expected ≥1 symbol for {lang:?} / {src:?}"
        );
        for s in &syms {
            assert_eq!(
                s.language, *lang,
                "extracted symbol {s:?} must report language={lang:?}"
            );
        }
    }
}
