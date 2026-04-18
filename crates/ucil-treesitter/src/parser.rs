//! Multi-language tree-sitter parser.
//!
//! Exposes a [`Parser`] struct that wraps [`tree_sitter::Parser`] and a
//! [`Language`] enum covering the ≥10 supported grammars.

// The public API re-exported from the module intentionally mirrors the module
// name ("parser" → "Parser", "ParseError") — suppress the pedantic lint.
#![allow(clippy::module_name_repetitions)]

use thiserror::Error;

/// Errors returned by [`Parser::parse`].
#[derive(Debug, Error)]
pub enum ParseError {
    /// Failed to load the tree-sitter language grammar.
    #[error("language load failed: {0}")]
    LanguageLoad(String),
    /// The parser returned `None` — typically a timeout or internal error.
    ///
    /// Note: ordinary syntax errors are represented as error *nodes* in the
    /// returned tree, not as this variant.
    #[error("parse returned None for language {lang:?}: {reason}")]
    ParseFailed {
        /// The language that was being parsed.
        lang: Language,
        /// Human-readable explanation.
        reason: String,
    },
}

/// Languages supported by `ucil-treesitter`.
///
/// Each variant corresponds to one tree-sitter grammar crate bundled with
/// `ucil-treesitter`.  Add new variants here (and to [`SUPPORTED_LANGUAGES`])
/// when additional grammars are integrated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Language {
    /// Rust source code.
    Rust,
    /// Python source code.
    Python,
    /// TypeScript source code.
    TypeScript,
    /// JavaScript source code.
    JavaScript,
    /// Go source code.
    Go,
    /// Java source code.
    Java,
    /// C source code.
    C,
    /// C++ source code.
    Cpp,
    /// Ruby source code.
    Ruby,
    /// Bash / shell scripts.
    Bash,
    /// JSON data files.
    Json,
}

impl Language {
    /// Return the underlying [`tree_sitter::Language`] for this grammar.
    ///
    /// The returned value is created from the grammar crate's `LANGUAGE`
    /// constant via [`From<tree_sitter_language::LanguageFn>`].
    #[must_use]
    pub fn ts_language(self) -> tree_sitter::Language {
        match self {
            Self::Rust => tree_sitter_rust::LANGUAGE.into(),
            Self::Python => tree_sitter_python::LANGUAGE.into(),
            // tree-sitter-typescript exposes separate constants for TS and TSX.
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Self::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Self::Go => tree_sitter_go::LANGUAGE.into(),
            Self::Java => tree_sitter_java::LANGUAGE.into(),
            Self::C => tree_sitter_c::LANGUAGE.into(),
            Self::Cpp => tree_sitter_cpp::LANGUAGE.into(),
            Self::Ruby => tree_sitter_ruby::LANGUAGE.into(),
            Self::Bash => tree_sitter_bash::LANGUAGE.into(),
            Self::Json => tree_sitter_json::LANGUAGE.into(),
        }
    }

    /// Map a filename extension (without the leading `.`) to the
    /// [`Language`] variant whose tree-sitter grammar handles it.
    ///
    /// Consumers downstream of [`crate::Parser`] (for example the
    /// `understand_code` MCP tool handler introduced by WO-0036 and
    /// feature `P1-W4-F09`) need to pick a grammar from the caller's
    /// target-file path alone.  This helper centralises the mapping so
    /// every call site (handler, warm-tier sweep, phase-2 fusion
    /// layer) agrees on which extension routes to which grammar.
    ///
    /// Matching is case-insensitive — `"RS"`, `"Rs"`, and `"rs"` all map
    /// to [`Self::Rust`].  Unknown extensions return `None`; callers
    /// decide whether that is an error or a skip condition.
    ///
    /// References:
    ///   * master-plan §18 Phase 1 Week 2 line 1744 (multi-language
    ///     parser: `≥10 languages`).
    ///   * feature `P1-W4-F09` (`understand_code` — extension → language
    ///     is the first step of file-mode dispatch).
    ///
    /// # Examples
    ///
    /// ```
    /// use ucil_treesitter::Language;
    ///
    /// assert_eq!(Language::from_extension("rs"), Some(Language::Rust));
    /// assert_eq!(Language::from_extension("PY"), Some(Language::Python));
    /// assert_eq!(Language::from_extension("tsx"), Some(Language::TypeScript));
    /// assert_eq!(Language::from_extension("unknown"), None);
    /// ```
    #[must_use]
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "rs" => Some(Self::Rust),
            "py" | "pyi" => Some(Self::Python),
            "ts" | "tsx" | "mts" | "cts" => Some(Self::TypeScript),
            "js" | "jsx" | "mjs" | "cjs" => Some(Self::JavaScript),
            "go" => Some(Self::Go),
            "java" => Some(Self::Java),
            "c" | "h" => Some(Self::C),
            "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => Some(Self::Cpp),
            "rb" => Some(Self::Ruby),
            "sh" | "bash" => Some(Self::Bash),
            "json" => Some(Self::Json),
            _ => None,
        }
    }
}

