//! Rate limit metadata types.
//!
//! [`RateLimitInfo`] carries the structured data normally surfaced through
//! `X-RateLimit-*` HTTP response headers, making it easy to include quota
//! information in both successful responses and 429 error bodies.
//!
//! # Example
//!
//! ```rust
//! use shared_types::ratelimit::RateLimitInfo;
//!
//! let info = RateLimitInfo::new(100, 0, 1_700_000_000).retry_after(60);
//! assert!(info.is_exceeded());
//! assert_eq!(info.retry_after, Some(60));
//! ```

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// RateLimitInfo
// ---------------------------------------------------------------------------

/// Structured rate-limit metadata matching `X-RateLimit-*` headers.
///
/// | Field           | HTTP header              | Meaning                               |
/// |-----------------|--------------------------|---------------------------------------|
/// | `limit`         | `X-RateLimit-Limit`      | Max requests allowed in the window    |
/// | `remaining`     | `X-RateLimit-Remaining`  | Requests still available              |
/// | `reset`         | `X-RateLimit-Reset`      | Unix timestamp when the window resets |
/// | `retry_after`   | `Retry-After`            | Seconds to wait before retrying (429) |
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "proptest", derive(proptest_derive::Arbitrary))]
pub struct RateLimitInfo {
    /// Maximum number of requests allowed in the current window.
    pub limit: u64,

    /// Number of requests remaining in the current window.
    pub remaining: u64,

    /// Unix timestamp (seconds) at which the current window resets.
    pub reset: u64,

    /// Seconds the client should wait before retrying (present on 429 responses).
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub retry_after: Option<u64>,
}

impl RateLimitInfo {
    /// Create a new `RateLimitInfo`.
    #[must_use]
    pub fn new(limit: u64, remaining: u64, reset: u64) -> Self {
        Self {
            limit,
            remaining,
            reset,
            retry_after: None,
        }
    }

    /// Set the `retry_after` hint (builder-style).
    #[must_use]
    pub fn retry_after(mut self, seconds: u64) -> Self {
        self.retry_after = Some(seconds);
        self
    }

    /// Return `true` when no requests remain in the current window.
    #[must_use]
    pub fn is_exceeded(&self) -> bool {
        self.remaining == 0
    }
}

// ---------------------------------------------------------------------------
// Axum: header extraction / injection
// ---------------------------------------------------------------------------

#[cfg(feature = "http")]
mod http_impl {
    use super::RateLimitInfo;
    use http::{HeaderMap, HeaderValue};

    /// Header name constants.
    pub const HEADER_LIMIT: &str = "x-ratelimit-limit";
    pub const HEADER_REMAINING: &str = "x-ratelimit-remaining";
    pub const HEADER_RESET: &str = "x-ratelimit-reset";
    pub const HEADER_RETRY_AFTER: &str = "retry-after";

    impl RateLimitInfo {
        /// Inject rate-limit headers into a [`HeaderMap`].
        ///
        /// Inserts `X-RateLimit-Limit`, `X-RateLimit-Remaining`, and
        /// `X-RateLimit-Reset`.  Also inserts `Retry-After` when
        /// [`retry_after`](RateLimitInfo::retry_after) is set.
        pub fn inject_headers(&self, headers: &mut HeaderMap) {
            // These values are u64 formatted to ASCII digits — infallible.
            headers.insert(
                HEADER_LIMIT,
                HeaderValue::from_str(&self.limit.to_string())
                    .expect("u64 decimal is always a valid header value"),
            );
            headers.insert(
                HEADER_REMAINING,
                HeaderValue::from_str(&self.remaining.to_string())
                    .expect("u64 decimal is always a valid header value"),
            );
            headers.insert(
                HEADER_RESET,
                HeaderValue::from_str(&self.reset.to_string())
                    .expect("u64 decimal is always a valid header value"),
            );
            if let Some(secs) = self.retry_after {
                headers.insert(
                    HEADER_RETRY_AFTER,
                    HeaderValue::from_str(&secs.to_string())
                        .expect("u64 decimal is always a valid header value"),
                );
            }
        }

        /// Extract a `RateLimitInfo` from a [`HeaderMap`].
        ///
        /// Returns `None` if any of the three required headers
        /// (`X-RateLimit-Limit`, `X-RateLimit-Remaining`, `X-RateLimit-Reset`)
        /// are missing or cannot be parsed as `u64`.
        #[must_use]
        pub fn from_headers(headers: &HeaderMap) -> Option<Self> {
            let parse = |name| -> Option<u64> {
                headers
                    .get(name)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse().ok())
            };

