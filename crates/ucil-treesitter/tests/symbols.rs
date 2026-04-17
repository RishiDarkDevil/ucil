//! Integration tests for `ucil_treesitter::symbols` — safeguards the
//! reality-check mutation oracle against the "tests-in-same-source-file"
//! edge case.
//!
//! The unit tests that live flat inside `src/symbols.rs` (per DEC-0005)
//! are removed when `scripts/reality-check.sh` stashes the source file;
//! `cargo nextest run … symbols::` then matches the selector against
//! zero tests, which the script's `'0 passed'` heuristic flags as a
//! fake-green.  This integration test file survives the stash because
//! its introducing commit does NOT carry a `Feature: P1-W2-F02` trailer
//! — reality-check only stashes files from Feature-tagged commits (the
//! `git log --grep="Feature: $FEATURE_ID"` CANDIDATES set).
//!
//! When `src/symbols.rs` is stashed and `src/lib.rs` is reverted (losing
//! the `pub mod symbols;` and `pub use symbols::{…}` lines), this file
//! fails to COMPILE because `ucil_treesitter::SymbolExtractor` is no
//! longer exported from the crate root — that compile error is the
//! genuine FAILURE signal the mutation check expects, rather than a
//! "0 passed" message.
//!
//! The pattern is borrowed from `crates/ucil-cli/tests/init.rs`, which
//! was introduced for the same purpose in WO-0004 (F04/F05/F06) —
//! commit `bd68c88`.

#![deny(warnings)]

mod symbols {
    use std::path::Path;

    use ucil_treesitter::{ExtractedSymbol, Language, Parser, SymbolExtractor, SymbolKind};

    /// End-to-end exercise of the public `SymbolExtractor` API against
    /// real `tree-sitter` parsing of a minimal Rust source.  Fails to
    /// compile if `SymbolExtractor`, `SymbolKind`, or the `extract`
    /// method signature are absent from the `ucil_treesitter` crate
    /// root — which is the situation `reality-check.sh` creates when it
    /// rolls `lib.rs` back to the pre-symbols state.
    #[test]
    fn symbols_integration_extracts_rust_function_via_public_api() {
        let mut parser = Parser::new();
        let src = "fn hello() {}";
        let tree = parser
            .parse(src, Language::Rust)
            .expect("parse must succeed on valid Rust");
        let extractor = SymbolExtractor::new();
        let syms: Vec<ExtractedSymbol> =
            extractor.extract(&tree, src, Path::new("hello.rs"), Language::Rust);
        assert_eq!(syms.len(), 1, "expected one symbol, got {syms:?}");
        assert_eq!(syms[0].name, "hello");
        assert_eq!(syms[0].kind, SymbolKind::Function);
        assert_eq!(syms[0].language, Language::Rust);
    }

    /// Parallel integration check for Python to cover a second primary
    /// language path; also exercises `SymbolKind::Class` and confirms
    /// the `file_path` field is copied verbatim into every returned
    /// symbol.
    #[test]
    fn symbols_integration_extracts_python_class_via_public_api() {
        let mut parser = Parser::new();
        let src = "class Foo:\n    pass\n";
        let tree = parser
            .parse(src, Language::Python)
            .expect("parse must succeed on valid Python");
        let extractor = SymbolExtractor::new();
        let syms: Vec<ExtractedSymbol> =
            extractor.extract(&tree, src, Path::new("foo.py"), Language::Python);
        let has_class_foo = syms
            .iter()
            .any(|s| s.name == "Foo" && s.kind == SymbolKind::Class);
        assert!(has_class_foo, "expected Class(Foo), got {syms:?}");
        for s in &syms {
            assert_eq!(s.language, Language::Python);
            assert_eq!(s.file_path, Path::new("foo.py"));
        }
    }
}
