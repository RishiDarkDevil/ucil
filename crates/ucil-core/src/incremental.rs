//! Skeleton incremental-computation engine backed by the `salsa` 2022 crate.
//!
//! This module introduces the minimal Salsa-based dependency DAG required
//! by master-plan §10 (daemon architecture).  It wires one tracked input
//! ([`FileRevision`]) to two tracked query functions ([`symbol_count()`] and
//! [`dependent_metric()`]) so the compiler, rustdoc, and the unit-test suite
//! can all verify the three invariants the Week-3 feature card demands:
//!
//! 1. **Memoisation** — two reads of the same input do not re-execute the
//!    tracked function.
//! 2. **Invalidation** — mutating a salsa input (e.g. bumping a file's
//!    recorded mtime) forces the dependent tracked function to recompute.
//! 3. **Early cutoff** — if a tracked function's *return value* is
//!    unchanged after a forced recompute, second-order tracked functions
//!    that depend only on the return value are *not* re-executed.
//!
//! The engine is deliberately narrow: it ships no file watching, no cache
//! persistence, and no plugin wiring.  Those land in later work-orders
//! (P1-W3-F02, P1-W3-F06, P1-W3-F07).  This module is the oracle the
//! rest of the daemon will build on top of.
//!
//! # Example
//!
//! ```no_run
//! use std::path::PathBuf;
//! use ucil_core::incremental::{FileRevision, UcilDatabase, symbol_count};
//!
//! let db = UcilDatabase::default();
//! let rev = FileRevision::new(&db, PathBuf::from("lib.rs"), 0, "fn a() {}".to_owned());
//! assert_eq!(symbol_count(&db, rev), 2);
//! ```

// The tracked-function macro generates code that triggers
// `clippy::used_underscore_binding` on the db parameter; the macro is
// opaque to us so we silence the specific lint here rather than shipping
// a pervasive `#[allow]` at crate root.
#![allow(clippy::used_underscore_binding)]

use std::path::PathBuf;

use salsa::Storage;

// ── Inputs ────────────────────────────────────────────────────────────────────

/// A single logical revision of a source file observed by the daemon.
///
/// `FileRevision` is a `#[salsa::input]` — it participates in the salsa
/// dependency graph so that setting any of its fields (via the generated
/// `set_*` methods) bumps the database's revision counter and invalidates
/// every tracked function that previously read the field.
///
/// The `mtime_nanos` field is the invalidation token: tests and the live
/// file-watcher layer both use it to signal "this file has changed" even
/// when the textual content happens to be identical (which is how salsa
/// exercises its *early-cutoff* optimisation).
#[salsa::input]
pub struct FileRevision {
    /// Path to the file.  Stored for diagnostic purposes only; the tracked
    /// functions here do not branch on it.
    pub path: PathBuf,
    /// A monotonically-increasing timestamp token.  Nanoseconds are used
    /// because [`std::time::SystemTime`] is awkward to inject in tests and
    /// the precise clock domain does not matter for the DAG — only the
    /// fact that `set_mtime_nanos` bumps the revision counter does.
    pub mtime_nanos: u64,
    /// The file's textual contents.  Returned by reference to avoid the
    /// cost of cloning on every memoised read.
    #[returns(ref)]
    pub contents: String,
}

// ── Database ─────────────────────────────────────────────────────────────────

/// Marker trait for any database that exposes the [`FileRevision`] input
/// and the tracked queries in this module.
///
/// The trait is empty: every concrete database that implements
/// [`salsa::Database`] gets a blanket `UcilDb` impl below.  The name
/// exists so downstream modules can take `&dyn UcilDb` rather than a
/// concrete database type, which keeps their compile surface small.
#[salsa::db]
pub trait UcilDb: salsa::Database {}

/// The concrete Salsa database used by the skeleton engine.
///
/// Owns the Salsa [`Storage`] — no other state is currently required.
/// Cheap to clone; each clone is an independent handle onto the same
/// underlying storage and can issue reads in parallel.
#[salsa::db]
#[derive(Clone, Default)]
pub struct UcilDatabase {
    storage: Storage<Self>,
}

#[salsa::db]
impl salsa::Database for UcilDatabase {}

#[salsa::db]
impl<Db: salsa::Database> UcilDb for Db {}

// ── Tracked queries ──────────────────────────────────────────────────────────

/// Count the whitespace-separated *symbols* in `rev.contents`.
///
/// This is a deliberately tiny pure function — it stands in for the
/// real symbol extractor implemented in `ucil-treesitter`.  What matters
/// for this skeleton is the shape of the query:
///
/// * It reads two fields off the input (`mtime_nanos` and `contents`),
///   so any mutation to either field invalidates the memoised result.
/// * Its return value is a `u32`, which salsa compares with `PartialEq`
///   to drive the early-cutoff optimisation used by [`dependent_metric`].
#[salsa::tracked]
pub fn symbol_count(db: &dyn UcilDb, rev: FileRevision) -> u32 {
    // Touch the mtime token so salsa records a dependency on it; this is
    // what lets the "change mtime" test case trigger a re-execution even
    // when the contents are byte-identical.
    let _revision_token = rev.mtime_nanos(db);
    let contents = rev.contents(db);
    u32::try_from(contents.split_ascii_whitespace().count()).unwrap_or(u32::MAX)
}

