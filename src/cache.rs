//! `Cache-Control` header builder and parser (RFC 7234).
//!
//! [`CacheControl`] represents the structured set of directives that can
//! appear in a `Cache-Control` HTTP header, with builder methods for the most
//! common request and response directives.
//!
//! # Example
//!
//! ```rust
//! use api_bones::cache::CacheControl;
//!
//! // Build a typical immutable public response.
//! let cc = CacheControl::new()
//!     .public()
//!     .max_age(31_536_000)
//!     .immutable();
//! assert_eq!(cc.to_string(), "public, immutable, max-age=31536000");
//!
//! // Parse a header value.
//! let cc: CacheControl = "no-store, no-cache".parse().unwrap();
//! assert!(cc.no_store);
//! assert!(cc.no_cache);
//! ```

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use core::{fmt, str::FromStr};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// CacheControl
// ---------------------------------------------------------------------------

/// Structured `Cache-Control` header (RFC 7234 §5.2).
///
/// All boolean directives default to `false`; numeric directives default to
/// `None` (absent). Use the builder methods to set them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct CacheControl {
    // -----------------------------------------------------------------------
    // Response directives
    // -----------------------------------------------------------------------
    /// `public` — response may be stored by any cache.
    pub public: bool,
    /// `private` — response is intended for a single user; must not be stored
    /// by a shared cache.
    pub private: bool,
    /// `no-cache` — cache must revalidate with the origin before serving.
    pub no_cache: bool,
    /// `no-store` — must not store any part of the request or response.
    pub no_store: bool,
    /// `no-transform` — no transformations or conversions should be made.
    pub no_transform: bool,
    /// `must-revalidate` — stale responses must not be used without revalidation.
    pub must_revalidate: bool,
    /// `proxy-revalidate` — like `must-revalidate` but only for shared caches.
    pub proxy_revalidate: bool,
    /// `immutable` — response body will not change over its lifetime.
    pub immutable: bool,
    /// `max-age=<seconds>` — maximum time the response is considered fresh.
    pub max_age: Option<u64>,
    /// `s-maxage=<seconds>` — overrides `max-age` for shared caches.
    pub s_maxage: Option<u64>,
    /// `stale-while-revalidate=<seconds>` — serve stale while revalidating.
    pub stale_while_revalidate: Option<u64>,
    /// `stale-if-error=<seconds>` — use stale response on error.
    pub stale_if_error: Option<u64>,

    // -----------------------------------------------------------------------
    // Request directives
    // -----------------------------------------------------------------------
    /// `only-if-cached` — do not use the network; only return a cached response.
    pub only_if_cached: bool,
    /// `max-stale[=<seconds>]` — accept a response up to this many seconds stale.
    /// `Some(0)` means any staleness is acceptable; `None` means the directive
    /// is absent.
    pub max_stale: Option<u64>,
    /// `min-fresh=<seconds>` — require at least this much remaining freshness.
    pub min_fresh: Option<u64>,
}

impl CacheControl {
    /// Create an empty `CacheControl` with all directives absent.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    // -----------------------------------------------------------------------
    // Builder methods — response directives
    // -----------------------------------------------------------------------

    /// Set the `public` directive.
    #[must_use]
    pub fn public(mut self) -> Self {
        self.public = true;
        self
    }

    /// Set the `private` directive.
    #[must_use]
    pub fn private(mut self) -> Self {
        self.private = true;
        self
    }

    /// Set the `no-cache` directive.
    #[must_use]
    pub fn no_cache(mut self) -> Self {
        self.no_cache = true;
        self
    }

    /// Set the `no-store` directive.
    #[must_use]
    pub fn no_store(mut self) -> Self {
        self.no_store = true;
        self
    }

    /// Set the `no-transform` directive.
    #[must_use]
    pub fn no_transform(mut self) -> Self {
        self.no_transform = true;
        self
    }

    /// Set the `must-revalidate` directive.
    #[must_use]
    pub fn must_revalidate(mut self) -> Self {
        self.must_revalidate = true;
        self
    }

    /// Set the `proxy-revalidate` directive.
    #[must_use]
    pub fn proxy_revalidate(mut self) -> Self {
        self.proxy_revalidate = true;
        self
    }

    /// Set the `immutable` directive.
    #[must_use]
    pub fn immutable(mut self) -> Self {
        self.immutable = true;
        self
    }

    /// Set `max-age=<seconds>`.
    #[must_use]
    pub fn max_age(mut self, seconds: u64) -> Self {
        self.max_age = Some(seconds);
        self
    }