/// All languages supported by this build of `ucil-treesitter`.
///
/// Contains ≥10 entries — verified by acceptance criterion
/// `grep -c 'Language::' parser.rs | awk '{if($1>=10)exit 0; else exit 1}'`.
pub const SUPPORTED_LANGUAGES: &[Language] = &[
    Language::Rust,
    Language::Python,
    Language::TypeScript,
    Language::JavaScript,
    Language::Go,
    Language::Java,
    Language::C,
    Language::Cpp,
    Language::Ruby,
    Language::Bash,
    Language::Json,
];

/// Multi-language source-code parser backed by tree-sitter.
///
/// Wraps a [`tree_sitter::Parser`] and switches the active language before
/// each [`parse`][Parser::parse] call.
///
/// # Examples
///
/// ```
/// use ucil_treesitter::parser::{Language, Parser};
///
/// let mut p = Parser::new();
/// let tree = p.parse("fn main() {}", Language::Rust).unwrap();
/// assert!(!tree.root_node().is_error());
/// ```
pub struct Parser {
    inner: tree_sitter::Parser,
}

impl Parser {
    /// Create a new `Parser` with no language pre-loaded.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: tree_sitter::Parser::new(),
        }
    }

    /// Parse `source` as the given [`Language`].
    ///
    /// Returns the parsed [`tree_sitter::Tree`].  Syntax errors within the
    /// source are represented as error *nodes* inside the tree — this is
    /// tree-sitter's standard behaviour and does **not** produce a
    /// `ParseError`.
    ///
    /// # Errors
    ///
    /// - [`ParseError::LanguageLoad`] — the grammar could not be loaded
    ///   (ABI version mismatch between the grammar crate and the tree-sitter
    ///   runtime).
    /// - [`ParseError::ParseFailed`] — the parser returned `None`, which
    ///   happens on timeout or certain internal errors.
    pub fn parse(&mut self, source: &str, lang: Language) -> Result<tree_sitter::Tree, ParseError> {
        let ts_lang = lang.ts_language();
        self.inner
            .set_language(&ts_lang)
            .map_err(|e| ParseError::LanguageLoad(e.to_string()))?;
        self.inner
            .parse(source, None)
            .ok_or_else(|| ParseError::ParseFailed {
                lang,
                reason: "parser returned None — possible timeout or internal error".to_owned(),
            })
    }
}