/// Second-order tracked function whose output depends *only* on the
/// return value of [`symbol_count`].
///
/// The body doubles the symbol count — a stand-in for downstream
/// structural metrics (e.g. "how many function bodies should we chunk").
/// The body is deliberately deterministic so the unit test at
/// [`tests::early_cutoff_skips_downstream_recompute`] can observe the
/// early-cutoff behaviour: when `symbol_count` is forced to re-execute
/// but returns an unchanged value, `dependent_metric` must **not**
/// re-execute.
#[salsa::tracked]
pub fn dependent_metric(db: &dyn UcilDb, rev: FileRevision) -> u32 {
    symbol_count(db, rev).saturating_mul(2)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        sync::{Arc, Mutex},
    };

    use salsa::Setter;

    use super::{dependent_metric, symbol_count, FileRevision, UcilDatabase};

    /// Instrumented database variant whose event hook records every
    /// `WillExecute` event so tests can assert exactly which tracked
    /// functions re-executed across a revision boundary.
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

    #[test]
    fn memoise_skips_second_call_with_identical_input() {
        let db = LoggingDatabase::default();
        let rev = fixture_revision(&db, "alpha beta gamma");

        assert_eq!(symbol_count(&db, rev), 3);
        let first = db.drain_log();
        assert!(
            first.iter().any(|entry| entry.contains("symbol_count")),
            "first call should have executed symbol_count (log: {first:?})"
        );

        assert_eq!(symbol_count(&db, rev), 3);
        let second = db.drain_log();
        assert!(
            second.iter().all(|entry| !entry.contains("symbol_count")),
            "second identical call must be served from the memo (log: {second:?})"
        );
    }

    #[test]
    fn invalidate_on_mtime_change_forces_recompute() {
        let mut db = LoggingDatabase::default();
        let rev = fixture_revision(&db, "alpha beta gamma delta");

        assert_eq!(symbol_count(&db, rev), 4);
        let _ = db.drain_log();

        // Bump the mtime token WITHOUT changing the contents.  salsa must
        // treat this as an input change and force symbol_count to re-run.
        rev.set_mtime_nanos(&mut db).to(1);

        assert_eq!(symbol_count(&db, rev), 4);
        let after = db.drain_log();
        assert!(
            after.iter().any(|entry| entry.contains("symbol_count")),
            "mtime change must invalidate the tracked fn and trigger re-execution (log: {after:?})"
        );
    }

    #[test]
    fn early_cutoff_skips_downstream_recompute() {
        let mut db = LoggingDatabase::default();
        let rev = fixture_revision(&db, "alpha beta gamma");

        assert_eq!(dependent_metric(&db, rev), 6);
        let _ = db.drain_log();

        // Bump mtime; contents unchanged.  symbol_count will re-execute
        // but must still return 3.  That means `dependent_metric`'s own
        // cached value (6) remains valid — salsa must detect the stable
        // return and skip re-executing dependent_metric (early cutoff).
        rev.set_mtime_nanos(&mut db).to(42);

        assert_eq!(dependent_metric(&db, rev), 6);
        let after = db.drain_log();

        let symbol_reran = after.iter().any(|e| e.contains("symbol_count"));
        let dependent_reran = after.iter().any(|e| e.contains("dependent_metric"));

        assert!(
            symbol_reran,
            "symbol_count should have re-executed after mtime bump (log: {after:?})"
        );
        assert!(
            !dependent_reran,
            "dependent_metric must NOT re-execute when its input value is unchanged \
             — this is early cutoff (log: {after:?})"
        );
    }

    #[test]
    fn contents_change_propagates_through_downstream() {
        let mut db = LoggingDatabase::default();
        let rev = fixture_revision(&db, "alpha beta");

        assert_eq!(dependent_metric(&db, rev), 4);
        let _ = db.drain_log();

        rev.set_contents(&mut db)
            .to("alpha beta gamma delta".to_owned());

        assert_eq!(dependent_metric(&db, rev), 8);
        let after = db.drain_log();
        assert!(
            after.iter().any(|e| e.contains("symbol_count")),
            "symbol_count must re-execute after contents change (log: {after:?})"
        );
        assert!(
            after.iter().any(|e| e.contains("dependent_metric")),
            "dependent_metric must re-execute when symbol_count's return value changes \
             (log: {after:?})"
        );
    }

    #[test]
    fn concrete_database_default_constructs() {
        // Guarantees that the production `UcilDatabase` (not the
        // test-only LoggingDatabase) is usable out of the box and that
        // the tracked functions compile against it — otherwise the
        // rustdoc example in the module header would not compile either.
        let db = UcilDatabase::default();
        let rev = fixture_revision(&db, "x y z");
        assert_eq!(symbol_count(&db, rev), 3);
        assert_eq!(dependent_metric(&db, rev), 6);
    }
}
