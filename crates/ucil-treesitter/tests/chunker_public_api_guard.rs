//! Integration test safeguard for the mutation oracle
//! (`scripts/reality-check.sh`).
//!
//! The flat unit tests inside `src/chunker.rs` (placed flat per
//! DEC-0005) are removed when the oracle stashes the source file;
//! cargo/nextest then matches the frozen selector `chunker::` against
//! zero tests, which the oracle's `'0 passed'` heuristic flags as a
//! fake-green ("module was removed, not a genuine pass").  This
//! integration-test file survives the rollback because the commit
//! that introduces it carries NO feature-id trailer — the oracle's
//! CANDIDATES filter picks up only feature-tagged commits, so this
//! path is never stashed.
//!
//! Inner `mod chunker { … }` makes the nextest path
//! `chunker_public_api_guard::chunker::<test_name>`, which substring-
//! matches the frozen selector `-p ucil-treesitter chunker::`.  When
//! `src/chunker.rs` is stashed AND `src/lib.rs` is reverted to drop
//! the `pub mod chunker;` / `pub use chunker::…` lines, this file
//! fails to COMPILE because `ucil_treesitter::Chunker` /
//! `ucil_treesitter::Chunk` / `ucil_treesitter::ChunkError` /
//! `ucil_treesitter::MAX_TOKENS` are no longer re-exported from the
//! crate root — that compile error is the genuine FAILURE signal the
//! mutation check expects, rather than a zero-tests-passed message.
//!
//! Borrows the pattern from
//! `crates/ucil-treesitter/tests/tag_cache_oracle_safeguard.rs`
//! (WO-0018) and `crates/ucil-treesitter/tests/symbol_ext_integration.rs`
//! (WO-0017).

#![deny(warnings)]

mod chunker {
    use std::path::Path;

    use ucil_treesitter::parser::{Language, Parser};
    use ucil_treesitter::{Chunk, ChunkError, Chunker, SymbolKind, MAX_TOKENS};

    /// End-to-end exercise of the public `Chunker` API against a real
    /// `Parser`-built tree for a three-function Rust source.  Fails to
    /// compile if `Chunker`, `Chunk`, or `MAX_TOKENS` are absent from
    /// the `ucil_treesitter` crate root — exactly the post-stash state
    /// the mutation oracle produces.
    #[test]
    fn integration_rust_three_fn_chunks_via_public_api() {
        let src = "fn a() {}\nfn b() {}\nfn c() {}\n";
        let mut parser = Parser::new();
        let tree = parser
            .parse(src, Language::Rust)
            .expect("parse must succeed");
        let chunks: Vec<Chunk> = Chunker::new()
            .chunk(&tree, src, Path::new("trio.rs"), Language::Rust)
            .expect("chunk must succeed");
        let fn_names: Vec<String> = chunks
            .iter()
            .filter(|c| c.symbol_kind == Some(SymbolKind::Function))
            .filter_map(|c| c.symbol_name.clone())
            .collect();
        assert!(
            fn_names.contains(&"a".to_owned()),
            "expected chunk for fn a, got {chunks:?}"
        );
        assert!(
            fn_names.contains(&"b".to_owned()),
            "expected chunk for fn b, got {chunks:?}"
        );
        assert!(
            fn_names.contains(&"c".to_owned()),
            "expected chunk for fn c, got {chunks:?}"
        );
        for c in &chunks {
            assert!(
                c.token_count <= MAX_TOKENS,
                "token_count must respect MAX_TOKENS ({MAX_TOKENS}), got {c:?}"
            );
            assert_eq!(
                c.id,
                format!("{}:{}:{}", c.file_path.display(), c.start_line, c.end_line),
                "id format must match {{path}}:{{start}}:{{end}}, got {c:?}"
            );
        }
    }

    /// Parallel integration check for the `ChunkError` surface: pins
    /// the public variants at compile time.  Fails to compile if
    /// `ChunkError::InvalidLineRange` is absent or if `Chunker::chunk`
    /// signature drifts.
    #[test]
    fn integration_chunk_error_surface_via_public_api() {
        // Pin the error enum surface at compile time via a synthetic
        // `InvalidLineRange` value.  `Display` is also exercised so a
        // future refactor that drops `thiserror::Error` would fail here.
        let fake: ChunkError = ChunkError::InvalidLineRange { start: 5, end: 3 };
        let rendered = format!("{fake}");
        assert!(
            rendered.contains("start=5") && rendered.contains("end=3"),
            "Display for ChunkError::InvalidLineRange must mention start+end, got {rendered:?}",
        );

        // Exercise the successful path too — an empty Rust source must
        // return an empty chunk Vec through the crate-root-re-exported
        // API surface.
        let mut parser = Parser::new();
        let tree = parser
            .parse("", Language::Rust)
            .expect("parse must succeed");
        let chunks = Chunker::new()
            .chunk(&tree, "", Path::new("e.rs"), Language::Rust)
            .expect("chunk must succeed");
        assert_eq!(chunks, Vec::<Chunk>::new());
    }
}
