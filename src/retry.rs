//! Retry primitives: policy, backoff strategies, `Retry-After` parsing, and
//! the `Idempotent` marker trait.
//!
//! # Overview
//!
//! | Type / Trait       | Issue | Purpose                                               |
//! |--------------------|-------|-------------------------------------------------------|
//! | [`RetryPolicy`]    | #112  | Max-attempt cap + backoff strategy                    |
//! | [`BackoffStrategy`]| #112  | Fixed, Exponential, or DecorrelatedJitter delays      |
//! | [`RetryAfter`]     | #113  | Parse `Retry-After` header (delta-seconds or date)    |
//! | [`Idempotent`]     | #114  | Marker trait for request types safe to retry          |
//!
//! # Examples
//!
//! ```rust
//! use api_bones::retry::{BackoffStrategy, Idempotent, RetryPolicy};
//! use core::time::Duration;
//!
//! struct GetUser { id: u64 }
//! impl Idempotent for GetUser {}
//!
//! fn should_retry<R: Idempotent>(req: &R, policy: &RetryPolicy, attempt: u32) -> bool {
//!     attempt < policy.max_attempts
//! }
//!
//! let policy = RetryPolicy::exponential(3, Duration::from_millis(100));
//! let delay = policy.next_delay(1);
//! assert!(delay >= Duration::from_millis(100));
//! ```

use core::time::Duration;

// ---------------------------------------------------------------------------
// BackoffStrategy
// ---------------------------------------------------------------------------

/// Backoff strategy used by [`RetryPolicy`] to compute inter-attempt delays.
///
/// | Variant              | Description                                           |
/// |----------------------|-------------------------------------------------------|
/// | `Fixed`              | Always wait the same `base` duration                 |
/// | `Exponential`        | `base * 2^attempt` (capped at `max_delay`)           |
/// | `DecorrelatedJitter` | Jittered delay in `[base, prev * 3]` (AWS algorithm) |
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum BackoffStrategy {
    /// Return `base` unconditionally on every attempt.
    Fixed {
        /// Constant delay between attempts.
        base: Duration,
    },
    /// Double the delay on each attempt, capping at `max_delay`.
    ///
    /// `delay(attempt) = min(base * 2^attempt, max_delay)`
    Exponential {
        /// Initial delay (attempt 0).
        base: Duration,
        /// Upper bound for computed delays.
        max_delay: Duration,
    },
    /// Decorrelated jitter as described by AWS:
    /// `delay = random_between(base, prev_delay * 3)`.
    ///
    /// [`RetryPolicy::next_delay`] uses a deterministic approximation
    /// (`base + (prev_delay * 3 - base) / 2`) so that the type needs no
    /// random-number-generator dependency.  Callers that want true
    /// randomness should implement jitter on top using the returned duration
    /// as an upper bound.
    DecorrelatedJitter {
        /// Minimum delay floor.
        base: Duration,
        /// Upper bound for any single delay.
        max_delay: Duration,
    },
}

// ---------------------------------------------------------------------------
// RetryPolicy  (#112)
// ---------------------------------------------------------------------------

/// Retry policy combining a maximum-attempt cap with a [`BackoffStrategy`].
///
/// # Integration hints
///
/// - **reqwest-retry** (`reqwest-middleware`): implement `RetryableStrategy`
///   and call [`RetryPolicy::next_delay`] from `retry_decision`.
/// - **tower-retry**: implement `tower::retry::Policy` and delegate to
///   [`RetryPolicy::next_delay`] inside `retry`.
///
/// # Examples
///
/// ```rust
/// use api_bones::retry::RetryPolicy;
/// use core::time::Duration;
///
/// // Fixed: always wait 500 ms, up to 5 attempts.
/// let fixed = RetryPolicy::fixed(5, Duration::from_millis(500));
/// assert_eq!(fixed.next_delay(0), Duration::from_millis(500));
/// assert_eq!(fixed.next_delay(4), Duration::from_millis(500));
///
/// // Exponential: 100 ms → 200 ms → 400 ms … capped at 2 s.
/// let exp = RetryPolicy::exponential(4, Duration::from_millis(100));
/// assert_eq!(exp.next_delay(0), Duration::from_millis(100));
/// assert_eq!(exp.next_delay(1), Duration::from_millis(200));
/// assert_eq!(exp.next_delay(2), Duration::from_millis(400));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct RetryPolicy {
    /// Maximum number of retry attempts (excluding the original request).
    pub max_attempts: u32,
    /// Backoff strategy used to compute delays between attempts.
    pub strategy: BackoffStrategy,
}