            let limit = parse(HEADER_LIMIT)?;
            let remaining = parse(HEADER_REMAINING)?;
            let reset = parse(HEADER_RESET)?;
            let retry_after = parse(HEADER_RETRY_AFTER);

            Some(Self {
                limit,
                remaining,
                reset,
                retry_after,
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Construction
    // -----------------------------------------------------------------------

    #[test]
    fn new_sets_fields() {
        let info = RateLimitInfo::new(100, 42, 1_700_000_000);
        assert_eq!(info.limit, 100);
        assert_eq!(info.remaining, 42);
        assert_eq!(info.reset, 1_700_000_000);
        assert!(info.retry_after.is_none());
    }

    #[test]
    fn retry_after_builder() {
        let info = RateLimitInfo::new(100, 0, 1_700_000_000).retry_after(60);
        assert_eq!(info.retry_after, Some(60));
    }

    // -----------------------------------------------------------------------
    // is_exceeded
    // -----------------------------------------------------------------------

    #[test]
    fn is_exceeded_when_remaining_zero() {
        let info = RateLimitInfo::new(100, 0, 1_700_000_000);
        assert!(info.is_exceeded());
    }

    #[test]
    fn is_not_exceeded_when_remaining_nonzero() {
        let info = RateLimitInfo::new(100, 1, 1_700_000_000);
        assert!(!info.is_exceeded());
    }

    // -----------------------------------------------------------------------
    // Serde round-trips
    // -----------------------------------------------------------------------

    #[cfg(feature = "serde")]
    #[test]
    fn serde_round_trip_without_retry_after() {
        let info = RateLimitInfo::new(100, 50, 1_700_000_000);
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json["limit"], 100);
        assert_eq!(json["remaining"], 50);
        assert_eq!(json["reset"], 1_700_000_000_u64);
        assert!(json.get("retry_after").is_none());
        let back: RateLimitInfo = serde_json::from_value(json).unwrap();
        assert_eq!(back, info);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_round_trip_with_retry_after() {
        let info = RateLimitInfo::new(100, 0, 1_700_000_000).retry_after(30);
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json["retry_after"], 30);
        let back: RateLimitInfo = serde_json::from_value(json).unwrap();
        assert_eq!(back, info);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_omits_retry_after_when_none() {
        let info = RateLimitInfo::new(10, 5, 999);
        let json = serde_json::to_value(&info).unwrap();
        assert!(json.get("retry_after").is_none());
    }

    // -----------------------------------------------------------------------
    // Axum header injection / extraction
    // -----------------------------------------------------------------------

    #[cfg(feature = "http")]
    mod http_tests {
        use super::*;
        use http::HeaderMap;

        #[test]
        fn inject_and_extract_without_retry_after() {
            let info = RateLimitInfo::new(200, 150, 1_700_000_000);
            let mut headers = HeaderMap::new();
            info.inject_headers(&mut headers);

            let extracted = RateLimitInfo::from_headers(&headers).unwrap();
            assert_eq!(extracted.limit, 200);
            assert_eq!(extracted.remaining, 150);
            assert_eq!(extracted.reset, 1_700_000_000);
            assert!(extracted.retry_after.is_none());
        }

        #[test]
        fn inject_and_extract_with_retry_after() {
            let info = RateLimitInfo::new(100, 0, 1_700_000_000).retry_after(45);
            let mut headers = HeaderMap::new();
            info.inject_headers(&mut headers);

            let extracted = RateLimitInfo::from_headers(&headers).unwrap();
            assert_eq!(extracted.retry_after, Some(45));
        }

        #[test]
        fn from_headers_returns_none_on_missing_required_header() {
            let headers = HeaderMap::new();
            assert!(RateLimitInfo::from_headers(&headers).is_none());
        }

        #[test]
        fn from_headers_returns_none_on_invalid_value() {
            let mut headers = HeaderMap::new();
            headers.insert("x-ratelimit-limit", "not-a-number".parse().unwrap());
            headers.insert("x-ratelimit-remaining", "5".parse().unwrap());
            headers.insert("x-ratelimit-reset", "999".parse().unwrap());
            assert!(RateLimitInfo::from_headers(&headers).is_none());
        }
    }
}
