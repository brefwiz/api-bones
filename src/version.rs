//! API versioning types.
//!
//! [`ApiVersion`] supports three versioning schemes used in practice:
//! - Simple integer (`v1`, `v2`, …) — internal/private APIs
//! - Semver (`1.2.3`) — SDK-coupled APIs
//! - Date-based (`2024-06-01`) — Stripe/Cloudflare style public APIs
//!
//! # Example
//!
//! ```rust
//! use api_bones::version::ApiVersion;
//! use core::str::FromStr;
//!
//! let v: ApiVersion = "v3".parse().unwrap();
//! assert_eq!(v.to_string(), "v3");
//!
//! let v: ApiVersion = "1.2.3".parse().unwrap();
//! assert_eq!(v.to_string(), "1.2.3");
//!
//! let v: ApiVersion = "2024-06-01".parse().unwrap();
//! assert_eq!(v.to_string(), "2024-06-01");
//! ```

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::string::{String, ToString};
use core::{fmt, str::FromStr};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// ApiVersion
// ---------------------------------------------------------------------------

/// An API version that supports three common schemes.
///
/// Variants are ordered: `Simple` < `Semver` < `Date` within each variant, and
/// by discriminant across variants (i.e. a `Simple` version is always less than
/// a `Semver` version). Use the `PartialOrd` / `Ord` implementations for
/// "minimum version" guards.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize), serde(untagged))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum ApiVersion {
    /// Integer version: `v1`, `v2`, … (stored as the bare number).
    Simple(u32),
    /// Semantic version: `1.2.3`.
    Semver(SemverTriple),
    /// Date version: `YYYY-MM-DD` (stored as `(year, month, day)`).
    Date(u16, u8, u8),
}

/// Semantic version triple `(major, minor, patch)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SemverTriple(pub u32, pub u32, pub u32);

// ---------------------------------------------------------------------------
// Display
// ---------------------------------------------------------------------------

impl fmt::Display for SemverTriple {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

impl fmt::Display for ApiVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Simple(n) => write!(f, "v{n}"),
            Self::Semver(t) => write!(f, "{t}"),
            Self::Date(y, m, d) => write!(f, "{y:04}-{m:02}-{d:02}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Error returned when an [`ApiVersion`] string cannot be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ApiVersionParseError(
    #[cfg(any(feature = "std", feature = "alloc"))] pub String,
    #[cfg(not(any(feature = "std", feature = "alloc")))] pub &'static str,
);

impl fmt::Display for ApiVersionParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid API version: {}", self.0)
    }
}

#[cfg(feature = "std")]
impl std::error::Error for ApiVersionParseError {}

impl FromStr for ApiVersion {
    type Err = ApiVersionParseError;

    /// Parse a version string.
    ///
    /// Accepted formats:
    /// - `vN` or `VN` — simple integer (e.g. `v1`)
    /// - `N.N.N` — semver (e.g. `1.2.3`)
    /// - `YYYY-MM-DD` — date (e.g. `2024-06-01`)
    ///
    /// # Errors
    ///
    /// Returns [`ApiVersionParseError`] when `s` does not match any recognised format.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // vN / VN
        if let Some(rest) = s.strip_prefix(['v', 'V']) {
            let n: u32 = rest.parse().map_err(|_| ApiVersionParseError(s.into()))?;
            return Ok(Self::Simple(n));
        }

        // YYYY-MM-DD — exactly 10 chars, dashes at positions 4 and 7
        if s.len() == 10 && s.as_bytes().get(4) == Some(&b'-') && s.as_bytes().get(7) == Some(&b'-')
        {
            let year: u16 = s[..4].parse().map_err(|_| ApiVersionParseError(s.into()))?;
            let month: u8 = s[5..7]
                .parse()
                .map_err(|_| ApiVersionParseError(s.into()))?;
            let day: u8 = s[8..10]
                .parse()
                .map_err(|_| ApiVersionParseError(s.into()))?;
            if (1..=12).contains(&month) && (1..=31).contains(&day) {
                return Ok(Self::Date(year, month, day));
            }
            return Err(ApiVersionParseError(s.into()));
        }

        // N.N.N
        let parts: Vec<&str> = s.splitn(4, '.').collect();
        if parts.len() == 3 {
            let maj: u32 = parts[0]
                .parse()
                .map_err(|_| ApiVersionParseError(s.into()))?;
            let min: u32 = parts[1]
                .parse()
                .map_err(|_| ApiVersionParseError(s.into()))?;
            let pat: u32 = parts[2]
                .parse()
                .map_err(|_| ApiVersionParseError(s.into()))?;
            return Ok(Self::Semver(SemverTriple(maj, min, pat)));
        }

