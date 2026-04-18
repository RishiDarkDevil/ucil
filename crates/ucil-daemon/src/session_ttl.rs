//! Session TTL (time-to-live) helpers — P1-W4-F07.
//!
//! Centralises the saturating arithmetic used to derive a session's
//! `expires_at` from its `created_at` and a TTL.  Extracted from
//! [`crate::session_manager`] so that the policy
//! (`expires_at = created_at + ttl`, saturating on overflow) has a
//! single source of truth and a dedicated test surface.
//!
//! Master-plan §11 (daemon internals, session model) specifies unix-
//! seconds timestamps for session timekeeping; this module sticks to
//! `u64` seconds so it composes directly with
//! [`std::time::SystemTime::duration_since`] output.
//!
//! # Example
//!
//! ```
//! use ucil_daemon::session_ttl::{compute_expires_at, DEFAULT_TTL_SECS};
//!
//! let created_at: u64 = 1_700_000_000;
//! assert_eq!(
//!     compute_expires_at(created_at, DEFAULT_TTL_SECS),
//!     created_at + DEFAULT_TTL_SECS,
//! );
//! ```

/// Default session TTL in seconds — 1 hour.
///
/// The `SessionManager::create_session` path uses this when the caller
/// does not supply an explicit TTL; callers that want a different
/// window use [`SessionManager::set_ttl`](crate::SessionManager::set_ttl)
/// after creation.
pub const DEFAULT_TTL_SECS: u64 = 3600;

/// Compute `expires_at` as `created_at + ttl_secs`, clamped to
/// [`u64::MAX`] on overflow.
///
/// Saturating is the right choice here: a clamped-high expiry is
/// effectively "never expires" which is harmless for the purge path
/// (`expires_at > now_secs` will be true for any plausible `now`).  A
/// wrapped low expiry would cause premature purging of the session.
///
/// # Examples
///
/// ```
/// use ucil_daemon::session_ttl::compute_expires_at;
///
/// assert_eq!(compute_expires_at(10, 5), 15);
/// assert_eq!(compute_expires_at(u64::MAX, 1), u64::MAX); // saturates
/// assert_eq!(compute_expires_at(100, 0), 100);          // ttl=0 edge
/// ```
#[must_use]
pub const fn compute_expires_at(created_at: u64, ttl_secs: u64) -> u64 {
    created_at.saturating_add(ttl_secs)
}

/// `true` when `expires_at <= now_secs`, i.e. the session's window
/// has elapsed and the purge path should evict it.
///
/// Separated from the raw comparison so callers can read
/// `is_expired(info.expires_at, now)` at the call site instead of
/// an inline relational operator, which improves readability in the
/// `retain` closure of `SessionManager::purge_expired`.
///
/// # Examples
///
/// ```
/// use ucil_daemon::session_ttl::is_expired;
///
/// assert!(is_expired(10, 20));      // expired 10s ago
/// assert!(is_expired(10, 10));      // exactly at boundary — expired
/// assert!(!is_expired(20, 10));     // still has 10s left
/// ```
#[must_use]
pub const fn is_expired(expires_at: u64, now_secs: u64) -> bool {
    expires_at <= now_secs
}

// ── Unit tests ────────────────────────────────────────────────────────────────
//
// Tests placed at module root (not inside `mod tests {}`) so nextest
// exact-match selectors like `session_ttl::test_*` resolve to them
// without an intervening `::tests::` segment (DEC-0005).

#[cfg(test)]
#[tokio::test(flavor = "current_thread")]
async fn test_compute_expires_at_basic_addition() {
    assert_eq!(compute_expires_at(1000, 3600), 4600);
}

#[cfg(test)]
#[tokio::test(flavor = "current_thread")]
async fn test_compute_expires_at_saturates_on_overflow() {
    // `u64::MAX + 1` would overflow; saturating returns u64::MAX.
    assert_eq!(compute_expires_at(u64::MAX, 1), u64::MAX);
    assert_eq!(compute_expires_at(u64::MAX - 1, 10), u64::MAX);
}

#[cfg(test)]
#[tokio::test(flavor = "current_thread")]
async fn test_compute_expires_at_zero_ttl_yields_created_at() {
    // ttl=0 is an odd but legal value — lets callers force immediate
    // expiry of a session.
    assert_eq!(compute_expires_at(12_345, 0), 12_345);
}

#[cfg(test)]
#[tokio::test(flavor = "current_thread")]
async fn test_default_ttl_is_one_hour() {
    assert_eq!(DEFAULT_TTL_SECS, 3600);
}

#[cfg(test)]
#[tokio::test(flavor = "current_thread")]
async fn test_is_expired_boundary_is_inclusive() {
    // At exactly `now == expires_at` the session is considered
    // expired — purge_expired uses `expires_at > now_secs` as the
    // KEEP predicate, so `==` evicts.
    assert!(is_expired(100, 100));
    assert!(is_expired(100, 101));
    assert!(!is_expired(100, 99));
}
