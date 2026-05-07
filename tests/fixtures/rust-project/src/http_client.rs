//! Minimal HTTP retry helper with exponential backoff.
//!
//! Provides `retry_with_backoff`, a generic retry combinator that retries
//! a closure up to `max_attempts` times, doubling the delay between attempts
//! starting from `initial_delay`. Used by `main.rs` for a deterministic
//! demo banner-fetch on startup.
//!
//! Authorised by ADR DEC-0017 (effectiveness-scenario fixture augmentation).

use std::thread;
use std::time::Duration;

/// Retry an operation up to `max_attempts` times with exponential backoff.
///
/// Delay starts at `initial_delay` and doubles after each failed attempt.
/// Returns the final `Err` if every attempt fails; otherwise returns the
/// first `Ok`.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use rust_project::http_client::retry_with_backoff;
///
/// let mut tries = 0;
/// let result: Result<&str, &str> = retry_with_backoff(
///     || {
///         tries += 1;
///         if tries < 3 { Err("not yet") } else { Ok("hello") }
///     },
///     5,
///     Duration::from_millis(1),
/// );
/// assert_eq!(result, Ok("hello"));
/// assert_eq!(tries, 3);
/// ```
pub fn retry_with_backoff<F, T, E>(
    mut op: F,
    max_attempts: u32,
    initial_delay: Duration,
) -> Result<T, E>
where
    F: FnMut() -> Result<T, E>,
{
    let mut delay = initial_delay;
    for attempt in 1..=max_attempts {
        match op() {
            Ok(v) => return Ok(v),
            Err(e) if attempt == max_attempts => return Err(e),
            Err(_) => {
                thread::sleep(delay);
                delay = delay.checked_mul(2).unwrap_or(delay);
            }
        }
    }
    unreachable!("loop guarantees a return on the last attempt")
}

/// Fetch a startup banner with one retry. Backed by an in-process closure
/// for fixture determinism — no real network. The closure succeeds on the
/// second attempt to exercise the retry path.
pub fn fetch_startup_banner() -> Result<&'static str, &'static str> {
    let mut attempts = 0u32;
    retry_with_backoff(
        || {
            attempts += 1;
            if attempts < 2 {
                Err("transient")
            } else {
                Ok("rust-project")
            }
        },
        3,
        Duration::from_millis(1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_returns_ok_on_first_success() {
        let result: Result<i32, &str> = retry_with_backoff(|| Ok(42), 3, Duration::from_millis(1));
        assert_eq!(result, Ok(42));
    }

    #[test]
    fn retry_doubles_delay_and_eventually_succeeds() {
        let mut attempts = 0;
        let result: Result<i32, &str> = retry_with_backoff(
            || {
                attempts += 1;
                if attempts < 3 {
                    Err("not yet")
                } else {
                    Ok(7)
                }
            },
            5,
            Duration::from_millis(1),
        );
        assert_eq!(result, Ok(7));
        assert_eq!(attempts, 3);
    }

    #[test]
    fn retry_returns_last_error_when_max_attempts_reached() {
        let mut attempts = 0;
        let result: Result<i32, &str> = retry_with_backoff(
            || {
                attempts += 1;
                Err("persistent")
            },
            3,
            Duration::from_millis(1),
        );
        assert_eq!(result, Err("persistent"));
        assert_eq!(attempts, 3);
    }

    #[test]
    fn fetch_startup_banner_succeeds_via_retry() {
        let banner = fetch_startup_banner().expect("retry should succeed");
        assert_eq!(banner, "rust-project");
    }
}
