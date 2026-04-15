# WO-0006 — Ready for Review

**Work-order**: WO-0006 `symbol-extraction-chunker-storage`
**Branch**: `feat/WO-0006-symbol-extraction-chunker-storage`
**Final commit**: `705e5d62882216cc4699c9da5bf85d3a1a4f5e14`
**Date**: 2026-04-15

## What I verified locally

- **AC1** `cargo nextest run -p ucil-treesitter symbols:: --test-threads 4`
  → **6/6 passed** (extract_rust_functions, extract_rust_struct, extract_python_class,
    extract_typescript_functions, extract_empty_source_returns_empty_vec,
    all_symbols_have_nonempty_names)

- **AC2** `cargo nextest run -p ucil-treesitter chunker:: --test-threads 4`
  → **4/4 passed** (chunk_rust_two_functions, chunk_oversized_function,
    chunk_empty_source, chunk_ids_match_expected_format)

- **AC3** `cargo nextest run -p ucil-daemon storage::tests::test_two_tier_layout --test-threads 4`
  → **1/1 passed**

- **AC4** `cargo clippy -p ucil-treesitter -- -D warnings`
  → **0 errors** (resolved: let..else rewrites, usize_to_u32 helper, is_some_and,
    const fn for lang_to_str/symbol_kind_to_str, chars_to_u32 helper)

- **AC5** `cargo clippy -p ucil-daemon -- -D warnings`
  → **0 errors**

- **AC6** `cargo build --workspace`
  → **Finished dev profile** with 0 errors

- **AC7** `grep -c 'SymbolKind::' crates/ucil-treesitter/src/symbols.rs`
  → **23** (≥ 6 required; covers Function, Class, Method, Struct, Enum, Trait,
    Const, TypeAlias, Module, Interface, Constructor, Field)

## Commits on this branch

```
705e5d6 refactor(treesitter): fix all clippy::pedantic warnings in symbols + chunker
aa8c4aa feat(daemon): add StorageLayout two-tier .ucil/ directory initialiser
3cedd46 feat(treesitter): add AST-aware Chunker producing ≤512-token Chunk values
711fb2c feat(treesitter): add SymbolExtractor, SymbolKind, ExtractedSymbol
```

## Files created / modified

- `crates/ucil-treesitter/src/symbols.rs` (NEW) — SymbolKind, ExtractedSymbol, SymbolExtractor + 6 unit tests
- `crates/ucil-treesitter/src/chunker.rs` (NEW) — Chunk, ChunkError, Chunker + 4 unit tests
- `crates/ucil-daemon/src/storage.rs` (NEW) — StorageLayout, StorageError + test_two_tier_layout
- `crates/ucil-treesitter/src/lib.rs` (modified) — pub mod symbols + chunker re-exports
- `crates/ucil-daemon/src/lib.rs` (modified) — pub mod storage re-export
- `crates/ucil-treesitter/Cargo.toml` (modified) — added streaming-iterator dep
- `Cargo.toml` (modified) — added streaming-iterator workspace dep