        Err(ApiVersionParseError(s.into()))
    }
}

// ---------------------------------------------------------------------------
// Header helpers
// ---------------------------------------------------------------------------

/// Header name for the requested API version (`Accept-Version`).
pub const ACCEPT_VERSION: &str = "Accept-Version";

/// Header name for the version the response was produced with (`Content-Version`).
pub const CONTENT_VERSION: &str = "Content-Version";

impl ApiVersion {
    /// Return the value suitable for use in an `Accept-Version` or
    /// `Content-Version` HTTP header (the display string).
    ///
    /// # Examples
    ///
    /// ```
    /// use api_bones::version::ApiVersion;
    ///
    /// let v = ApiVersion::Simple(2);
    /// assert_eq!(v.header_value(), "v2");
    /// ```
    #[must_use]
    #[cfg(any(feature = "std", feature = "alloc"))]
    pub fn header_value(&self) -> String {
        self.to_string()
    }

    /// Inject this version into an [`http::HeaderMap`] as `Content-Version`.
    ///
    /// # Errors
    ///
    /// Returns an error if the version string contains characters that are
    /// invalid in HTTP header values.
    #[cfg(feature = "http")]
    pub fn inject_content_version(
        &self,
        headers: &mut http::HeaderMap,
    ) -> Result<(), http::header::InvalidHeaderValue> {
        use http::header::HeaderValue;
        let val = HeaderValue::from_str(&self.to_string())?;
        headers.insert(
            http::header::HeaderName::from_static("content-version"),
            val,
        );
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple() {
        let v: ApiVersion = "v1".parse().unwrap();
        assert_eq!(v, ApiVersion::Simple(1));
        assert_eq!(v.to_string(), "v1");
    }

    #[test]
    fn parse_simple_uppercase() {
        let v: ApiVersion = "V42".parse().unwrap();
        assert_eq!(v, ApiVersion::Simple(42));
    }

    #[test]
    fn parse_semver() {
        let v: ApiVersion = "1.2.3".parse().unwrap();
        assert_eq!(v, ApiVersion::Semver(SemverTriple(1, 2, 3)));
        assert_eq!(v.to_string(), "1.2.3");
    }

    #[test]
    fn parse_date() {
        let v: ApiVersion = "2024-06-01".parse().unwrap();
        assert_eq!(v, ApiVersion::Date(2024, 6, 1));
        assert_eq!(v.to_string(), "2024-06-01");
    }

    #[test]
    fn parse_invalid() {
        assert!("nope".parse::<ApiVersion>().is_err());
        assert!("1.2".parse::<ApiVersion>().is_err());
        assert!("2024-13-01".parse::<ApiVersion>().is_err());
    }

    #[test]
    fn ordering_simple() {
        let v1: ApiVersion = "v1".parse().unwrap();
        let v2: ApiVersion = "v2".parse().unwrap();
        assert!(v1 < v2);
    }

    #[test]
    fn ordering_semver() {
        let a: ApiVersion = "1.0.0".parse().unwrap();
        let b: ApiVersion = "1.0.1".parse().unwrap();
        let c: ApiVersion = "2.0.0".parse().unwrap();
        assert!(a < b);
        assert!(b < c);
    }

    #[test]
    fn ordering_date() {
        let a: ApiVersion = "2024-01-01".parse().unwrap();
        let b: ApiVersion = "2024-06-01".parse().unwrap();
        assert!(a < b);
    }

    #[cfg(any(feature = "std", feature = "alloc"))]
    #[test]
    fn header_value() {
        let v = ApiVersion::Date(2024, 6, 1);
        assert_eq!(v.header_value(), "2024-06-01");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_round_trip_simple() {
        let v = ApiVersion::Simple(3);
        let s = serde_json::to_value(&v).unwrap();
        // untagged serialises Simple(3) as 3
        let back: ApiVersion = serde_json::from_value(s).unwrap();
        assert_eq!(back, v);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_round_trip_semver() {
        let v = ApiVersion::Semver(SemverTriple(1, 2, 3));
        let s = serde_json::to_value(&v).unwrap();
        let back: ApiVersion = serde_json::from_value(s).unwrap();
        assert_eq!(back, v);
    }
}
