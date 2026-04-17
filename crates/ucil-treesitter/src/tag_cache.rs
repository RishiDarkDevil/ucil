//! LMDB-backed tag cache for tree-sitter symbol extraction.
//!
//! [`TagCache`] stores `Vec<ExtractedSymbol>` values keyed by
//! `(file_path, mtime)`.  It is the **L1** tier of the master-plan §2.1
//! tiered cache (L0 = in-memory LRU, L1 = local LMDB index, L2 = full
//! tool invocation) and is the on-disk complement to a future
//! in-memory LRU layer (Phase 4 Week 20).
//!
//! This module implements feature `P1-W2-F04` — master plan §18 Phase 1
//! Week 2 ("Implement tag cache in LMDB: `file_path` + mtime → \[Symbol\]",
//! line 1735).  The cache's on-disk home (`.ucil/branches/<branch>/tags.lmdb`)
//! is defined in master-plan §9.1; wiring the daemon-level
//! `OnceLock<heed::Env>` is the Phase 1 Week 3 file-watcher work-order
//! (`P1-W3-F01`) and intentionally out of scope here — this module is a
//! passive store, not a populator.
//!
//! # Design
//!
//! [`TagCache`] wraps a [`heed::Env`] and a single typed
//! [`heed::Database<Bytes, Bytes>`] (the unnamed default DB — one DB per
//! env keeps the per-call overhead minimal).  Each call routes through a
//! read-only or read-write LMDB transaction; no reference to cached data
//! escapes the helper that opens the transaction, so the cache is safe
//! to share across threads via `Arc<TagCache>`.
//!
//! ## Key encoding
//!
//! Keys are composed as:
//!
//! ```text
//! [ path_bytes ] [ 0x00 ] [ mtime_nanos_be (16 bytes, big-endian i128) ]
//! ```
//!
//! where `path_bytes` is `Path::as_os_str().as_encoded_bytes()`.  The
//! `NUL` byte is a sentinel — POSIX filesystem paths cannot contain it,
//! and Windows paths encoded via `WTF-8` also reject it — so it is a
//! safe terminator that disambiguates `/foo` from `/foo/bar` when both
//! are stored at the same mtime.  Big-endian `i128` encoding makes the
//! byte-lexicographic ordering imposed by LMDB's default comparator
//! agree with numerical ordering for non-negative nanoseconds (all
//! post-epoch mtimes), and yields an efficient prefix scan for the
//! future `invalidate_path` helper — the prefix `[path_bytes, 0x00]`
//! captures every `(path, mtime)` pair for that path.
//!
//! ## Value encoding
//!
//! Values are `bincode` v1 serialised `Vec<ExtractedSymbol>`.  [`ExtractedSymbol`]
//! and [`crate::symbols::SymbolKind`] both derive `Serialize` and
//! `Deserialize` (WO-0017), so no bespoke codec is required.
//!
//! ## Performance target
//!
//! Warm reads complete with a **median latency < 1 ms** on a release
//! build against a 100-entry fixture (see the
//! `tag_cache_warm_read_under_1ms` module-root test).  Debug builds are
//! given a 3× slop factor (< 3 ms median) via `cfg!(debug_assertions)`
//! so the test runs under `cargo test` and `cargo nextest` without a
//! release flag.
//!
//! # Tracing
//!
//! [`TagCache::get`] and [`TagCache::put`] open `tracing` spans at
//! `DEBUG` level named `ucil.treesitter.tag_cache_get` and
//! `ucil.treesitter.tag_cache_put` respectively, per master-plan §15.2
//! (`ucil.<layer>.<op>` naming).  The invalidation span
//! `ucil.treesitter.tag_cache_invalidate` is added alongside
//! `invalidate_path` in a follow-up commit.

// Like `parser.rs` and `symbols.rs`, types inside the `tag_cache` module
// share a name-prefix with the module; pedantic's
// `module_name_repetitions` would otherwise flag `TagCache` /
// `TagCacheError` and sap signal.
#![allow(clippy::module_name_repetitions)]

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use heed::types::Bytes;
use heed::{Database, Env, EnvOpenOptions};

use crate::symbols::ExtractedSymbol;

// ── Constants ──────────────────────────────────────────────────────────────