impl RetryPolicy {
    /// Create a policy with a [`BackoffStrategy::Fixed`] delay.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::retry::RetryPolicy;
    /// use core::time::Duration;
    ///
    /// let p = RetryPolicy::fixed(3, Duration::from_secs(1));
    /// assert_eq!(p.max_attempts, 3);
    /// assert_eq!(p.next_delay(99), Duration::from_secs(1));
    /// ```
    #[must_use]
    pub fn fixed(max_attempts: u32, base: Duration) -> Self {
        Self {
            max_attempts,
            strategy: BackoffStrategy::Fixed { base },
        }
    }

    /// Create a policy with a [`BackoffStrategy::Exponential`] delay
    /// capped at `base * 2^10` (≈ 1024× the base).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::retry::RetryPolicy;
    /// use core::time::Duration;
    ///
    /// let p = RetryPolicy::exponential(5, Duration::from_millis(50));
    /// assert_eq!(p.next_delay(0), Duration::from_millis(50));
    /// assert_eq!(p.next_delay(1), Duration::from_millis(100));
    /// ```
    #[must_use]
    pub fn exponential(max_attempts: u32, base: Duration) -> Self {
        // Default cap: 1024 × base (same as many HTTP client defaults).
        let max_delay = base * 1024;
        Self {
            max_attempts,
            strategy: BackoffStrategy::Exponential { base, max_delay },
        }
    }

    /// Create a policy with a [`BackoffStrategy::DecorrelatedJitter`] delay.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::retry::RetryPolicy;
    /// use core::time::Duration;
    ///
    /// let p = RetryPolicy::decorrelated_jitter(4, Duration::from_millis(100));
    /// let d = p.next_delay(1);
    /// assert!(d >= Duration::from_millis(100));
    /// ```
    #[must_use]
    pub fn decorrelated_jitter(max_attempts: u32, base: Duration) -> Self {
        let max_delay = base * 1024;
        Self {
            max_attempts,
            strategy: BackoffStrategy::DecorrelatedJitter { base, max_delay },
        }
    }

