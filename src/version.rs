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
use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use core::fmt;
#[cfg(any(feature = "std", feature = "alloc"))]
use core::str::FromStr;
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
#[cfg_attr(feature = "serde", derive(Serialize))]
#[cfg_attr(feature = "serde", serde(into = "String"))]
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

#[cfg(any(feature = "std", feature = "alloc"))]
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
// Serde: string-based serialization/deserialization
// ---------------------------------------------------------------------------

// `serde(into = "String")` requires `Into<String>`.
#[cfg(any(feature = "std", feature = "alloc"))]
impl From<ApiVersion> for String {
    fn from(v: ApiVersion) -> Self {
        v.to_string()
    }
}

#[cfg(feature = "serde")]
#[cfg(any(feature = "std", feature = "alloc"))]
impl<'de> Deserialize<'de> for ApiVersion {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse::<Self>().map_err(serde::de::Error::custom)
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
// Axum extractor
// ---------------------------------------------------------------------------

/// Extracts the API version from the `X-Api-Version` header or the `v` query
/// parameter (header takes precedence). Parses the raw string through
/// [`ApiVersion::from_str`]; rejects with `400 Bad Request` when neither
/// source is present or the value is not a recognised version format.
#[cfg(all(feature = "axum", any(feature = "std", feature = "alloc")))]
impl<S: Send + Sync> axum::extract::FromRequestParts<S> for ApiVersion {
    type Rejection = crate::error::ApiError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        // 1. Try X-Api-Version header
        if let Some(val) = parts.headers.get("x-api-version") {
            let s = val.to_str().map_err(|_| {
                crate::error::ApiError::bad_request("header x-api-version contains non-UTF-8 bytes")
            })?;
            return s.parse::<Self>().map_err(|e| {
                crate::error::ApiError::bad_request(format!("invalid X-Api-Version: {e}"))
            });
        }
        // 2. Try query parameter `v`
        if let Some(query) = parts.uri.query() {
            for pair in query.split('&') {
                if let Some(v) = pair.strip_prefix("v=") {
                    return v.parse::<Self>().map_err(|e| {
                        crate::error::ApiError::bad_request(format!("invalid v= query param: {e}"))
                    });
                }
            }
        }
        Err(crate::error::ApiError::bad_request(
            "missing api version: provide X-Api-Version header or v= query parameter",
        ))
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
        // string-based serde: serialises as "v3"
        let s = serde_json::to_value(&v).unwrap();
        assert_eq!(s, serde_json::json!("v3"));
        let back: ApiVersion = serde_json::from_value(s).unwrap();
        assert_eq!(back, v);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_round_trip_semver() {
        let v = ApiVersion::Semver(SemverTriple(1, 2, 3));
        // string-based serde: serialises as "1.2.3"
        let s = serde_json::to_value(&v).unwrap();
        assert_eq!(s, serde_json::json!("1.2.3"));
        let back: ApiVersion = serde_json::from_value(s).unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn semver_triple_display() {
        let t = SemverTriple(2, 10, 0);
        assert_eq!(t.to_string(), "2.10.0");
    }

    #[test]
    fn api_version_parse_error_display() {
        let err = ApiVersionParseError("bad".into());
        let s = err.to_string();
        assert!(s.contains("invalid API version"));
        assert!(s.contains("bad"));
    }

    #[test]
    fn ordering_cross_variant() {
        // Simple < Semver < Date by discriminant ordering
        let simple: ApiVersion = "v1".parse().unwrap();
        let semver: ApiVersion = "1.0.0".parse().unwrap();
        let date: ApiVersion = "2024-01-01".parse().unwrap();
        assert!(simple < semver);
        assert!(semver < date);
    }

    #[cfg(any(feature = "std", feature = "alloc"))]
    #[test]
    fn header_value_simple() {
        let v = ApiVersion::Simple(5);
        assert_eq!(v.header_value(), "v5");
    }

    #[cfg(any(feature = "std", feature = "alloc"))]
    #[test]
    fn header_value_semver() {
        let v = ApiVersion::Semver(SemverTriple(1, 2, 3));
        assert_eq!(v.header_value(), "1.2.3");
    }

    #[test]
    fn parse_date_invalid_day_zero() {
        assert!("2024-01-00".parse::<ApiVersion>().is_err());
    }

    #[test]
    fn parse_date_invalid_month_zero() {
        assert!("2024-00-01".parse::<ApiVersion>().is_err());
    }

    #[test]
    fn parse_semver_bad_component() {
        assert!("1.x.3".parse::<ApiVersion>().is_err());
    }

    #[test]
    fn parse_simple_bad_number() {
        assert!("vabc".parse::<ApiVersion>().is_err());
    }

    #[test]
    fn display_date_pads_correctly() {
        let v = ApiVersion::Date(2024, 1, 5);
        assert_eq!(v.to_string(), "2024-01-05");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_round_trip_date() {
        let v = ApiVersion::Date(2024, 6, 1);
        // string-based serde: serialises as "2024-06-01"
        let json = serde_json::to_value(&v).unwrap();
        assert_eq!(json, serde_json::json!("2024-06-01"));
        let back: ApiVersion = serde_json::from_value(json).unwrap();
        assert_eq!(back, v);
    }

    #[cfg(feature = "http")]
    #[test]
    fn inject_content_version_header() {
        let v = ApiVersion::Simple(3);
        let mut headers = http::HeaderMap::new();
        v.inject_content_version(&mut headers).unwrap();
        assert_eq!(headers["content-version"], "v3");
    }

    #[cfg(feature = "std")]
    #[test]
    fn api_version_parse_error_is_std_error() {
        // Exercises the `std::error::Error` impl (source returns None by default).
        let err = ApiVersionParseError("oops".into());
        let boxed: Box<dyn std::error::Error> = Box::new(err);
        assert!(boxed.source().is_none());
    }

    #[test]
    fn semver_triple_ordering() {
        let a = SemverTriple(1, 0, 0);
        let b = SemverTriple(1, 1, 0);
        let c = SemverTriple(2, 0, 0);
        assert!(a < b);
        assert!(b < c);
        assert!(a < c);
        assert_eq!(a, SemverTriple(1, 0, 0));
    }

    #[test]
    fn api_version_parse_error_clone_and_eq() {
        let err = ApiVersionParseError("bad-version".into());
        let cloned = err.clone();
        assert_eq!(err, cloned);
    }

    #[test]
    fn parse_date_invalid_year_non_numeric() {
        // "abcd-01-01" matches the date pattern (len=10, dashes at 4/7) but
        // year parse fails → exercises that error branch in from_str.
        assert!("abcd-01-01".parse::<ApiVersion>().is_err());
    }

    #[test]
    fn parse_date_invalid_day_non_numeric() {
        // "2024-01-xx" — day parse fails.
        assert!("2024-01-xx".parse::<ApiVersion>().is_err());
    }

    #[test]
    fn parse_date_invalid_month_non_numeric() {
        // month parse fails → exercises that map_err closure in from_str
        assert!("2024-xx-01".parse::<ApiVersion>().is_err());
    }

    #[test]
    fn parse_semver_bad_major() {
        // major component non-numeric → exercises that map_err closure
        assert!("x.1.3".parse::<ApiVersion>().is_err());
    }

    #[test]
    fn parse_semver_bad_patch() {
        // patch component non-numeric → exercises that map_err closure
        assert!("1.2.x".parse::<ApiVersion>().is_err());
    }

    #[test]
    fn parse_semver_too_many_parts() {
        // "1.2.3.4" splits into 4 parts with splitn(4,'.')  → parts.len()==4, not 3
        // → falls through to the final Err.
        assert!("1.2.3.4".parse::<ApiVersion>().is_err());
    }

    #[test]
    fn hash_semver_triple() {
        use core::hash::{Hash, Hasher};
        use std::collections::hash_map::DefaultHasher;
        let mut h1 = DefaultHasher::new();
        let mut h2 = DefaultHasher::new();
        SemverTriple(1, 2, 3).hash(&mut h1);
        SemverTriple(1, 2, 3).hash(&mut h2);
        assert_eq!(h1.finish(), h2.finish());
    }

    #[test]
    fn hash_api_version() {
        use core::hash::{Hash, Hasher};
        use std::collections::hash_map::DefaultHasher;
        let mut h = DefaultHasher::new();
        ApiVersion::Simple(1).hash(&mut h);
        let _ = h.finish();
    }

    #[test]
    fn api_version_in_hashset() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(ApiVersion::Simple(1));
        set.insert(ApiVersion::Semver(SemverTriple(1, 0, 0)));
        set.insert(ApiVersion::Date(2024, 1, 1));
        assert_eq!(set.len(), 3);
        assert!(set.contains(&ApiVersion::Simple(1)));
    }

    #[test]
    fn semver_triple_in_hashset() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(SemverTriple(1, 2, 3));
        assert!(set.contains(&SemverTriple(1, 2, 3)));
    }