    /// Set `s-maxage=<seconds>`.
    #[must_use]
    pub fn s_maxage(mut self, seconds: u64) -> Self {
        self.s_maxage = Some(seconds);
        self
    }

    /// Set `stale-while-revalidate=<seconds>`.
    #[must_use]
    pub fn stale_while_revalidate(mut self, seconds: u64) -> Self {
        self.stale_while_revalidate = Some(seconds);
        self
    }

    /// Set `stale-if-error=<seconds>`.
    #[must_use]
    pub fn stale_if_error(mut self, seconds: u64) -> Self {
        self.stale_if_error = Some(seconds);
        self
    }

    // -----------------------------------------------------------------------
    // Builder methods — request directives
    // -----------------------------------------------------------------------

    /// Set the `only-if-cached` directive.
    #[must_use]
    pub fn only_if_cached(mut self) -> Self {
        self.only_if_cached = true;
        self
    }

    /// Set `max-stale[=<seconds>]`.  Pass `0` to accept any staleness.
    #[must_use]
    pub fn max_stale(mut self, seconds: u64) -> Self {
        self.max_stale = Some(seconds);
        self
    }

    /// Set `min-fresh=<seconds>`.
    #[must_use]
    pub fn min_fresh(mut self, seconds: u64) -> Self {
        self.min_fresh = Some(seconds);
        self
    }

    // -----------------------------------------------------------------------
    // Convenience constructors
    // -----------------------------------------------------------------------

    /// Convenience: `no-store` (disable all caching).
    ///
    /// ```
    /// use api_bones::cache::CacheControl;
    ///
    /// let cc = CacheControl::no_caching();
    /// assert!(cc.no_store);
    /// assert_eq!(cc.to_string(), "no-store");
    /// ```
    #[must_use]
    pub fn no_caching() -> Self {
        Self::new().no_store()
    }

    /// Convenience: `private, no-cache, no-store`.
    ///
    /// ```
    /// use api_bones::cache::CacheControl;
    ///
    /// let cc = CacheControl::private_no_cache();
    /// assert!(cc.private && cc.no_cache && cc.no_store);
    /// ```
    #[must_use]
    pub fn private_no_cache() -> Self {
        Self::new().private().no_cache().no_store()
    }
}

// ---------------------------------------------------------------------------
// Display
// ---------------------------------------------------------------------------

impl fmt::Display for CacheControl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts: Vec<&str> = Vec::new();
        // We write variable-width entries inline; use a local buffer.
        // Collect fixed-string directives first, then emit numeric ones.

        // Boolean directives (response)
        if self.public {
            parts.push("public");
        }
        if self.private {
            parts.push("private");
        }
        if self.no_cache {
            parts.push("no-cache");
        }
        if self.no_store {
            parts.push("no-store");
        }
        if self.no_transform {
            parts.push("no-transform");
        }
        if self.must_revalidate {
            parts.push("must-revalidate");
        }
        if self.proxy_revalidate {
            parts.push("proxy-revalidate");
        }
        if self.immutable {
            parts.push("immutable");
        }
        // Boolean directives (request)
        if self.only_if_cached {
            parts.push("only-if-cached");
        }

        // Write fixed parts first
        for (i, p) in parts.iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            f.write_str(p)?;
        }

        // Collect numeric directives.
        let numeric: [Option<(&str, u64)>; 6] = [
            self.max_age.map(|v| ("max-age", v)),
            self.s_maxage.map(|v| ("s-maxage", v)),
            self.stale_while_revalidate
                .map(|v| ("stale-while-revalidate", v)),
            self.stale_if_error.map(|v| ("stale-if-error", v)),
            self.max_stale.map(|v| ("max-stale", v)),
            self.min_fresh.map(|v| ("min-fresh", v)),
        ];

        let mut need_sep = !parts.is_empty();
        for entry in &numeric {
            if let Some((name, v)) = entry {
                if need_sep {
                    f.write_str(", ")?;
                }
                write!(f, "{name}={v}")?;
                need_sep = true;
            }
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Parse error
// ---------------------------------------------------------------------------

/// Error returned when parsing a `Cache-Control` header fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseCacheControlError(String);

impl fmt::Display for ParseCacheControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid Cache-Control header: {}", self.0)
    }
}

#[cfg(feature = "std")]
impl std::error::Error for ParseCacheControlError {}

// ---------------------------------------------------------------------------
// FromStr
// ---------------------------------------------------------------------------

impl FromStr for CacheControl {
    type Err = ParseCacheControlError;