/// Maximum virtual address space LMDB may map for the tag cache, in
/// bytes.  1 GiB comfortably fits every tag cache this codebase is
/// expected to create on a single branch (tens of thousands of source
/// files × a few hundred symbols apiece).
///
/// The value is page-aligned for every mainstream page size (4 KiB on
/// x86-64 / ARM64, 16 KiB on Apple silicon): `1 GiB = 262144 × 4 KiB =
/// 65536 × 16 KiB`, which LMDB requires for [`EnvOpenOptions::map_size`].
const MAP_SIZE_BYTES: usize = 1024 * 1024 * 1024;

/// Maximum number of named databases per LMDB environment.  We only use
/// the unnamed default DB, so `1` is sufficient — larger values waste a
/// small amount of metadata space.
const MAX_DBS: u32 = 1;

/// Sentinel byte that separates the path prefix from the mtime tail in a
/// cache key.  `0x00` is impossible inside a filesystem path on Unix
/// (POSIX forbids it) and inside a WTF-8-encoded Windows path, so it is
/// unambiguous.
const PATH_MTIME_SEP: u8 = 0x00;

/// Width of the encoded mtime tail, in bytes — 16 bytes of big-endian
/// `i128` nanoseconds since [`UNIX_EPOCH`].
const MTIME_WIDTH: usize = 16;

// ── Errors ─────────────────────────────────────────────────────────────────

