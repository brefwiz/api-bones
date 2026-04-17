//! Media type / Content-Type representation.
//!
//! [`ContentType`] models a structured `Content-Type` header value consisting
//! of a `type/subtype` pair and optional parameters (e.g. `charset=utf-8`).
//!
//! Pre-built constants cover the most common media types.
//!
//! # Example
//!
//! ```rust
//! use api_bones::content_type::ContentType;
//!
//! let ct = ContentType::application_json();
//! assert_eq!(ct.to_string(), "application/json");
//!
//! let with_charset = ContentType::text_plain_utf8();
//! assert_eq!(with_charset.to_string(), "text/plain; charset=utf-8");
//!
//! let parsed: ContentType = "application/json".parse().unwrap();
//! assert_eq!(parsed, ContentType::application_json());
//! ```

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{
    borrow::ToOwned,
    string::{String, ToString},
    vec::Vec,
};
use core::{fmt, str::FromStr};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// ContentType
// ---------------------------------------------------------------------------

/// A structured `Content-Type` / media type value.
///
/// Stores the `type/subtype` pair plus an optional list of `name=value`
/// parameters.  The [`Display`](fmt::Display) implementation produces the
/// canonical wire format, e.g. `application/json` or
/// `text/plain; charset=utf-8`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ContentType {
    /// The primary type (e.g. `"application"`).
    pub type_: String,
    /// The subtype (e.g. `"json"`).
    pub subtype: String,
    /// Optional parameters such as `charset` or `boundary`.
    pub params: Vec<(String, String)>,
}

#[cfg(feature = "serde")]
impl Serialize for ContentType {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

#[cfg(feature = "serde")]
impl<'de> Deserialize<'de> for ContentType {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

impl ContentType {
    /// Construct a `ContentType` with no parameters.
    #[must_use]
    pub fn new(type_: impl Into<String>, subtype: impl Into<String>) -> Self {
        Self {
            type_: type_.into(),
            subtype: subtype.into(),
            params: Vec::new(),
        }
    }

    /// Construct a `ContentType` with parameters.
    #[must_use]
    pub fn with_params(
        type_: impl Into<String>,
        subtype: impl Into<String>,
        params: Vec<(String, String)>,
    ) -> Self {
        Self {
            type_: type_.into(),
            subtype: subtype.into(),
            params,
        }
    }

    /// Return the `type/subtype` string without parameters.
    ///
    /// ```
    /// use api_bones::content_type::ContentType;
    ///
    /// let ct = ContentType::text_plain_utf8();
    /// assert_eq!(ct.essence(), "text/plain");
    /// ```
    #[must_use]
    pub fn essence(&self) -> String {
        format!("{}/{}", self.type_, self.subtype)
    }

    /// Return the value of the named parameter, if present.
    ///
    /// ```
    /// use api_bones::content_type::ContentType;
    ///
    /// let ct = ContentType::text_plain_utf8();
    /// assert_eq!(ct.param("charset"), Some("utf-8"));
    /// ```
    #[must_use]
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    // -----------------------------------------------------------------------
    // Pre-built constructors for common media types
    // -----------------------------------------------------------------------

    /// Returns `application/json`.
    #[must_use]
    pub fn application_json() -> Self {
        Self::new("application", "json")
    }

    /// Returns `application/problem+json` (RFC 9457).
    #[must_use]
    pub fn application_problem_json() -> Self {
        Self::new("application", "problem+json")
    }

    /// Returns `application/octet-stream`.
    #[must_use]
    pub fn application_octet_stream() -> Self {
        Self::new("application", "octet-stream")
    }

    /// Returns `multipart/form-data` with the given boundary parameter.
    #[must_use]
    pub fn multipart_form_data(boundary: impl Into<String>) -> Self {
        Self::with_params(
            "multipart",
            "form-data",
            vec![("boundary".to_owned(), boundary.into())],
        )
    }

    /// Returns `text/plain`.
    #[must_use]
    pub fn text_plain() -> Self {
        Self::new("text", "plain")
    }

    /// Returns `text/plain; charset=utf-8`.
    #[must_use]
    pub fn text_plain_utf8() -> Self {
        Self::with_params(
            "text",
            "plain",
            vec![("charset".to_owned(), "utf-8".to_owned())],
        )
    }

