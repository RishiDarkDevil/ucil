//! Symbol extraction from tree-sitter parse trees.
//!
//! Walks a [`tree_sitter::Tree`] using per-language query strings to extract
//! named symbols (functions, classes, structs, etc.) and returns them as
//! [`ExtractedSymbol`] values.
//!
//! # Supported languages
//!
//! | Language           | Node types extracted                               |
//! |--------------------|-----------------------------------------------------|
//! | Rust               | `function_item`, `struct_item`, `enum_item`,        |
//! |                    | `trait_item`, `const_item`, `type_item`, `mod_item` |
//! | Python             | `class_definition`, `function_definition`,          |
//! |                    | `decorated_definition`                              |
//! | TypeScript / JS    | `function_declaration`, `method_definition`,        |
//! |                    | `class_declaration`, arrow `variable_declarator`    |
//!
//! Other languages (Go, Java, C, C++, Ruby, Bash, JSON) return an empty
//! `Vec` — extraction for those languages is added in later work-orders.

// Public API items intentionally share a name prefix with the module.
#![allow(clippy::module_name_repetitions)]

use tree_sitter::{Node, Query, QueryCursor, Tree};

use crate::parser::Language;

// ── Types ──────────────────────────────────────────────────────────────────

/// The kind of a symbol extracted from source code.
///
/// This enum is `#[non_exhaustive]`; new variants may be added as more
/// language grammars are integrated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SymbolKind {
    /// A free function or standalone function declaration.
    Function,
    /// A class definition (Python, TypeScript, JavaScript).
    Class,
    /// A method inside a class body or `impl` block.
    Method,
    /// A struct type (Rust).
    Struct,
    /// An enum type (Rust).
    Enum,
    /// A trait definition (Rust).
    Trait,
    /// A constant item (`const` in Rust/TypeScript/JavaScript).
    Const,
    /// A type alias (`type Foo = …`).
    TypeAlias,
    /// A module declaration (`mod foo { … }` in Rust).
    Module,
    /// An interface declaration (TypeScript).
    Interface,
    /// A constructor method (`__init__`, TypeScript `constructor`, etc.).
    Constructor,
    /// A field or property declaration.
    Field,
}

/// A named symbol extracted from a parse tree.
#[derive(Debug, Clone)]
pub struct ExtractedSymbol {
    /// The symbol's declared name (e.g. `"my_func"`, `"MyStruct"`).
    pub name: String,
    /// What kind of syntactic construct this symbol is.
    pub kind: SymbolKind,
    /// Zero-based row of the first character of the symbol node.
    pub start_line: u32,
    /// Zero-based row of the last character of the symbol node.
    pub end_line: u32,
    /// First line of the symbol node, trimmed and capped at 256 chars.
    /// Useful as a short human-readable signature for display.
    pub signature: String,
}

// ── Public entry-point ─────────────────────────────────────────────────────

/// Stateless symbol extractor backed by per-language tree-sitter queries.
pub struct SymbolExtractor;

impl SymbolExtractor {
    /// Extract named symbols from `tree` / `source` for the given `lang`.
    ///
    /// Returns an empty `Vec` for languages that are not yet wired up.
    ///
    /// # Examples
    ///
    /// ```
    /// use ucil_treesitter::parser::{Language, Parser};
    /// use ucil_treesitter::symbols::SymbolExtractor;
    ///
    /// let mut p = Parser::new();
    /// let src = "fn main() {}";
    /// let tree = p.parse(src, Language::Rust).unwrap();
    /// let syms = SymbolExtractor::extract(&tree, src, &Language::Rust);
    /// assert_eq!(syms.len(), 1);
    /// assert_eq!(syms[0].name, "main");
    /// ```
    #[must_use]
    pub fn extract(tree: &Tree, source: &str, lang: &Language) -> Vec<ExtractedSymbol> {
        match lang {
            Language::Rust => extract_rust(tree, source),
            Language::Python => extract_python(tree, source),
            Language::TypeScript => extract_ts_js(tree, source, false),
            Language::JavaScript => extract_ts_js(tree, source, true),
            _ => Vec::new(),
        }
    }
}

// ── Shared helpers ─────────────────────────────────────────────────────────

/// Return the first line of a node's source text, trimmed, capped at 256 chars.
fn signature_from_node(node: Node<'_>, source: &str) -> String {
    let start = node.start_byte();
    let end = node.end_byte().min(source.len());
    source[start..end]
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .chars()
        .take(256)
        .collect()
}

/// Return the source text of a node.
fn node_text<'src>(node: Node<'_>, source: &'src str) -> &'src str {
    let start = node.start_byte();
    let end = node.end_byte().min(source.len());
    &source[start..end]
}