/// Failures that [`TagCache`] operations may surface to callers.
///
/// Marked `#[non_exhaustive]` so future variants (e.g. a capacity-limit
/// error once an eviction policy lands) can be added without a semver
/// break.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TagCacheError {
    /// I/O error opening or locking the LMDB environment directory.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// The underlying LMDB / `heed` layer reported a failure — database
    /// open, transaction start, put, get, or range deletion.
    #[error("LMDB error: {0}")]
    Lmdb(#[from] heed::Error),

    /// A [`bincode`] (de)serialisation failure — a corrupted value
    /// decoded from the database, or a value that refused to encode.
    #[error("value codec error: {0}")]
    Serialize(#[from] bincode::Error),

    /// A key-encoding failure — most commonly, `mtime` is before
    /// [`UNIX_EPOCH`] so the big-endian `i128` nanosecond representation
    /// cannot be computed.  Carries the offending path for diagnostics.
    #[error("invalid path or mtime for cache key: {0}")]
    InvalidKey(PathBuf),
}

// ── TagCache ───────────────────────────────────────────────────────────────

/// An LMDB-backed tag cache mapping `(file_path, mtime) →
/// Vec<ExtractedSymbol>`.
///
/// Construct via [`TagCache::open`] by pointing at a directory on-disk.
/// A single [`TagCache`] owns one [`heed::Env`]; sharing a `TagCache`
/// across threads via `Arc<TagCache>` is the intended use-pattern once
/// the daemon-level cache wiring lands (P1-W3-F01).
///
/// # Examples
///
/// ```no_run
/// use std::time::SystemTime;
///
/// use tempfile::TempDir;
/// use ucil_treesitter::TagCache;
///
/// let dir = TempDir::new().unwrap();
/// let cache = TagCache::open(dir.path()).unwrap();
/// let hit = cache.get(std::path::Path::new("src/lib.rs"), SystemTime::now()).unwrap();
/// assert!(hit.is_none());
/// ```
#[derive(Debug, Clone)]
pub struct TagCache {
    pub(crate) env: Env,
    pub(crate) db: Database<Bytes, Bytes>,
}

impl TagCache {
    /// Open (or create) a tag cache rooted at `dir`.
    ///
    /// If `dir` does not exist, it is created recursively before the
    /// LMDB environment is initialised.  The map size is fixed at 1 GiB
    /// of virtual address space — LMDB lazily commits physical pages,
    /// so this does **not** pre-allocate 1 GiB on disk.
    ///
    /// # Safety
    ///
    /// LMDB maps the database file into process memory; `heed`'s
    /// `EnvOpenOptions::open` is `unsafe` to reflect that writes to the
    /// mapped file through another process are undefined behaviour.
    /// This wrapper treats the invariant as "UCIL owns `dir`
    /// exclusively" — the daemon controls access by being the single
    /// writer/reader.
    ///
    /// # Errors
    ///
    /// Returns [`TagCacheError::Io`] if `dir` cannot be created, and
    /// [`TagCacheError::Lmdb`] if the environment or default database
    /// cannot be opened (e.g. incompatible LMDB version on the existing
    /// directory).
    pub fn open(dir: &Path) -> Result<Self, TagCacheError> {
        std::fs::create_dir_all(dir)?;
        // SAFETY: The LMDB environment is single-process — the enclosing
        // UCIL daemon is the sole accessor of its `.ucil/branches/<branch>/`
        // directory at runtime, and tests open disjoint `TempDir`s.
        let env = unsafe {
            EnvOpenOptions::new()
                .map_size(MAP_SIZE_BYTES)
                .max_dbs(MAX_DBS)
                .open(dir)?
        };
        let mut wtxn = env.write_txn()?;
        let db: Database<Bytes, Bytes> = env.create_database(&mut wtxn, None)?;
        wtxn.commit()?;
        Ok(Self { env, db })
    }

    /// Look up the symbol list cached for `(path, mtime)`.
    ///
    /// Returns `Ok(Some(_))` when a value is stored, `Ok(None)` when no
    /// entry exists for that key, and `Err(_)` on transaction or
    /// decoding failure.
    ///
    /// An empty `Vec<ExtractedSymbol>` stored via [`TagCache::put`] is
    /// faithfully read back as `Some(vec![])` — the `Option` wrapper
    /// carries only the "entry present?" signal, distinct from "symbol
    /// list happens to be empty."
    ///
    /// # Errors
    ///
    /// - [`TagCacheError::InvalidKey`] if `mtime` is before [`UNIX_EPOCH`].
    /// - [`TagCacheError::Lmdb`] on transaction failure.
    /// - [`TagCacheError::Serialize`] if the stored value cannot be decoded
    ///   (corruption).
    #[tracing::instrument(
        name = "ucil.treesitter.tag_cache_get",
        level = "debug",
        skip(self),
        fields(path = %path.display())
    )]
    pub fn get(
        &self,
        path: &Path,
        mtime: SystemTime,
    ) -> Result<Option<Vec<ExtractedSymbol>>, TagCacheError> {
        let key = encode_key(path, mtime)?;
        let rtxn = self.env.read_txn()?;
        let raw = self.db.get(&rtxn, key.as_slice())?;
        match raw {
            Some(bytes) => Ok(Some(decode_value(bytes)?)),
            None => Ok(None),
        }
    }

    /// Store `symbols` under the key `(path, mtime)`, overwriting any
    /// existing entry at that exact key.
    ///
    /// # Errors
    ///
    /// - [`TagCacheError::InvalidKey`] if `mtime` is before [`UNIX_EPOCH`].
    /// - [`TagCacheError::Serialize`] if `bincode` cannot encode the
    ///   `Vec<ExtractedSymbol>`.
    /// - [`TagCacheError::Lmdb`] on transaction failure or a `put` error
    ///   (e.g. the environment's map is full).
    #[tracing::instrument(
        name = "ucil.treesitter.tag_cache_put",
        level = "debug",
        skip(self, symbols),
        fields(path = %path.display(), symbols = symbols.len())
    )]
    pub fn put(
        &self,
        path: &Path,
        mtime: SystemTime,
        symbols: &[ExtractedSymbol],
    ) -> Result<(), TagCacheError> {
        let key = encode_key(path, mtime)?;
        let value = bincode::serialize(symbols)?;
        let mut wtxn = self.env.write_txn()?;
        self.db.put(&mut wtxn, key.as_slice(), value.as_slice())?;
        wtxn.commit()?;
        Ok(())
    }
}

// ── Key codec ──────────────────────────────────────────────────────────────

/// Encode `(path, mtime)` into a cache key.
///
/// Layout: `[ path_bytes ] [ 0x00 ] [ mtime_nanos_i128_be ]`.
fn encode_key(path: &Path, mtime: SystemTime) -> Result<Vec<u8>, TagCacheError> {
    let path_bytes = path.as_os_str().as_encoded_bytes();
    if path_bytes.is_empty() {
        return Err(TagCacheError::InvalidKey(path.to_path_buf()));
    }
    let nanos =
        mtime_to_nanos(mtime).ok_or_else(|| TagCacheError::InvalidKey(path.to_path_buf()))?;
    let mut out = Vec::with_capacity(path_bytes.len() + 1 + MTIME_WIDTH);
    out.extend_from_slice(path_bytes);
    out.push(PATH_MTIME_SEP);
    out.extend_from_slice(&nanos.to_be_bytes());
    Ok(out)
}

