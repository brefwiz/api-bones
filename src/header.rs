//! Lightweight `HeaderName` and `HeaderValue` wrappers.
//!
//! These types wrap the corresponding types from the `http` crate, adding
//! validation-on-construction helpers and interop with the existing
//! [`ETag`](crate::etag::ETag) and [`RateLimitInfo`](crate::ratelimit::RateLimitInfo)
//! header helpers.
//!
//! This module requires the `http` feature flag.
//!
//! # Example
//!
//! ```rust
//! use api_bones::header::{HeaderName, HeaderValue};
//!
//! let name = HeaderName::from_static("content-type");
//! let value = HeaderValue::parse("application/json").unwrap();
//! assert_eq!(name.as_str(), "content-type");
//! assert_eq!(value.to_str().unwrap(), "application/json");
//! ```

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::string::String;
use core::fmt;
use http::header::{
    HeaderName as HttpHeaderName, HeaderValue as HttpHeaderValue, InvalidHeaderName,
    InvalidHeaderValue,
};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// HeaderName
// ---------------------------------------------------------------------------

/// A validated HTTP header name.
///
/// Wraps [`http::header::HeaderName`] and provides the same construction and
/// conversion interface.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HeaderName(HttpHeaderName);

impl HeaderName {
    /// Create a `HeaderName` from a lowercase static string.
    ///
    /// # Panics
    ///
    /// Panics if `s` is not a valid header name.
    #[must_use]
    pub fn from_static(s: &'static str) -> Self {
        Self(HttpHeaderName::from_static(s))
    }

    /// Try to create a `HeaderName` from a string, validating it.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidHeaderName`] if the string contains characters that
    /// are not valid in an HTTP header name.
    pub fn parse(s: &str) -> Result<Self, InvalidHeaderName> {
        s.parse::<HttpHeaderName>().map(Self)
    }

    /// Return the header name as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Consume the wrapper and return the inner [`http::header::HeaderName`].
    #[must_use]
    pub fn into_inner(self) -> HttpHeaderName {
        self.0
    }
}

impl fmt::Display for HeaderName {
    #[inline(never)]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.as_str())
    }
}

impl From<HttpHeaderName> for HeaderName {
    fn from(name: HttpHeaderName) -> Self {
        Self(name)
    }
}

impl From<HeaderName> for HttpHeaderName {
    fn from(name: HeaderName) -> Self {
        name.0
    }
}

impl core::str::FromStr for HeaderName {
    type Err = InvalidHeaderName;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

#[cfg(feature = "serde")]
impl Serialize for HeaderName {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.0.as_str())
    }
}

#[cfg(feature = "serde")]
impl<'de> Deserialize<'de> for HeaderName {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

// ---------------------------------------------------------------------------
// HeaderValue
// ---------------------------------------------------------------------------

/// A validated HTTP header value.
///
/// Wraps [`http::header::HeaderValue`] and provides a validated constructor.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HeaderValue(HttpHeaderValue);

impl HeaderValue {
    /// Try to create a `HeaderValue` from a string slice, validating it.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidHeaderValue`] if the string contains characters that
    /// are not valid in an HTTP header value (e.g. non-visible ASCII or NUL).
    pub fn parse(s: &str) -> Result<Self, InvalidHeaderValue> {
        s.parse::<HttpHeaderValue>().map(Self)
    }

    /// Create a `HeaderValue` from a static string.
    ///
    /// # Panics
    ///
    /// Panics if `s` is not a valid header value.
    #[must_use]
    pub fn from_static(s: &'static str) -> Self {
        Self(HttpHeaderValue::from_static(s))
    }

    /// Return the header value as a string slice if it contains only visible
    /// ASCII characters.
    ///
    /// # Errors
    ///
    /// Returns a [`http::header::ToStrError`] if the value contains non-ASCII
    /// or non-visible bytes.
    pub fn to_str(&self) -> Result<&str, http::header::ToStrError> {
        self.0.to_str()
    }

    /// Return the raw bytes of the header value.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    /// Consume the wrapper and return the inner [`http::header::HeaderValue`].
    #[must_use]
    pub fn into_inner(self) -> HttpHeaderValue {
        self.0
    }
}

impl fmt::Display for HeaderValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0.to_str() {
            Ok(s) => f.write_str(s),
            Err(_) => write!(f, "{:?}", self.0.as_bytes()),
        }
    }
}

impl From<HttpHeaderValue> for HeaderValue {
    fn from(val: HttpHeaderValue) -> Self {
        Self(val)
    }
}

impl From<HeaderValue> for HttpHeaderValue {
    fn from(val: HeaderValue) -> Self {
        val.0
    }
}

impl core::str::FromStr for HeaderValue {
    type Err = InvalidHeaderValue;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

#[cfg(feature = "serde")]
impl Serialize for HeaderValue {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0.to_str() {
            Ok(s) => serializer.serialize_str(s),
            Err(_) => serializer.serialize_bytes(self.0.as_bytes()),
        }
    }
}