impl Default for Parser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_rust_snippet_succeeds() {
        let mut p = Parser::new();
        let tree = p.parse("fn main() {}", Language::Rust).expect("parse Rust");
        assert!(
            !tree.root_node().is_error(),
            "root node must not be an error node"
        );
    }

    #[test]
    fn parse_valid_python_snippet_succeeds() {
        let mut p = Parser::new();
        let tree = p
            .parse("def double(x):\n    return x * 2\n", Language::Python)
            .expect("parse Python");
        assert!(!tree.root_node().is_error());
    }

    #[test]
    fn parse_valid_typescript_snippet_succeeds() {
        let mut p = Parser::new();
        let tree = p
            .parse("const x: number = 42;", Language::TypeScript)
            .expect("parse TypeScript");
        assert!(!tree.root_node().is_error());
    }

    /// Empty source must return a (minimal) tree, not a `ParseError`.
    ///
    /// tree-sitter can always produce a tree — even for an empty or broken
    /// input — as long as the grammar is loaded correctly.
    #[test]
    fn parse_empty_source_returns_tree_not_error() {
        let mut p = Parser::new();
        let result = p.parse("", Language::Rust);
        assert!(result.is_ok(), "empty source must not produce a ParseError");
    }

    /// Parsing syntactically incorrect content for a language must **not**
    /// panic.  tree-sitter inserts error nodes into the tree instead.
    #[test]
    fn parse_wrong_language_content_does_not_panic() {
        let mut p = Parser::new();
        // Python source parsed as Rust: tree-sitter produces error nodes.
        let _ = p.parse("def foo(): pass", Language::Rust);
    }

    #[test]
    fn supported_languages_has_at_least_ten_entries() {
        assert!(
            SUPPORTED_LANGUAGES.len() >= 10,
            "SUPPORTED_LANGUAGES must contain ≥10 entries, got {}",
            SUPPORTED_LANGUAGES.len()
        );
    }

    #[test]
    fn all_supported_languages_load_without_error() {
        let mut p = Parser::new();
        for &lang in SUPPORTED_LANGUAGES {
            // Parsing a trivial comment works in every language we support.
            let result = p.parse("// test", lang);
            assert!(
                result.is_ok() || matches!(result, Err(ParseError::ParseFailed { .. })),
                "language {lang:?} produced unexpected LanguageLoad error"
            );
        }
    }

    #[test]
    fn from_extension_maps_canonical_extensions() {
        assert_eq!(Language::from_extension("rs"), Some(Language::Rust));
        assert_eq!(Language::from_extension("py"), Some(Language::Python));
        assert_eq!(Language::from_extension("ts"), Some(Language::TypeScript));
        assert_eq!(Language::from_extension("tsx"), Some(Language::TypeScript));
        assert_eq!(Language::from_extension("js"), Some(Language::JavaScript));
        assert_eq!(Language::from_extension("jsx"), Some(Language::JavaScript));
        assert_eq!(Language::from_extension("go"), Some(Language::Go));
        assert_eq!(Language::from_extension("java"), Some(Language::Java));
        assert_eq!(Language::from_extension("c"), Some(Language::C));
        assert_eq!(Language::from_extension("h"), Some(Language::C));
        assert_eq!(Language::from_extension("cpp"), Some(Language::Cpp));
        assert_eq!(Language::from_extension("hpp"), Some(Language::Cpp));
        assert_eq!(Language::from_extension("rb"), Some(Language::Ruby));
        assert_eq!(Language::from_extension("sh"), Some(Language::Bash));
        assert_eq!(Language::from_extension("bash"), Some(Language::Bash));
        assert_eq!(Language::from_extension("json"), Some(Language::Json));
    }

    #[test]
    fn from_extension_is_case_insensitive() {
        assert_eq!(Language::from_extension("RS"), Some(Language::Rust));
        assert_eq!(Language::from_extension("Py"), Some(Language::Python));
        assert_eq!(Language::from_extension("JSX"), Some(Language::JavaScript));
    }

    #[test]
    fn from_extension_returns_none_for_unknown() {
        assert_eq!(Language::from_extension(""), None);
        assert_eq!(Language::from_extension("txt"), None);
        assert_eq!(Language::from_extension("md"), None);
        assert_eq!(Language::from_extension("yaml"), None);
        assert_eq!(Language::from_extension("xyz"), None);
    }

    #[test]
    fn from_extension_does_not_panic_on_leading_dot() {
        // Helper does not strip the leading `.` — callers must.  This
        // test simply proves the call does not panic for that input and
        // returns None, so a future caller that forgets to strip sees a
        // graceful None instead of a crash.
        assert_eq!(Language::from_extension(".rs"), None);
    }
}