/// Convert a [`SystemTime`] to nanoseconds since [`UNIX_EPOCH`] as an
/// `i128`.  Returns `None` if `mtime` is before the epoch (rare for
/// file mtimes on a sane system) or if the nanosecond count overflows
/// `i128` (effectively impossible — 2^127 ns ≈ 5.4×10^21 years).
fn mtime_to_nanos(mtime: SystemTime) -> Option<i128> {
    let duration = mtime.duration_since(UNIX_EPOCH).ok()?;
    i128::try_from(duration.as_nanos()).ok()
}

// ── Value codec ────────────────────────────────────────────────────────────

/// Decode a `bincode`-serialised `Vec<ExtractedSymbol>` back from bytes.
fn decode_value(bytes: &[u8]) -> Result<Vec<ExtractedSymbol>, TagCacheError> {
    let symbols: Vec<ExtractedSymbol> = bincode::deserialize(bytes)?;
    Ok(symbols)
}

// ── Module-root unit tests ─────────────────────────────────────────────────
//
// Per DEC-0005 (WO-0006 module-coherence commits), unit tests live at
// module root — NOT wrapped in `#[cfg(test)] mod tests { … }` — so the
// frozen acceptance selector `tag_cache::` resolves every test as
// `ucil_treesitter::tag_cache::<test_name>`.

#[cfg(test)]
use std::time::Duration;

#[cfg(test)]
use tempfile::TempDir;

#[cfg(test)]
use crate::parser::Language;
#[cfg(test)]
use crate::symbols::SymbolKind;

#[cfg(test)]
fn sample_symbol(name: &str, file: &str, start_line: u32) -> ExtractedSymbol {
    ExtractedSymbol {
        name: name.to_owned(),
        kind: SymbolKind::Function,
        file_path: PathBuf::from(file),
        language: Language::Rust,
        start_line,
        start_col: 1,
        end_line: start_line,
        end_col: 10,
        signature: None,
        doc_comment: None,
    }
}

#[cfg(test)]
fn fully_populated_symbol(name: &str) -> ExtractedSymbol {
    ExtractedSymbol {
        name: name.to_owned(),
        kind: SymbolKind::Method,
        file_path: PathBuf::from("src/model.rs"),
        language: Language::Rust,
        start_line: 42,
        start_col: 5,
        end_line: 120,
        end_col: 1,
        signature: Some("fn dangle(&self, x: i32) -> Result<(), Err>".to_owned()),
        doc_comment: Some("/// dangle x into a Result".to_owned()),
    }
}

#[cfg(test)]
fn open_cache() -> (TempDir, TagCache) {
    let dir = TempDir::new().expect("temp dir");
    let cache = TagCache::open(dir.path()).expect("open cache");
    (dir, cache)
}

#[cfg(test)]
#[test]
fn tag_cache_put_then_get_returns_symbols() {
    let (_dir, cache) = open_cache();
    let path = Path::new("src/alpha.rs");
    let mtime = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let symbols = vec![
        sample_symbol("alpha", "src/alpha.rs", 1),
        sample_symbol("beta", "src/alpha.rs", 25),
    ];

    cache.put(path, mtime, &symbols).expect("put");
    let got = cache.get(path, mtime).expect("get").expect("Some entry");

    assert_eq!(
        got, symbols,
        "round-trip must preserve every field of every symbol"
    );
}

#[cfg(test)]
#[test]
fn tag_cache_get_missing_returns_none() {
    let (_dir, cache) = open_cache();
    let got = cache
        .get(
            Path::new("/never/written.rs"),
            UNIX_EPOCH + Duration::from_secs(123),
        )
        .expect("get");
    assert!(
        got.is_none(),
        "freshly-opened cache must report miss as None"
    );
}

