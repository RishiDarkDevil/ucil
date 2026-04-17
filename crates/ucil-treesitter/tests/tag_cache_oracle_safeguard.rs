//! Integration test safeguard for the mutation oracle
//! (`scripts/reality-check.sh`).
//!
//! The flat unit tests inside `src/tag_cache.rs` (placed flat per
//! DEC-0005) are removed when the oracle stashes the source file;
//! cargo/nextest then matches the frozen selector against zero tests,
//! which the oracle's `'0 passed'` heuristic flags as a fake-green.
//! This integration-test file survives the rollback because this
//! commit is introduced without any feature-id trailer — the oracle's
//! CANDIDATES filter picks up only feature-tagged commits.
//!
//! Inner `mod tag_cache { … }` makes the nextest path
//! `tag_cache_integration::tag_cache::<test_name>`, which substring-
//! matches the frozen selector `-p ucil-treesitter tag_cache::`.  When
//! `src/tag_cache.rs` is stashed and `src/lib.rs` is reverted to drop
//! the `pub mod tag_cache;` / `pub use tag_cache::…` lines, this file
//! fails to COMPILE because `ucil_treesitter::TagCache` is no longer
//! re-exported from the crate root — that compile error is the genuine
//! FAILURE signal the mutation check expects, rather than a
//! zero-tests-passed message.
//!
//! Borrows the pattern from `crates/ucil-treesitter/tests/symbol_ext_integration.rs`
//! (WO-0017) which was introduced for the same purpose, itself a
//! derivative of `crates/ucil-cli/tests/init.rs` (WO-0004).

#![deny(warnings)]

mod tag_cache {
    use std::path::Path;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use tempfile::TempDir;
    use ucil_treesitter::symbols::ExtractedSymbol;
    use ucil_treesitter::{Language, SymbolKind, TagCache, TagCacheError};

    /// End-to-end exercise of the public `TagCache` API: opens a cache
    /// in a real `tempfile::TempDir`-backed `heed::Env`, writes one
    /// symbol vec, reads it back, and asserts identity round-trip.
    /// Fails to compile if `TagCache`, its `open`/`put`/`get` methods,
    /// or `TagCacheError` are absent from the `ucil_treesitter` crate
    /// root — exactly the post-stash state the mutation oracle
    /// produces.
    #[test]
    fn integration_put_then_get_via_public_api() {
        let dir = TempDir::new().expect("tempdir must open");
        let cache: TagCache = TagCache::open(dir.path()).expect("tag cache must open");
        let path = Path::new("hello.rs");
        let mtime = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let sym = ExtractedSymbol {
            name: "hello".to_string(),
            kind: SymbolKind::Function,
            language: Language::Rust,
            file_path: path.to_path_buf(),
            start_line: 1,
            start_col: 1,
            end_line: 1,
            end_col: 13,
            signature: None,
            doc_comment: None,
        };
        let symbols = vec![sym.clone()];
        cache
            .put(path, mtime, &symbols)
            .expect("put must succeed on open cache");
        let got: Option<Vec<ExtractedSymbol>> = cache
            .get(path, mtime)
            .expect("get must succeed on open cache");
        let got = got.expect("entry must be present after put");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "hello");
        assert_eq!(got[0].kind, SymbolKind::Function);
    }

    /// Parallel integration check for the `TagCacheError` surface:
    /// constructs a `TagCache`, asks for a missing key, confirms the
    /// `Ok(None)` arm, and pattern-matches a synthetic `TagCacheError`
    /// so the compile-time check also binds the error enum's public
    /// variants.  Fails to compile if `TagCacheError::Io` is absent or
    /// if `open`/`get` signatures drift.
    #[test]
    fn integration_missing_key_returns_none_via_public_api() {
        let dir = TempDir::new().expect("tempdir must open");
        let cache = TagCache::open(dir.path()).expect("tag cache must open");
        let got = cache
            .get(Path::new("no-such.rs"), SystemTime::UNIX_EPOCH)
            .expect("get must succeed on open cache");
        assert!(got.is_none(), "missing key must return None, got {got:?}");

        // Pin the error enum surface at compile time.
        let fake: TagCacheError = TagCacheError::Io(std::io::Error::other("probe"));
        let rendered = format!("{fake}");
        assert!(rendered.contains("probe"));
    }
}
