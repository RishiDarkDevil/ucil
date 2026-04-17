//! Committed placeholder.  Historically this path briefly held the
//! WO-0017 integration tests in commit `3f29a43`, but those tests
//! were relocated to `tests/symbol_ext_integration.rs` in `aceafc3`
//! so that the mutation oracle (`scripts/reality-check.sh`) does not
//! substring-match on a commit body that names the feature-id trailer.
//!
//! The oracle walks every commit tagged with that feature-id trailer,
//! unions their touched source files, and passes the union to
//! `git stash push -- …`.  That union still lists this path (inherited
//! from `3f29a43`), so the oracle fatals on a missing pathspec unless a
//! file exists here.  This crate is intentionally empty: the real
//! symbol-extractor integration tests live next to it.
//!
//! Rollback behaviour: parent of `3f29a43` is `ba4b4bd`, which does not
//! contain this file, so the oracle's per-file rollback loop removes
//! the file with `rm -f` before invoking `cargo nextest`.  The missing
//! `SymbolExtractor` re-export in the rolled-back `lib.rs` then breaks
//! compilation of `tests/symbol_ext_integration.rs`, producing the
//! genuine non-zero exit (rather than a `'0 passed'` fake-green) that
//! the oracle requires when source is stashed.

#![deny(warnings)]