    /// Compute the delay to wait before attempt number `attempt`.
    ///
    /// `attempt` is **0-indexed**: pass `0` before the first retry,
    /// `1` before the second, and so on.
    ///
    /// For [`BackoffStrategy::DecorrelatedJitter`] this function returns the
    /// **midpoint** of `[base, min(base * 3^(attempt+1), max_delay)]` as a
    /// deterministic approximation.  Callers wanting true randomness should
    /// use the returned value as an upper bound and sample uniformly in
    /// `[base, returned_delay]`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::retry::RetryPolicy;
    /// use core::time::Duration;
    ///
    /// let policy = RetryPolicy::exponential(5, Duration::from_millis(100));
    /// assert_eq!(policy.next_delay(0), Duration::from_millis(100));
    /// assert_eq!(policy.next_delay(1), Duration::from_millis(200));
    /// assert_eq!(policy.next_delay(2), Duration::from_millis(400));
    /// // Capped at base * 1024 = 102_400 ms
    /// assert_eq!(policy.next_delay(20), Duration::from_millis(102_400));
    /// ```
    #[must_use]
    pub fn next_delay(&self, attempt: u32) -> Duration {
        match &self.strategy {
            BackoffStrategy::Fixed { base } => *base,
            BackoffStrategy::Exponential { base, max_delay } => {
                // Saturating shift to avoid overflow on large `attempt` values.
                let multiplier = 1_u64.checked_shl(attempt).unwrap_or(u64::MAX);
                let delay = base.saturating_mul(u32::try_from(multiplier).unwrap_or(u32::MAX));
                delay.min(*max_delay)
            }
            BackoffStrategy::DecorrelatedJitter { base, max_delay } => {
                // Deterministic midpoint approximation:
                // upper = min(base * 3^(attempt+1), max_delay)
                // result = base + (upper - base) / 2
                let mut upper = *base;
                for _ in 0..=attempt {
                    upper = upper.saturating_mul(3).min(*max_delay);
                }
                let half_range = upper.saturating_sub(*base) / 2;
                (*base + half_range).min(*max_delay)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// RetryAfter  (#113)
// ---------------------------------------------------------------------------

/// Parsed value of an HTTP `Retry-After` response header.
///
/// The header supports two forms (RFC 9110 §10.2.3):
/// - **Delta-seconds**: a non-negative integer — `Retry-After: 120`
/// - **HTTP-date**: an RFC 7231 date — `Retry-After: Wed, 21 Oct 2015 07:28:00 GMT`
///
/// # Parsing
///
/// ```rust
/// use api_bones::retry::RetryAfter;
/// use core::time::Duration;
///
/// let delay: RetryAfter = "120".parse().unwrap();
/// assert_eq!(delay, RetryAfter::Delay(Duration::from_secs(120)));
///
/// let date: RetryAfter = "Wed, 21 Oct 2015 07:28:00 GMT".parse().unwrap();
/// matches!(date, RetryAfter::Date(_));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum RetryAfter {
    /// A relative delay expressed as a [`Duration`].
    Delay(Duration),
    /// An absolute point-in-time expressed as an RFC 7231 HTTP-date string.
    ///
    /// Stored as a raw string to avoid a mandatory `chrono` dependency.
    /// Parse it with [`chrono::DateTime::parse_from_rfc2822`] when the
    /// `chrono` feature is enabled.
    Date(
        #[cfg_attr(feature = "serde", serde(rename = "date"))]
        /// Raw HTTP-date string (RFC 7231 / RFC 5322 format).
        RetryAfterDate,
    ),
}

/// Inner date representation for [`RetryAfter::Date`].
///
/// When the `chrono` feature is enabled this is a
/// `chrono::DateTime<chrono::FixedOffset>`; otherwise it is a raw `&'static`-
/// free heap string (requires `alloc` or `std`).
#[cfg(feature = "chrono")]
pub type RetryAfterDate = chrono::DateTime<chrono::FixedOffset>;

/// Inner date representation for [`RetryAfter::Date`] (string fallback).
#[cfg(all(not(feature = "chrono"), any(feature = "std", feature = "alloc")))]
#[cfg(not(feature = "std"))]
pub type RetryAfterDate = alloc::string::String;

/// Inner date representation for [`RetryAfter::Date`] (string fallback, std).
#[cfg(all(not(feature = "chrono"), feature = "std"))]
pub type RetryAfterDate = std::string::String;

// ---------------------------------------------------------------------------
// RetryAfterParseError
// ---------------------------------------------------------------------------

/// Error returned when a `Retry-After` header value cannot be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryAfterParseError(
    #[cfg(any(feature = "std", feature = "alloc"))]
    #[cfg_attr(not(feature = "std"), allow(dead_code))]
    RetryAfterParseErrorInner,
);

#[cfg(any(feature = "std", feature = "alloc"))]
#[derive(Debug, Clone, PartialEq, Eq)]
enum RetryAfterParseErrorInner {
    #[cfg(feature = "chrono")]
    InvalidDate(chrono::ParseError),
    #[cfg(not(feature = "chrono"))]
    InvalidFormat,
}

impl core::fmt::Display for RetryAfterParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        #[cfg(any(feature = "std", feature = "alloc"))]
        match &self.0 {
            #[cfg(feature = "chrono")]
            RetryAfterParseErrorInner::InvalidDate(e) => {
                write!(f, "invalid Retry-After date: {e}")
            }
            #[cfg(not(feature = "chrono"))]
            RetryAfterParseErrorInner::InvalidFormat => {
                f.write_str("Retry-After value must be delta-seconds or an HTTP-date")
            }
        }
        #[cfg(not(any(feature = "std", feature = "alloc")))]
        f.write_str("invalid Retry-After value")
    }
}

#[cfg(feature = "std")]
impl std::error::Error for RetryAfterParseError {
    #[cfg(feature = "chrono")]
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.0 {
            RetryAfterParseErrorInner::InvalidDate(e) => Some(e),
        }
    }
}

// ---------------------------------------------------------------------------
// FromStr for RetryAfter
// ---------------------------------------------------------------------------