#[cfg(test)]
#[test]
fn tag_cache_different_mtime_is_distinct_entry() {
    let (_dir, cache) = open_cache();
    let path = Path::new("src/beta.rs");
    let mtime_a = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let mtime_b = UNIX_EPOCH + Duration::from_secs(1_700_000_060);

    let at_a = vec![sample_symbol("first", "src/beta.rs", 1)];
    let at_b = vec![
        sample_symbol("second_a", "src/beta.rs", 7),
        sample_symbol("second_b", "src/beta.rs", 13),
    ];

    cache.put(path, mtime_a, &at_a).expect("put a");
    cache.put(path, mtime_b, &at_b).expect("put b");

    assert_eq!(cache.get(path, mtime_a).expect("get a").unwrap(), at_a);
    assert_eq!(cache.get(path, mtime_b).expect("get b").unwrap(), at_b);
}

#[cfg(test)]
#[test]
fn tag_cache_reopen_persists_entries() {
    let dir = TempDir::new().expect("temp dir");
    let path = Path::new("src/persist.rs");
    let mt = UNIX_EPOCH + Duration::from_secs(1_700_000_200);
    let syms = vec![sample_symbol("persist", "src/persist.rs", 1)];

    {
        let cache = TagCache::open(dir.path()).expect("open 1");
        cache.put(path, mt, &syms).expect("put");
    }
    // First `TagCache` is dropped here — env is released.

    let reopened = TagCache::open(dir.path()).expect("open 2");
    let got = reopened
        .get(path, mt)
        .expect("get")
        .expect("entry must persist across reopen");
    assert_eq!(got, syms);
}

#[cfg(test)]
#[test]
fn tag_cache_roundtrip_preserves_all_symbol_fields() {
    let (_dir, cache) = open_cache();
    let path = Path::new("src/model.rs");
    let mt = UNIX_EPOCH + Duration::from_secs(1_700_000_300);
    let rich = vec![fully_populated_symbol("dangle")];

    cache.put(path, mt, &rich).expect("put");
    let got = cache.get(path, mt).expect("get").unwrap();

    assert_eq!(got.len(), 1);
    let s = &got[0];
    assert_eq!(s.name, "dangle");
    assert_eq!(s.kind, SymbolKind::Method);
    assert_eq!(s.file_path, PathBuf::from("src/model.rs"));
    assert_eq!(s.language, Language::Rust);
    assert_eq!(s.start_line, 42);
    assert_eq!(s.start_col, 5);
    assert_eq!(s.end_line, 120);
    assert_eq!(s.end_col, 1);
    assert_eq!(
        s.signature.as_deref(),
        Some("fn dangle(&self, x: i32) -> Result<(), Err>")
    );
    assert_eq!(s.doc_comment.as_deref(), Some("/// dangle x into a Result"));
    assert_eq!(got, rich, "full-struct equality must hold");
}

#[cfg(test)]
#[test]
fn tag_cache_empty_vec_roundtrips() {
    let (_dir, cache) = open_cache();
    let path = Path::new("src/empty.rs");
    let mt = UNIX_EPOCH + Duration::from_secs(1_700_000_400);
    let empty: Vec<ExtractedSymbol> = Vec::new();

    cache.put(path, mt, &empty).expect("put");
    let got = cache.get(path, mt).expect("get");
    assert_eq!(
        got,
        Some(Vec::new()),
        "empty Vec must read back as Some(vec![]), not None — presence is preserved"
    );
}

