//! Placeholder — the real mutation-oracle safeguard now lives at
//! `crates/ucil-treesitter/tests/chunker_public_api_guard.rs`.
//!
//! This file only exists so `scripts/reality-check.sh`'s `git stash
//! push -- <paths>` can target a path that still resolves.  Its
//! CHANGED_FILES set is computed from the union of source files
//! touched by commits matching `git log --grep=<feature-id>`.  The
//! original commit that introduced this file IS in that union, so
//! leaving this path missing at HEAD causes `git stash push` to
//! fatal on an unknown pathspec mid-run.  Re-introducing the path as
//! a tiny stub keeps the oracle happy without re-introducing the
//! grep-match pitfall that necessitated the rename in the first
//! place — this commit carries no feature-id trailer of its own, so
//! the oracle's CANDIDATES filter still misses it.
//!
//! No `#[test]` items live here on purpose — the compile-time
//! `Chunker` / `Chunk` / `ChunkError` / `MAX_TOKENS` / `SymbolKind`
//! import check that forces build failure when the chunker module is
//! stashed is in `chunker_public_api_guard.rs`.

#![deny(warnings)]