#[cfg(any(feature = "std", feature = "alloc"))]
impl core::str::FromStr for RetryAfter {
    type Err = RetryAfterParseError;

    /// Parse a `Retry-After` header value.
    ///
    /// Tries delta-seconds first; falls back to HTTP-date.
    ///
    /// # Errors
    ///
    /// Returns [`RetryAfterParseError`] when the value is neither a valid
    /// non-negative integer nor a parseable HTTP-date string.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim();

        // --- delta-seconds ---
        if let Ok(secs) = trimmed.parse::<u64>() {
            return Ok(Self::Delay(Duration::from_secs(secs)));
        }

        // --- HTTP-date (RFC 7231 / RFC 5322) ---
        #[cfg(feature = "chrono")]
        {
            chrono::DateTime::parse_from_rfc2822(trimmed)
                .map(Self::Date)
                .map_err(|e| RetryAfterParseError(RetryAfterParseErrorInner::InvalidDate(e)))
        }

        #[cfg(not(feature = "chrono"))]
        {
            // Without chrono we accept any non-empty string as a raw date.
            if trimmed.is_empty() {
                Err(RetryAfterParseError(
                    RetryAfterParseErrorInner::InvalidFormat,
                ))
            } else {
                Ok(Self::Date(trimmed.into()))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Display for RetryAfter
// ---------------------------------------------------------------------------

#[cfg(any(feature = "std", feature = "alloc"))]
impl core::fmt::Display for RetryAfter {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Delay(d) => write!(f, "{}", d.as_secs()),
            #[cfg(feature = "chrono")]
            Self::Date(dt) => write!(f, "{}", dt.to_rfc2822()),
            #[cfg(not(feature = "chrono"))]
            Self::Date(s) => f.write_str(s),
        }
    }
}

// ---------------------------------------------------------------------------
// Idempotent  (#114)
// ---------------------------------------------------------------------------

/// Marker trait for request types that are safe to retry.
///
/// A request is idempotent when repeating it produces the same observable
/// side-effects as issuing it once (RFC 9110 §9.2.2).  Typical examples:
/// `GET`, `HEAD`, `PUT`, `DELETE`.
///
/// Implement this trait on your request structs to opt into generic retry
/// helpers that gate retries on idempotency:
///
/// ```rust
/// use api_bones::retry::{Idempotent, RetryPolicy};
/// use core::time::Duration;
///
/// struct DeleteResource { id: u64 }
/// impl Idempotent for DeleteResource {}
///
/// fn maybe_retry<R: Idempotent>(policy: &RetryPolicy, attempt: u32) -> Option<Duration> {
///     if attempt < policy.max_attempts {
///         Some(policy.next_delay(attempt))
///     } else {
///         None
///     }
/// }
///
/// let policy = RetryPolicy::fixed(3, Duration::from_millis(500));
/// let req = DeleteResource { id: 42 };
/// let delay = maybe_retry::<DeleteResource>(&policy, 0);
/// assert_eq!(delay, Some(Duration::from_millis(500)));
/// ```
pub trait Idempotent {}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use core::time::Duration;

    // -----------------------------------------------------------------------
    // RetryPolicy – Fixed
    // -----------------------------------------------------------------------

    #[test]
    fn fixed_delay_is_constant() {
        let p = RetryPolicy::fixed(5, Duration::from_millis(250));
        assert_eq!(p.next_delay(0), Duration::from_millis(250));
        assert_eq!(p.next_delay(3), Duration::from_millis(250));
        assert_eq!(p.next_delay(100), Duration::from_millis(250));
    }

    // -----------------------------------------------------------------------
    // RetryPolicy – Exponential
    // -----------------------------------------------------------------------

    #[test]
    fn exponential_doubles_each_attempt() {
        let p = RetryPolicy::exponential(10, Duration::from_millis(100));
        assert_eq!(p.next_delay(0), Duration::from_millis(100));
        assert_eq!(p.next_delay(1), Duration::from_millis(200));
        assert_eq!(p.next_delay(2), Duration::from_millis(400));
        assert_eq!(p.next_delay(3), Duration::from_millis(800));
    }