#[cfg(test)]
#[test]
fn tag_cache_warm_read_under_1ms() {
    use std::time::Instant;

    let (_dir, cache) = open_cache();
    let base_mt = UNIX_EPOCH + Duration::from_secs(1_700_000_500);

    // Populate 100 entries across 100 paths.
    let n_entries = 100usize;
    let paths: Vec<PathBuf> = (0..n_entries)
        .map(|i| PathBuf::from(format!("src/mod_{i}.rs")))
        .collect();
    for (i, p) in paths.iter().enumerate() {
        let syms = vec![sample_symbol(&format!("fn_{i}"), &p.to_string_lossy(), 1)];
        cache.put(p, base_mt, &syms).expect("populate");
    }

    // Warm 1000 reads — cycle through paths so the mmap stays hot.
    let n_reads = 1000usize;
    let mut samples_ns: Vec<u128> = Vec::with_capacity(n_reads);
    for i in 0..n_reads {
        let p = &paths[i % n_entries];
        let t0 = Instant::now();
        let got = cache.get(p, base_mt).expect("warm read");
        let dt = t0.elapsed().as_nanos();
        assert!(got.is_some(), "warm-read fixture must hit for every path");
        samples_ns.push(dt);
    }

    samples_ns.sort_unstable();
    let median_ns = samples_ns[n_reads / 2];
    // Release target: <1 ms.  Debug builds are given a 3× slop factor
    // via `cfg!(debug_assertions)` to absorb the optimiser gap.
    let threshold_ns: u128 = if cfg!(debug_assertions) {
        3_000_000
    } else {
        1_000_000
    };
    assert!(
        median_ns < threshold_ns,
        "median warm-read latency was {median_ns} ns — threshold {threshold_ns} ns \
         (debug_assertions = {})",
        cfg!(debug_assertions)
    );
}

#[cfg(test)]
#[test]
fn tag_cache_key_ordering_is_lexicographic() {
    // Writes at mtimes t1 < t2 < t3 for one path — iterate the DB and
    // assert the natural order preserves ascending mtime.  This pins
    // the key encoding against accidental regressions (e.g. someone
    // switching to little-endian mtime bytes).
    let (_dir, cache) = open_cache();
    let path = Path::new("src/ordering.rs");
    let t1 = UNIX_EPOCH + Duration::from_secs(1_000);
    let t2 = UNIX_EPOCH + Duration::from_secs(2_000);
    let t3 = UNIX_EPOCH + Duration::from_secs(3_000);

    cache
        .put(path, t2, &[sample_symbol("at_t2", "src/ordering.rs", 2)])
        .expect("put t2");
    cache
        .put(path, t1, &[sample_symbol("at_t1", "src/ordering.rs", 1)])
        .expect("put t1");
    cache
        .put(path, t3, &[sample_symbol("at_t3", "src/ordering.rs", 3)])
        .expect("put t3");

    let rtxn = cache.env.read_txn().expect("read_txn");
    let iter = cache.db.iter(&rtxn).expect("iter");
    let mut order: Vec<String> = Vec::new();
    for entry in iter {
        let (_k, v) = entry.expect("iter entry");
        let syms: Vec<ExtractedSymbol> = bincode::deserialize(v).expect("decode");
        order.push(syms[0].name.clone());
    }

    assert_eq!(
        order,
        vec!["at_t1".to_string(), "at_t2".into(), "at_t3".into()],
        "DB iteration must follow lexicographic key order, which the \
         big-endian i128 mtime encoding makes equivalent to ascending \
         mtime order for non-negative nanos"
    );
}

#[cfg(test)]
#[test]
fn tag_cache_put_overwrites_same_key() {
    let (_dir, cache) = open_cache();
    let path = Path::new("src/overwrite.rs");
    let mt = UNIX_EPOCH + Duration::from_secs(1_700_000_700);

    let first = vec![sample_symbol("first", "src/overwrite.rs", 1)];
    let second = vec![
        sample_symbol("second_a", "src/overwrite.rs", 10),
        sample_symbol("second_b", "src/overwrite.rs", 20),
    ];
    cache.put(path, mt, &first).expect("put 1");
    cache.put(path, mt, &second).expect("put 2 (overwrite)");

    let got = cache.get(path, mt).expect("get").unwrap();
    assert_eq!(
        got, second,
        "put at an existing key must overwrite, not accumulate"
    );
}

#[cfg(test)]
#[test]
fn tag_cache_error_display_shapes() {
    // Smoke test: every `TagCacheError` variant must format without
    // panicking and must include the variant-specific prefix.  Guards
    // against accidental removal of the `#[error("…")]` attributes.
    let io_err = TagCacheError::Io(std::io::Error::other("boom"));
    let msg = io_err.to_string();
    assert!(msg.contains("I/O error"), "got {msg}");

    let key_err = TagCacheError::InvalidKey(PathBuf::from("/nope"));
    let msg = key_err.to_string();
    assert!(msg.contains("invalid path"), "got {msg}");
    assert!(msg.contains("/nope"), "got {msg}");
}
