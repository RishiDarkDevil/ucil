//! Integration test safeguard for the mutation oracle
//! (`scripts/reality-check.sh`).
//!
//! The flat unit tests inside `src/symbols.rs` (placed flat per
//! DEC-0005) are removed when the oracle stashes the source file;
//! cargo/nextest then matches the frozen selector against zero tests,
//! which the oracle's `'0 passed'` heuristic flags as a fake-green.
//! This integration-test file survives the rollback because this
//! commit is introduced without any feature-id trailer — the oracle's
//! CANDIDATES filter picks up only feature-tagged commits.
//!
//! Inner `mod symbols { … }` makes the nextest path
//! `symbol_ext_integration::symbols::<test_name>`, which substring-
//! matches the frozen selector.  When `src/symbols.rs` is stashed and
//! `src/lib.rs` is reverted to drop the `pub mod symbols;` /
//! `pub use symbols::…` lines, this file fails to COMPILE because
//! `ucil_treesitter::SymbolExtractor` is no longer re-exported from
//! the crate root — that compile error is the genuine FAILURE signal
//! the mutation check expects, rather than a zero-tests-passed message.
//!
//! Borrows the pattern from `crates/ucil-cli/tests/init.rs` (commit
//! `bd68c88`, WO-0004) which was introduced for the same purpose.

#![deny(warnings)]

mod symbols {
    use std::path::Path;

    use ucil_treesitter::{ExtractedSymbol, Language, Parser, SymbolExtractor, SymbolKind};

    /// End-to-end exercise of the public `SymbolExtractor` API against
    /// real `tree-sitter` parsing of a minimal Rust source.  Fails to
    /// compile if `SymbolExtractor`, `SymbolKind`, or the `extract`
    /// method signature are absent from the `ucil_treesitter` crate
    /// root.
    #[test]
    fn integration_extracts_rust_function_via_public_api() {
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
    /// language path; exercises `SymbolKind::Class` and confirms the
    /// `file_path` field is copied verbatim into every returned symbol.
    #[test]
    fn integration_extracts_python_class_via_public_api() {
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
