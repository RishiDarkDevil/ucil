#![deny(warnings)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
// Each frozen test below carries an explicit Panics-section narrative
// in its rustdoc plus the standard `(SAn) ...` panic-message body
// convention from `DEC-0007`; suppressing the auto-emitted Panics-
// section requirement here matches the WO-0094 / WO-0070 / WO-0085 /
// WO-0089 / WO-0090 / WO-0093 frozen-test precedent.
#![allow(clippy::missing_panics_doc, clippy::too_long_first_doc_paragraph)]
//! `P3-W9-F11` — Salsa engine early-cutoff integration test binary.
//!
//! Master-plan §10 (daemon architecture — Salsa-backed incremental
//! computation), §17.2 line 1693 (`tests/integration/test_incremental.rs`
//! placement), §18 Phase 3 Week 9 (incremental computation integration
//! test suite). Implements the public-surface counterpart of the
//! `crates/ucil-core/src/incremental.rs::tests::early_cutoff_skips_downstream_recompute`
//! unit test — same invariant, but exercised through the public
//! `ucil_core::incremental` API (`UcilDatabase`, `FileRevision`,
//! `symbol_count`, `dependent_metric`) using a locally-defined
//! `LoggingDatabase` event-hook harness (the unit-test version is
//! `#[cfg(test)]`-private to ucil-core).
//!
//! The local `LoggingDatabase` is the trait-implementation seam
//! through which the test observes Salsa's `WillExecute` events —
//! it is the standard Salsa observability harness, not a substitute
//! for any external dep. Production impls (and the production
//! `UcilDatabase` itself) live in `crates/ucil-core/src/incremental.rs`.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use salsa::Setter;
use ucil_core::incremental::{dependent_metric, FileRevision};

/// Test-side instrumented Salsa database whose event hook records
/// every `WillExecute` event so the integration tests can assert
/// exactly which tracked functions re-executed across a revision
/// boundary. Mirrors `crates/ucil-core/src/incremental.rs::tests::LoggingDatabase`
/// because that type is `#[cfg(test)]`-private to ucil-core and not
/// reachable from this integration test crate.
#[salsa::db]
#[derive(Clone)]
struct LoggingDatabase {
    storage: salsa::Storage<Self>,
    execute_log: Arc<Mutex<Vec<String>>>,
}

impl Default for LoggingDatabase {
    fn default() -> Self {
        let execute_log: Arc<Mutex<Vec<String>>> = Arc::default();
        let log_clone = Arc::clone(&execute_log);
        Self {
            storage: salsa::Storage::new(Some(Box::new(move |event| {
                if let salsa::EventKind::WillExecute { database_key } = event.kind {
                    log_clone
                        .lock()
                        .expect("log mutex poisoned")
                        .push(format!("{database_key:?}"));
                }
            }))),
            execute_log,
        }
    }
}

#[salsa::db]
impl salsa::Database for LoggingDatabase {}

impl LoggingDatabase {
    fn drain_log(&self) -> Vec<String> {
        std::mem::take(&mut *self.execute_log.lock().expect("log mutex poisoned"))
    }
}

fn fixture_revision<DB: salsa::Database>(db: &DB, contents: &str) -> FileRevision {
    FileRevision::new(db, PathBuf::from("lib.rs"), 0, contents.to_owned())
}

/// Frozen acceptance test for `P3-W9-F11` — Salsa early-cutoff
/// invariant: when `mtime_nanos` is bumped (forcing Salsa to
/// re-execute `symbol_count`) but `symbol_count`'s return value
/// is unchanged (because the new contents differ ONLY in
/// whitespace, which `split_ascii_whitespace().count()` ignores),
/// `dependent_metric` MUST NOT re-execute.
///
/// # Panics
/// Panics with `(SAn) <semantic name>; left: ..., right: ...`
/// per DEC-0007 / WO-0094 precedent if any of the four assertions
/// fail. SA1 is the load-bearing early-cutoff assertion; SA0/SA2/SA3
/// are sanity canaries.
#[test]
pub fn test_incremental_whitespace_only_change_skips_downstream_recompute() {
    let mut db = LoggingDatabase::default();
    // Seed with `"alpha beta gamma"` — three whitespace-separated
    // tokens → symbol_count = 3, dependent_metric = 6.
    let rev = fixture_revision(&db, "alpha beta gamma");

    let initial_metric = dependent_metric(&db, rev);
    assert_eq!(
        initial_metric, 6,
        "(SA0) initial dependent_metric should equal 6 (3 symbols * 2); left: {initial_metric}, right: 6"
    );
    let _ = db.drain_log();

    // Whitespace-only change: bump mtime AND replace contents with
    // an extra-whitespace variant of the same three tokens.
    // `split_ascii_whitespace()` is whitespace-class agnostic so
    // the token COUNT (3) is preserved → symbol_count's return
    // value (3) is unchanged → early cutoff fires.
    rev.set_mtime_nanos(&mut db).to(42);
    rev.set_contents(&mut db)
        .to("alpha   beta\tgamma".to_owned());

    let after_metric = dependent_metric(&db, rev);
    let after_log = db.drain_log();

    let symbol_reran = after_log.iter().any(|e| e.contains("symbol_count"));
    let dependent_reran = after_log.iter().any(|e| e.contains("dependent_metric"));

    assert_eq!(
        after_metric, 6,
        "(SA2) dependent_metric value stable across whitespace-only change; left: {after_metric}, right: 6"
    );
    assert!(
        symbol_reran,
        "(SA3) symbol_count must re-execute after mtime+contents bump (revision invalidated); log: {after_log:?}"
    );
    assert!(
        !dependent_reran,
        "(SA1) dependent_metric must NOT re-execute when symbol_count's return value is unchanged — this is Salsa early cutoff; log: {after_log:?}"
    );
}

/// Frozen control test for `P3-W9-F11` — invalidation propagation
/// invariant: when `symbol_count`'s return value DOES change
/// (because the new contents introduce additional tokens),
/// `dependent_metric` MUST re-execute. This is the inverse of
/// `test_incremental_whitespace_only_change_skips_downstream_recompute`
/// and exists so the early-cutoff test cannot pass trivially when
/// `dependent_metric` is broken to never re-execute.
///
/// # Panics
/// Panics with `(SAn) ...` per DEC-0007 if any assertion fails.
#[test]
pub fn test_incremental_semantic_change_invalidates_downstream() {
    let mut db = LoggingDatabase::default();
    let rev = fixture_revision(&db, "alpha beta");

    let initial_metric = dependent_metric(&db, rev);
    assert_eq!(
        initial_metric, 4,
        "(SA0) initial dependent_metric should equal 4 (2 symbols * 2); left: {initial_metric}, right: 4"
    );
    let _ = db.drain_log();

    // Semantic change: add two new tokens. symbol_count's return
    // value bumps from 2 to 4; dependent_metric MUST recompute and
    // return 8 (4 * 2).
    rev.set_contents(&mut db)
        .to("alpha beta gamma delta".to_owned());

    let after_metric = dependent_metric(&db, rev);
    let after_log = db.drain_log();

    assert_eq!(
        after_metric, 8,
        "(SA1) dependent_metric must reflect the new symbol_count after a semantic change; left: {after_metric}, right: 8"
    );
    assert!(
        after_log.iter().any(|e| e.contains("symbol_count")),
        "(SA2) symbol_count must re-execute after contents change (revision invalidated); log: {after_log:?}"
    );
    assert!(
        after_log.iter().any(|e| e.contains("dependent_metric")),
        "(SA3) dependent_metric must re-execute when symbol_count's return value changes; log: {after_log:?}"
    );
}