    #[test]
    fn exponential_caps_at_max_delay() {
        let p = RetryPolicy::exponential(5, Duration::from_millis(100));
        // base * 1024 = 102_400 ms
        let cap = Duration::from_millis(100) * 1024;
        assert_eq!(p.next_delay(100), cap);
    }

    #[test]
    fn exponential_handles_overflow_gracefully() {
        let p = RetryPolicy::exponential(5, Duration::from_secs(1));
        // Very large attempt: must not panic, must be ≤ max_delay
        let d = p.next_delay(u32::MAX);
        assert!(d <= Duration::from_secs(1) * 1024);
    }

    // -----------------------------------------------------------------------
    // RetryPolicy – DecorrelatedJitter
    // -----------------------------------------------------------------------

    #[test]
    fn jitter_delay_gte_base() {
        let base = Duration::from_millis(100);
        let p = RetryPolicy::decorrelated_jitter(5, base);
        for attempt in 0..10 {
            assert!(
                p.next_delay(attempt) >= base,
                "attempt {attempt}: delay < base"
            );
        }
    }

    #[test]
    fn jitter_delay_lte_max() {
        let base = Duration::from_millis(100);
        let p = RetryPolicy::decorrelated_jitter(5, base);
        let max = base * 1024;
        for attempt in 0..20 {
            assert!(
                p.next_delay(attempt) <= max,
                "attempt {attempt}: delay > max_delay"
            );
        }
    }

    // -----------------------------------------------------------------------
    // RetryAfter – parsing
    // -----------------------------------------------------------------------

    #[cfg(any(feature = "std", feature = "alloc"))]
    #[test]
    fn parse_delta_seconds() {
        let r: RetryAfter = "120".parse().unwrap();
        assert_eq!(r, RetryAfter::Delay(Duration::from_secs(120)));
    }

    #[cfg(any(feature = "std", feature = "alloc"))]
    #[test]
    fn parse_zero_seconds() {
        let r: RetryAfter = "0".parse().unwrap();
        assert_eq!(r, RetryAfter::Delay(Duration::ZERO));
    }

    #[cfg(all(any(feature = "std", feature = "alloc"), feature = "chrono"))]
    #[test]
    fn parse_http_date() {
        let r: RetryAfter = "Wed, 21 Oct 2015 07:28:00 GMT".parse().unwrap();
        assert!(matches!(r, RetryAfter::Date(_)));
    }

    #[cfg(all(any(feature = "std", feature = "alloc"), feature = "chrono"))]
    #[test]
    fn parse_invalid_returns_error() {
        let r: Result<RetryAfter, _> = "not-a-valid-value".parse();
        assert!(r.is_err());
    }

    // -----------------------------------------------------------------------
    // RetryAfter – Display round-trip
    // -----------------------------------------------------------------------

    #[cfg(any(feature = "std", feature = "alloc"))]
    #[test]
    fn display_delay_round_trips() {
        let r = RetryAfter::Delay(Duration::from_secs(60));
        assert_eq!(r.to_string(), "60");
    }

    #[cfg(all(any(feature = "std", feature = "alloc"), feature = "chrono"))]
    #[test]
    fn display_date_round_trips() {
        let original = "Wed, 21 Oct 2015 07:28:00 +0000";
        let r: RetryAfter = original.parse().unwrap();
        // to_string() calls to_rfc2822() — verify it re-parses cleanly
        let back: RetryAfter = r.to_string().parse().unwrap();
        assert_eq!(r, back);
    }

    // -----------------------------------------------------------------------
    // RetryAfter – serde round-trip
    // -----------------------------------------------------------------------

    #[cfg(all(any(feature = "std", feature = "alloc"), feature = "serde"))]
    #[test]
    fn serde_delay_round_trip() {
        let r = RetryAfter::Delay(Duration::from_secs(30));
        let json = serde_json::to_value(&r).unwrap();
        let back: RetryAfter = serde_json::from_value(json).unwrap();
        assert_eq!(back, r);
    }

    // -----------------------------------------------------------------------
    // Idempotent – compile-time marker
    // -----------------------------------------------------------------------

    struct GetItems;
    impl Idempotent for GetItems {}

    fn require_idempotent<R: Idempotent>(_: &R) {}

    #[test]
    fn idempotent_implementor_accepted_by_generic_fn() {
        let req = GetItems;
        require_idempotent(&req);
    }
}