    /// Parse a `Cache-Control` header value.
    ///
    /// Unknown directives are silently ignored, matching real-world HTTP
    /// caching behaviour.
    ///
    /// ```
    /// use api_bones::cache::CacheControl;
    ///
    /// let cc: CacheControl = "public, max-age=3600, must-revalidate".parse().unwrap();
    /// assert!(cc.public);
    /// assert_eq!(cc.max_age, Some(3600));
    /// assert!(cc.must_revalidate);
    /// ```
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut cc = Self::new();
        for token in s.split(',') {
            let token = token.trim();
            if token.is_empty() {
                continue;
            }
            let (name, value) = if let Some(eq) = token.find('=') {
                (&token[..eq], Some(token[eq + 1..].trim()))
            } else {
                (token, None)
            };
            let name = name.trim().to_lowercase();

            let parse_u64 = |v: Option<&str>| -> Result<u64, ParseCacheControlError> {
                v.ok_or_else(|| ParseCacheControlError(format!("{name} requires a value")))?
                    .parse::<u64>()
                    .map_err(|_| {
                        ParseCacheControlError(format!("{name} value is not a valid integer"))
                    })
            };

            match name.as_str() {
                "public" => cc.public = true,
                "private" => cc.private = true,
                "no-cache" => cc.no_cache = true,
                "no-store" => cc.no_store = true,
                "no-transform" => cc.no_transform = true,
                "must-revalidate" => cc.must_revalidate = true,
                "proxy-revalidate" => cc.proxy_revalidate = true,
                "immutable" => cc.immutable = true,
                "only-if-cached" => cc.only_if_cached = true,
                "max-age" => cc.max_age = Some(parse_u64(value)?),
                "s-maxage" => cc.s_maxage = Some(parse_u64(value)?),
                "stale-while-revalidate" => cc.stale_while_revalidate = Some(parse_u64(value)?),
                "stale-if-error" => cc.stale_if_error = Some(parse_u64(value)?),
                "max-stale" => cc.max_stale = Some(value.and_then(|v| v.parse().ok()).unwrap_or(0)),
                "min-fresh" => cc.min_fresh = Some(parse_u64(value)?),
                _ => {} // unknown directives are ignored per RFC
            }
        }
        Ok(cc)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_empty() {
        let cc = CacheControl::new();
        assert_eq!(cc.to_string(), "");
    }

    #[test]
    fn builder_public_max_age_immutable() {
        let cc = CacheControl::new().public().max_age(31_536_000).immutable();
        // Boolean directives (public, immutable) appear before numeric ones (max-age).
        assert_eq!(cc.to_string(), "public, immutable, max-age=31536000");
    }

    #[test]
    fn builder_no_store() {
        let cc = CacheControl::no_caching();
        assert_eq!(cc.to_string(), "no-store");
    }

    #[test]
    fn builder_private_no_cache() {
        let cc = CacheControl::private_no_cache();
        assert!(cc.private);
        assert!(cc.no_cache);
        assert!(cc.no_store);
    }

    #[test]
    fn parse_simple_flags() {
        let cc: CacheControl = "no-store, no-cache".parse().unwrap();
        assert!(cc.no_store);
        assert!(cc.no_cache);
    }

    #[test]
    fn parse_numeric_directives() {
        let cc: CacheControl = "public, max-age=3600, s-maxage=7200".parse().unwrap();
        assert!(cc.public);
        assert_eq!(cc.max_age, Some(3600));
        assert_eq!(cc.s_maxage, Some(7200));
    }

    #[test]
    fn parse_unknown_directive_ignored() {
        let cc: CacheControl = "no-store, x-custom-thing=42".parse().unwrap();
        assert!(cc.no_store);
    }

    #[test]
    fn roundtrip_complex() {
        let original = CacheControl::new()
            .public()
            .max_age(600)
            .must_revalidate()
            .stale_if_error(86_400);
        let s = original.to_string();
        let parsed: CacheControl = s.parse().unwrap();
        assert_eq!(parsed.public, original.public);
        assert_eq!(parsed.max_age, original.max_age);
        assert_eq!(parsed.must_revalidate, original.must_revalidate);
        assert_eq!(parsed.stale_if_error, original.stale_if_error);
    }

    #[test]
    fn parse_case_insensitive() {
        let cc: CacheControl = "No-Store, Max-Age=60".parse().unwrap();
        assert!(cc.no_store);
        assert_eq!(cc.max_age, Some(60));
    }

    #[test]
    fn parse_max_stale_no_value() {
        let cc: CacheControl = "max-stale".parse().unwrap();
        assert_eq!(cc.max_stale, Some(0));
    }

    #[test]
    fn parse_max_stale_with_value() {
        let cc: CacheControl = "max-stale=300".parse().unwrap();
        assert_eq!(cc.max_stale, Some(300));
    }
}
