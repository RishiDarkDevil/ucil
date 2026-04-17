//! Placeholder file at the path where the tag-cache mutation-oracle
//! safeguard used to live before it was renamed (see commit `af49f64`
//! moving the real safeguard to
//! `tests/tag_cache_oracle_safeguard.rs`).
//!
//! This file exists ONLY so that `scripts/reality-check.sh`'s
//! `git stash push -u -- <CHANGED_FILES>` command has a path that
//! resolves.  The oracle's union of candidate-touched source paths
//! still contains the old path because the introducing commit
//! referenced it; without this placeholder the stash push fatals
//! with `pathspec … did not match any files` and the oracle halts
//! before the substantive stashed/restored phases can run.
//!
//! The placeholder carries no `#[test]` functions — an empty test
//! crate — so it contributes zero rows to the nextest selector match
//! count and cannot trip the zero-tests-ran fake-green heuristic.
//! Its post-rollback state under the oracle is "deleted" (parent of
//! `e4d9537` contains no file here).
//!
//! Borrows the pattern verbatim from
//! `crates/ucil-treesitter/tests/symbols.rs` (WO-0017 commit
//! `7d7314d`) which was introduced for the same purpose.

#![deny(warnings)]