    #[test]
    fn api_version_parse_error_in_hashset() {
        // ApiVersionParseError derives PartialEq+Eq but not Hash — just clone+eq coverage
        let e1 = ApiVersionParseError("x".into());
        let e2 = e1.clone();
        assert_eq!(e1, e2);
        assert_ne!(e1, ApiVersionParseError("y".into()));
    }

    #[test]
    fn semver_triple_ord_cmp() {
        use core::cmp::Ordering;
        let a = SemverTriple(1, 0, 0);
        let b = SemverTriple(2, 0, 0);
        assert_eq!(a.cmp(&b), Ordering::Less);
        assert_eq!(b.cmp(&a), Ordering::Greater);
        assert_eq!(a.cmp(&a), Ordering::Equal);
    }

    #[test]
    fn api_version_ord_cmp() {
        use core::cmp::Ordering;
        let a = ApiVersion::Simple(1);
        let b = ApiVersion::Simple(2);
        assert_eq!(a.cmp(&b), Ordering::Less);
        assert_eq!(b.cmp(&a), Ordering::Greater);
        assert_eq!(a.cmp(&a), Ordering::Equal);
    }

    #[test]
    fn api_version_parse_error_eq() {
        use core::cmp::PartialEq;
        let e1 = ApiVersionParseError("a".into());
        let e2 = ApiVersionParseError("a".into());
        let e3 = ApiVersionParseError("b".into());
        assert!(e1.eq(&e2));
        assert!(!e1.eq(&e3));
    }