    /// Returns `text/html`.
    #[must_use]
    pub fn text_html() -> Self {
        Self::new("text", "html")
    }
}

// ---------------------------------------------------------------------------
// Display
// ---------------------------------------------------------------------------

impl fmt::Display for ContentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.type_, self.subtype)?;
        for (k, v) in &self.params {
            write!(f, "; {k}={v}")?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Error returned when parsing a [`ContentType`] fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseContentTypeError;

impl fmt::Display for ParseContentTypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid Content-Type value")
    }
}

#[cfg(feature = "std")]
impl std::error::Error for ParseContentTypeError {}

impl FromStr for ContentType {
    type Err = ParseContentTypeError;

    /// Parse a `Content-Type` header value.
    ///
    /// Accepts `type/subtype` with an optional `; name=value` parameter list.
    /// Parameter names are lowercased; values are kept as-is.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        let mut parts = s.splitn(2, ';');
        let essence = parts.next().unwrap_or("").trim();
        let mut type_sub = essence.splitn(2, '/');
        let type_ = type_sub.next().unwrap_or("").trim();
        let subtype = type_sub.next().unwrap_or("").trim();
        if type_.is_empty() || subtype.is_empty() {
            return Err(ParseContentTypeError);
        }

        let mut params = Vec::new();
        if let Some(param_str) = parts.next() {
            for param in param_str.split(';') {
                let param = param.trim();
                if param.is_empty() {
                    continue;
                }
                let mut kv = param.splitn(2, '=');
                let k = kv.next().unwrap_or("").trim().to_ascii_lowercase();
                let v = kv.next().unwrap_or("").trim().to_owned();
                if k.is_empty() {
                    return Err(ParseContentTypeError);
                }
                params.push((k, v));
            }
        }

        Ok(Self {
            type_: type_.to_ascii_lowercase(),
            subtype: subtype.to_ascii_lowercase(),
            params,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_no_params() {
        assert_eq!(ContentType::application_json().to_string(), "application/json");
        assert_eq!(
            ContentType::application_problem_json().to_string(),
            "application/problem+json"
        );
        assert_eq!(
            ContentType::application_octet_stream().to_string(),
            "application/octet-stream"
        );
    }

    #[test]
    fn display_with_params() {
        let ct = ContentType::text_plain_utf8();
        assert_eq!(ct.to_string(), "text/plain; charset=utf-8");
    }

    #[test]
    fn display_multipart() {
        let ct = ContentType::multipart_form_data("abc123");
        assert_eq!(ct.to_string(), "multipart/form-data; boundary=abc123");
    }

    #[test]
    fn essence_strips_params() {
        let ct = ContentType::text_plain_utf8();
        assert_eq!(ct.essence(), "text/plain");
    }

    #[test]
    fn param_lookup() {
        let ct = ContentType::text_plain_utf8();
        assert_eq!(ct.param("charset"), Some("utf-8"));
        assert_eq!(ct.param("boundary"), None);
    }

    #[test]
    fn parse_simple() {
        let ct: ContentType = "application/json".parse().unwrap();
        assert_eq!(ct.type_, "application");
        assert_eq!(ct.subtype, "json");
        assert!(ct.params.is_empty());
    }

    #[test]
    fn parse_with_charset() {
        let ct: ContentType = "text/plain; charset=utf-8".parse().unwrap();
        assert_eq!(ct.type_, "text");
        assert_eq!(ct.subtype, "plain");
        assert_eq!(ct.param("charset"), Some("utf-8"));
    }

    #[test]
    fn parse_case_insensitive_type() {
        let ct: ContentType = "Application/JSON".parse().unwrap();
        assert_eq!(ct.type_, "application");
        assert_eq!(ct.subtype, "json");
    }

    #[test]
    fn parse_invalid_no_slash() {
        assert_eq!("application".parse::<ContentType>(), Err(ParseContentTypeError));
    }

    #[test]
    fn parse_invalid_empty() {
        assert_eq!("".parse::<ContentType>(), Err(ParseContentTypeError));
    }

    #[test]
    fn round_trip() {
        let ct = ContentType::text_plain_utf8();
        let s = ct.to_string();
        let back: ContentType = s.parse().unwrap();
        assert_eq!(back, ct);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_round_trip() {
        let ct = ContentType::application_problem_json();
        let json = serde_json::to_string(&ct).unwrap();
        assert_eq!(json, r#""application/problem+json""#);
        let back: ContentType = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ct);
    }
}