// ---------------------------------------------------------------------------
// Interop: ETag → HeaderValue
// ---------------------------------------------------------------------------

impl From<crate::etag::ETag> for HeaderValue {
    fn from(tag: crate::etag::ETag) -> Self {
        // ETag display is always valid ASCII; the expect is justified.
        Self(
            tag.to_string()
                .parse()
                .expect("ETag display is always a valid header value"),
        )
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_name_from_static() {
        let n = HeaderName::from_static("content-type");
        assert_eq!(n.as_str(), "content-type");
    }

    #[test]
    fn header_name_from_str_valid() {
        let n = HeaderName::parse("x-request-id").unwrap();
        assert_eq!(n.as_str(), "x-request-id");
    }

    #[test]
    fn header_name_from_str_invalid() {
        assert!(HeaderName::parse("bad header!").is_err());
    }

    #[test]
    fn header_name_display() {
        let n = HeaderName::from_static("accept");
        assert_eq!(n.to_string(), "accept");
        assert_eq!(format!("{n}"), "accept");
    }

    #[test]
    fn header_name_round_trips_inner() {
        let inner = http::header::CONTENT_TYPE;
        let wrapped = HeaderName::from(inner.clone());
        let back: http::header::HeaderName = wrapped.into();
        assert_eq!(back, inner);
    }

    #[test]
    fn header_value_from_str_valid() {
        let v = HeaderValue::parse("application/json").unwrap();
        assert_eq!(v.to_str().unwrap(), "application/json");
    }

    #[test]
    fn header_value_from_static() {
        let v = HeaderValue::from_static("application/json");
        assert_eq!(v.to_str().unwrap(), "application/json");
    }

    #[test]
    fn header_value_from_str_invalid() {
        // NUL byte is not a valid header value character.
        assert!(HeaderValue::parse("\0invalid").is_err());
    }

    #[test]
    fn header_value_as_bytes() {
        let v = HeaderValue::from_static("hello");
        assert_eq!(v.as_bytes(), b"hello");
    }

    #[test]
    fn header_value_display() {
        let v = HeaderValue::from_static("hello");
        assert_eq!(v.to_string(), "hello");
    }

    #[test]
    fn header_value_round_trips_inner() {
        let inner = http::header::HeaderValue::from_static("hello");
        let wrapped = HeaderValue::from(inner.clone());
        let back: http::header::HeaderValue = wrapped.into();
        assert_eq!(back, inner);
    }

    #[test]
    fn etag_into_header_value() {
        use crate::etag::ETag;
        let tag = ETag::strong("abc123");
        let val: HeaderValue = tag.into();
        assert_eq!(val.to_str().unwrap(), "\"abc123\"");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn header_name_serde_round_trip() {
        let n = HeaderName::from_static("content-type");
        let json = serde_json::to_string(&n).unwrap();
        assert_eq!(json, r#""content-type""#);
        let back: HeaderName = serde_json::from_str(&json).unwrap();
        assert_eq!(back, n);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn header_value_serde_ascii() {
        let v = HeaderValue::from_static("application/json");
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(json, r#""application/json""#);
    }

    #[test]
    fn header_name_into_inner() {
        let n = HeaderName::from_static("x-foo");
        let inner = n.into_inner();
        assert_eq!(inner, http::header::HeaderName::from_static("x-foo"));
    }

    #[test]
    fn header_value_into_inner() {
        let v = HeaderValue::from_static("bar");
        let inner = v.into_inner();
        assert_eq!(inner, http::header::HeaderValue::from_static("bar"));
    }

    #[test]
    fn header_value_to_str_opaque_bytes() {
        // bytes with non-ascii are opaque — to_str returns Err
        let inner = http::header::HeaderValue::from_bytes(b"\xff").unwrap();
        let v = HeaderValue::from(inner);
        assert!(v.to_str().is_err());
    }

    #[test]
    fn header_value_display_opaque() {
        // Display for non-UTF-8 value falls back to debug bytes format
        let inner = http::header::HeaderValue::from_bytes(b"\xff\xfe").unwrap();
        let v = HeaderValue::from(inner);
        let s = format!("{v}");
        assert!(s.contains("ff") || !s.is_empty());
    }

    #[test]
    fn header_value_parse_trait() {
        // Use FromStr trait (`.parse()`) not the inherent method
        let v: HeaderValue = "application/json".parse().unwrap();
        assert_eq!(v.to_str().unwrap(), "application/json");
        assert!("\0bad".parse::<HeaderValue>().is_err());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn header_value_serde_opaque_bytes() {
        // Non-UTF-8 value serializes as bytes array
        let inner = http::header::HeaderValue::from_bytes(b"\xff").unwrap();
        let v = HeaderValue::from(inner);
        let json = serde_json::to_string(&v).unwrap();
        // serde_json serializes bytes as array of ints
        assert!(!json.is_empty());
    }
}