    #[test]
    fn api_version_clone_all_variants() {
        let simple = ApiVersion::Simple(1);
        let semver = ApiVersion::Semver(SemverTriple(1, 2, 3));
        let date = ApiVersion::Date(2024, 6, 1);
        assert_eq!(simple.clone(), simple);
        assert_eq!(semver.clone(), semver);
        assert_eq!(date.clone(), date);
    }

    #[test]
    fn semver_triple_clone_and_copy() {
        let t = SemverTriple(1, 2, 3);
        let cloned = t; // Copy
        assert_eq!(t, cloned);
        assert_eq!(t.clone(), cloned);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_round_trip_semver_triple() {
        let t = SemverTriple(3, 14, 159);
        let s = serde_json::to_value(t).unwrap();
        let back: SemverTriple = serde_json::from_value(s).unwrap();
        assert_eq!(back, t);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_parse_error_round_trip() {
        let e = ApiVersionParseError("bad".into());
        let s = serde_json::to_value(&e).unwrap();
        let back: ApiVersionParseError = serde_json::from_value(s).unwrap();
        assert_eq!(back, e);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_api_version_invalid_string_produces_error() {
        // String that cannot be parsed as any ApiVersion variant.
        let result: Result<ApiVersion, _> =
            serde_json::from_value(serde_json::json!("not-a-version"));
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(!msg.is_empty());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_api_version_non_string_is_invalid() {
        // Deserializer now expects a string, so integer/boolean/null fail.
        let result: Result<ApiVersion, _> = serde_json::from_value(serde_json::json!(true));
        assert!(result.is_err());
        let result2: Result<ApiVersion, _> = serde_json::from_value(serde_json::json!(42));
        assert!(result2.is_err());
    }

    #[test]
    fn display_fmt_via_format_macro() {
        // Exercise Display::fmt for all three types through format! to ensure
        // the fmt function is reachable via the format path (not just to_string).
        let s = format!("{}", SemverTriple(1, 0, 0));
        assert_eq!(s, "1.0.0");
        let v = format!("{}", ApiVersion::Simple(7));
        assert_eq!(v, "v7");
        let e = format!("{}", ApiVersionParseError("x".into()));
        assert!(e.contains("invalid API version"));
    }

    #[test]
    fn display_fmt_direct_write() {
        use core::fmt::Write;
        let mut buf = String::new();
        write!(buf, "{}", SemverTriple(2, 3, 4)).unwrap();
        assert_eq!(buf, "2.3.4");
        buf.clear();
        write!(buf, "{}", ApiVersion::Semver(SemverTriple(0, 1, 0))).unwrap();
        assert_eq!(buf, "0.1.0");
        buf.clear();
        write!(buf, "{}", ApiVersionParseError("z".into())).unwrap();
        assert!(buf.contains('z'));
    }
}