/// Return `true` if any ancestor of `node` has `node.kind() == kind`.
fn has_ancestor_kind(mut node: Node<'_>, kind: &str) -> bool {
    while let Some(parent) = node.parent() {
        if parent.kind() == kind {
            return true;
        }
        node = parent;
    }
    false
}

/// Convert a tree-sitter row index (`usize`) to `u32`.
///
/// Source files never have more than ~4 billion lines; the `u32::MAX` cap is
/// a safety net, not an expected code path.
#[inline]
fn usize_to_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// Run a single compiled query and return `(sym_node, name_text)` pairs.
///
/// `sym_capture` names the capture that gives the full symbol span;
/// `name_capture` names the capture that gives the identifier text.
///
/// Uses the `StreamingIterator` API required by tree-sitter ≥ 0.24.
fn run_query<'tree>(
    query: &Query,
    root: Node<'tree>,
    source: &str,
    sym_capture: &str,
    name_capture: &str,
) -> Vec<(Node<'tree>, String)> {
    use streaming_iterator::StreamingIterator as _;

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
    // `Node<'tree>` is `Copy`, so we can safely copy nodes out of each match
    // before the iterator advances its internal buffer.
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

// ── Rust extraction ────────────────────────────────────────────────────────

fn extract_rust(tree: &Tree, source: &str) -> Vec<ExtractedSymbol> {
    let lang: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();

    // Each entry: (query_string, SymbolKind)
    // All queries use @sym for the full-span capture and @name for the identifier.
    let rules: &[(&str, SymbolKind)] = &[
        (
            "(function_item name: (identifier) @name) @sym",
            SymbolKind::Function,
        ),
        (
            "(struct_item name: (type_identifier) @name) @sym",
            SymbolKind::Struct,
        ),
        (
            "(enum_item name: (type_identifier) @name) @sym",
            SymbolKind::Enum,
        ),
        (
            "(trait_item name: (type_identifier) @name) @sym",
            SymbolKind::Trait,
        ),
        (
            "(const_item name: (identifier) @name) @sym",
            SymbolKind::Const,
        ),
        (
            "(type_item name: (type_identifier) @name) @sym",
            SymbolKind::TypeAlias,
        ),
        (
            "(mod_item name: (identifier) @name) @sym",
            SymbolKind::Module,
        ),
    ];

    let mut symbols = Vec::new();
    for (q_str, kind) in rules {
        let query = match Query::new(&lang, q_str) {
            Ok(q) => q,
            Err(e) => {
                tracing::warn!(query = q_str, error = ?e, "Rust query compile failed");
                continue;
            }
        };
        for (sym_node, name) in run_query(&query, tree.root_node(), source, "sym", "name") {
            symbols.push(ExtractedSymbol {
                name,
                kind: *kind,
                start_line: usize_to_u32(sym_node.start_position().row),
                end_line: usize_to_u32(sym_node.end_position().row),
                signature: signature_from_node(sym_node, source),
            });
        }
    }
    symbols
}

// ── Python extraction ──────────────────────────────────────────────────────

fn extract_python(tree: &Tree, source: &str) -> Vec<ExtractedSymbol> {
    let lang: tree_sitter::Language = tree_sitter_python::LANGUAGE.into();
    let mut symbols = Vec::new();

    // ── class_definition (skip those whose direct parent is decorated_definition) ──
    {
        let q_str = "(class_definition name: (identifier) @name) @sym";
        if let Ok(q) = Query::new(&lang, q_str) {
            for (sym_node, name) in run_query(&q, tree.root_node(), source, "sym", "name") {
                if sym_node
                    .parent()
                    .is_some_and(|p| p.kind() == "decorated_definition")
                {
                    // Handled by the decorated_definition pass below.
                    continue;
                }
                symbols.push(ExtractedSymbol {
                    name,
                    kind: SymbolKind::Class,
                    start_line: usize_to_u32(sym_node.start_position().row),
                    end_line: usize_to_u32(sym_node.end_position().row),
                    signature: signature_from_node(sym_node, source),
                });
            }
        }
    }

    // ── function_definition (skip decorated; classify Method vs Function) ──
    {
        let q_str = "(function_definition name: (identifier) @name) @sym";
        if let Ok(q) = Query::new(&lang, q_str) {
            for (sym_node, name) in run_query(&q, tree.root_node(), source, "sym", "name") {
                if sym_node
                    .parent()
                    .is_some_and(|p| p.kind() == "decorated_definition")
                {
                    // Handled by the decorated_definition pass below.
                    continue;
                }
                let kind = if has_ancestor_kind(sym_node, "class_definition") {
                    SymbolKind::Method
                } else {
                    SymbolKind::Function
                };
                symbols.push(ExtractedSymbol {
                    name,
                    kind,
                    start_line: usize_to_u32(sym_node.start_position().row),
                    end_line: usize_to_u32(sym_node.end_position().row),
                    signature: signature_from_node(sym_node, source),
                });
            }
        }
    }

    // ── decorated_definition wrapping function_definition ──
    {
        // The @sym capture is the outer decorated_definition node (for span).
        // The @name capture is the identifier inside the inner function_definition.
        let q_str = "(decorated_definition (function_definition name: (identifier) @name)) @sym";
        if let Ok(q) = Query::new(&lang, q_str) {
            for (sym_node, name) in run_query(&q, tree.root_node(), source, "sym", "name") {
                let kind = if has_ancestor_kind(sym_node, "class_definition") {
                    SymbolKind::Method
                } else {
                    SymbolKind::Function
                };
                symbols.push(ExtractedSymbol {
                    name,
                    kind,
                    start_line: usize_to_u32(sym_node.start_position().row),
                    end_line: usize_to_u32(sym_node.end_position().row),
                    signature: signature_from_node(sym_node, source),
                });
            }
        }
    }

    // ── decorated_definition wrapping class_definition ──
    {
        let q_str = "(decorated_definition (class_definition name: (identifier) @name)) @sym";
        if let Ok(q) = Query::new(&lang, q_str) {
            for (sym_node, name) in run_query(&q, tree.root_node(), source, "sym", "name") {
                symbols.push(ExtractedSymbol {
                    name,
                    kind: SymbolKind::Class,
                    start_line: usize_to_u32(sym_node.start_position().row),
                    end_line: usize_to_u32(sym_node.end_position().row),
                    signature: signature_from_node(sym_node, source),
                });
            }
        }
    }

    symbols
}

// ── TypeScript / JavaScript extraction ────────────────────────────────────

/// `is_js = true` uses the JavaScript grammar; `false` uses TypeScript.
fn extract_ts_js(tree: &Tree, source: &str, is_js: bool) -> Vec<ExtractedSymbol> {
    let lang: tree_sitter::Language = if is_js {
        tree_sitter_javascript::LANGUAGE.into()
    } else {
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
    };

    let mut symbols = Vec::new();

    // function_declaration — `identifier` in both TS and JS
    {
        let q_str = "(function_declaration name: (identifier) @name) @sym";
        if let Ok(q) = Query::new(&lang, q_str) {
            for (sym_node, name) in run_query(&q, tree.root_node(), source, "sym", "name") {
                symbols.push(ExtractedSymbol {
                    name,
                    kind: SymbolKind::Function,
                    start_line: usize_to_u32(sym_node.start_position().row),
                    end_line: usize_to_u32(sym_node.end_position().row),
                    signature: signature_from_node(sym_node, source),
                });
            }
        }
    }

    // class_declaration — TypeScript uses `type_identifier`; JavaScript uses `identifier`
    {
        let q_str = if is_js {
            "(class_declaration name: (identifier) @name) @sym"
        } else {
            "(class_declaration name: (type_identifier) @name) @sym"
        };
        if let Ok(q) = Query::new(&lang, q_str) {
            for (sym_node, name) in run_query(&q, tree.root_node(), source, "sym", "name") {
                symbols.push(ExtractedSymbol {
                    name,
                    kind: SymbolKind::Class,
                    start_line: usize_to_u32(sym_node.start_position().row),
                    end_line: usize_to_u32(sym_node.end_position().row),
                    signature: signature_from_node(sym_node, source),
                });
            }
        }
    }

    // method_definition — `property_identifier` in both TS and JS
    {
        let q_str = "(method_definition name: (property_identifier) @name) @sym";
        if let Ok(q) = Query::new(&lang, q_str) {
            for (sym_node, name) in run_query(&q, tree.root_node(), source, "sym", "name") {
                symbols.push(ExtractedSymbol {
                    name,
                    kind: SymbolKind::Method,
                    start_line: usize_to_u32(sym_node.start_position().row),
                    end_line: usize_to_u32(sym_node.end_position().row),
                    signature: signature_from_node(sym_node, source),
                });
            }
        }
    }

    // Arrow functions: `const foo = () => {}`
    // The variable_declarator node spans the name + arrow body.
    {
        let q_str = "(variable_declarator name: (identifier) @name value: (arrow_function)) @sym";
        if let Ok(q) = Query::new(&lang, q_str) {
            for (sym_node, name) in run_query(&q, tree.root_node(), source, "sym", "name") {
                symbols.push(ExtractedSymbol {
                    name,
                    kind: SymbolKind::Function,
                    start_line: usize_to_u32(sym_node.start_position().row),
                    end_line: usize_to_u32(sym_node.end_position().row),
                    signature: signature_from_node(sym_node, source),
                });
            }
        }
    }

    symbols
}

// ── Unit tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;

    fn parse_and_extract(src: &str, lang: Language) -> Vec<ExtractedSymbol> {
        let mut p = Parser::new();
        let tree = p.parse(src, lang).expect("parse must succeed");
        SymbolExtractor::extract(&tree, src, &lang)
    }

    /// Two standalone Rust functions must each produce one `Function` symbol.
    #[test]
    fn extract_rust_functions() {
        let src = r#"
fn alpha() {}
fn beta(x: i32) -> i32 { x + 1 }
"#;
        let syms = parse_and_extract(src, Language::Rust);
        let funcs: Vec<_> = syms
            .iter()
            .filter(|s| s.kind == SymbolKind::Function)
            .collect();
        assert_eq!(
            funcs.len(),
            2,
            "expected exactly 2 Function symbols, got {funcs:?}"
        );
        let names: Vec<&str> = funcs.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"alpha"), "expected 'alpha' in {names:?}");
        assert!(names.contains(&"beta"), "expected 'beta' in {names:?}");
    }

    /// A struct definition plus an impl block with methods must produce one
    /// `Struct` symbol and one `Function` symbol per method.
    #[test]
    fn extract_rust_struct() {
        let src = r#"
struct Counter {
    value: u32,
}

impl Counter {
    fn new() -> Self {
        Counter { value: 0 }
    }
    fn increment(&mut self) {
        self.value += 1;
    }
}
"#;
        let syms = parse_and_extract(src, Language::Rust);
        let structs: Vec<_> = syms
            .iter()
            .filter(|s| s.kind == SymbolKind::Struct)
            .collect();
        let funcs: Vec<_> = syms
            .iter()
            .filter(|s| s.kind == SymbolKind::Function)
            .collect();
        assert_eq!(structs.len(), 1, "expected 1 Struct, got {structs:?}");
        assert_eq!(structs[0].name, "Counter");
        assert!(
            funcs.len() >= 2,
            "expected ≥2 Function symbols for impl methods, got {funcs:?}"
        );
        let func_names: Vec<&str> = funcs.iter().map(|s| s.name.as_str()).collect();
        assert!(
            func_names.contains(&"new"),
            "expected 'new' in {func_names:?}"
        );
        assert!(
            func_names.contains(&"increment"),
            "expected 'increment' in {func_names:?}"
        );
    }

    /// A Python class with two methods must produce one `Class` and two `Method` symbols.
    #[test]
    fn extract_python_class() {
        let src = "class MyClass:\n    def __init__(self):\n        pass\n    def method(self):\n        pass\n";
        let syms = parse_and_extract(src, Language::Python);
        let classes: Vec<_> = syms
            .iter()
            .filter(|s| s.kind == SymbolKind::Class)
            .collect();
        let methods: Vec<_> = syms
            .iter()
            .filter(|s| s.kind == SymbolKind::Method)
            .collect();
        assert_eq!(classes.len(), 1, "expected 1 Class symbol, got {classes:?}");
        assert_eq!(classes[0].name, "MyClass");
        assert_eq!(
            methods.len(),
            2,
            "expected 2 Method symbols, got {methods:?}"
        );
        let mnames: Vec<&str> = methods.iter().map(|s| s.name.as_str()).collect();
        assert!(
            mnames.contains(&"__init__"),
            "expected '__init__' in {mnames:?}"
        );
        assert!(
            mnames.contains(&"method"),
            "expected 'method' in {mnames:?}"
        );
    }

    /// One TypeScript arrow function and one regular function must produce 2 symbols.
    #[test]
    fn extract_typescript_functions() {
        let src = "const foo = () => {};\nfunction bar() {}\n";
        let syms = parse_and_extract(src, Language::TypeScript);
        let funcs: Vec<_> = syms
            .iter()
            .filter(|s| s.kind == SymbolKind::Function)
            .collect();
        assert_eq!(funcs.len(), 2, "expected 2 Function symbols, got {funcs:?}");
        let names: Vec<&str> = funcs.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"foo"), "expected 'foo' in {names:?}");
        assert!(names.contains(&"bar"), "expected 'bar' in {names:?}");
    }

    /// Empty source must return an empty `Vec`.
    #[test]
    fn extract_empty_source_returns_empty_vec() {
        for lang in [Language::Rust, Language::Python, Language::TypeScript] {
            let syms = parse_and_extract("", lang);
            assert!(
                syms.is_empty(),
                "empty source for {lang:?} must return empty Vec, got {syms:?}"
            );
        }
    }

    /// Every extracted symbol must have a non-empty name.
    #[test]
    fn all_symbols_have_nonempty_names() {
        let rust_src = r#"
fn foo() {}
struct Bar { x: i32 }
enum Baz { A, B }
trait Qux {}
const C: u32 = 0;
type Alias = u32;
mod inner {}
"#;
        let syms = parse_and_extract(rust_src, Language::Rust);
        assert!(!syms.is_empty(), "expected at least one symbol");
        for sym in &syms {
            assert!(!sym.name.is_empty(), "symbol {sym:?} has an empty name");
        }
    }
}
