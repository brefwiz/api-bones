//! `ETag` and conditional request types (RFC 7232).
//!
//! [`ETag`] represents an HTTP entity tag with strong or weak variants.
//! [`IfMatch`] and [`IfNoneMatch`] model the corresponding conditional request
//! headers, supporting single values, multiple values, and the wildcard `*`.
//!
//! # Example
//!
//! ```rust
//! use api_bones::etag::{ETag, IfMatch};
//!
//! let tag = ETag::strong("abc123");
//! assert_eq!(tag.to_string(), "\"abc123\"");
//!
//! let weak = ETag::weak("xyz");
//! assert_eq!(weak.to_string(), "W/\"xyz\"");
//!
//! assert!(tag.matches(&ETag::strong("abc123")));
//! assert!(!tag.matches(&weak));
//! ```

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{string::String, vec::Vec};
use core::fmt;
#[cfg(feature = "http")]
use core::str::FromStr;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// ETag
// ---------------------------------------------------------------------------

/// An HTTP entity tag as defined by [RFC 7232 §2.3](https://www.rfc-editor.org/rfc/rfc7232#section-2.3).
///
/// An `ETag` is either **strong** (default) or **weak** (prefixed with `W/`).
/// Strong `ETags` require byte-for-byte equality; weak `ETags` indicate semantic
/// equivalence only.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "proptest", derive(proptest_derive::Arbitrary))]
pub struct ETag {
    /// The opaque tag value (without surrounding quotes).
    pub value: String,
    /// Whether this is a weak `ETag`.
    pub weak: bool,
}

impl ETag {
    /// Construct a strong `ETag`.
    ///
    /// The serialized form is `"<value>"` (with surrounding double-quotes).
    ///
    /// ```
    /// use api_bones::etag::ETag;
    ///
    /// let tag = ETag::strong("v1");
    /// assert_eq!(tag.to_string(), "\"v1\"");
    /// ```
    pub fn strong(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            weak: false,
        }
    }

    /// Construct a weak `ETag`.
    ///
    /// The serialized form is `W/"<value>"`.
    ///
    /// ```
    /// use api_bones::etag::ETag;
    ///
    /// let tag = ETag::weak("v1");
    /// assert_eq!(tag.to_string(), "W/\"v1\"");
    /// ```
    pub fn weak(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            weak: true,
        }
    }

    /// Compare two `ETags` according to RFC 7232 §2.3 comparison rules.
    ///
    /// **Strong comparison**: both must be strong *and* have the same value.
    /// **Weak comparison**: the values must match regardless of weakness.
    ///
    /// This method uses **strong comparison**: returns `true` only when both
    /// `ETags` are strong and their values are identical.
    ///
    /// ```
    /// use api_bones::etag::ETag;
    ///
    /// let a = ETag::strong("abc");
    /// let b = ETag::strong("abc");
    /// assert!(a.matches(&b));
    ///
    /// // Weak tags never match under strong comparison.
    /// let w = ETag::weak("abc");
    /// assert!(!a.matches(&w));
    /// ```
    #[must_use]
    pub fn matches(&self, other: &Self) -> bool {
        !self.weak && !other.weak && self.value == other.value
    }

    /// Weak comparison per RFC 7232 §2.3: values match regardless of strength.
    ///
    /// ```
    /// use api_bones::etag::ETag;
    ///
    /// let strong = ETag::strong("abc");
    /// let weak = ETag::weak("abc");
    /// assert!(strong.matches_weak(&weak));
    /// assert!(weak.matches_weak(&strong));
    /// ```
    #[must_use]
    pub fn matches_weak(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl fmt::Display for ETag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.weak {
            write!(f, "W/\"{}\"", self.value)
        } else {
            write!(f, "\"{}\"", self.value)
        }
    }
}

// ---------------------------------------------------------------------------
// Parsing (RFC 7232 §2.3 wire format) — `http` feature
// ---------------------------------------------------------------------------

/// Error returned when parsing an [`ETag`] from its HTTP wire format fails.
#[cfg(feature = "http")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseETagError {
    /// The input was empty after trimming.
    Empty,
    /// The tag was not enclosed in double quotes.
    Unquoted,
    /// The input was otherwise malformed (e.g. stray `W` prefix, missing closing quote).
    Malformed,
}

#[cfg(feature = "http")]
impl fmt::Display for ParseETagError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("ETag is empty"),
            Self::Unquoted => f.write_str("ETag must be enclosed in double quotes"),
            Self::Malformed => f.write_str("ETag is malformed"),
        }
    }
}

#[cfg(all(feature = "http", feature = "std"))]
impl std::error::Error for ParseETagError {}

#[cfg(feature = "http")]
impl FromStr for ETag {
    type Err = ParseETagError;

    /// Parse an `ETag` from its RFC 7232 §2.3 wire format.
    ///
    /// Accepts `"<value>"` (strong) and `W/"<value>"` (weak). Leading and
    /// trailing ASCII whitespace is trimmed.
    ///
    /// ```
    /// use api_bones::etag::ETag;
    ///
    /// let strong: ETag = "\"v1\"".parse().unwrap();
    /// assert_eq!(strong, ETag::strong("v1"));
    ///
    /// let weak: ETag = "W/\"v1\"".parse().unwrap();
    /// assert_eq!(weak, ETag::weak("v1"));
    /// ```
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if s.is_empty() {
            return Err(ParseETagError::Empty);
        }

        let (weak, rest) = if let Some(rest) = s.strip_prefix("W/") {
            (true, rest)
        } else {
            (false, s)
        };

        let rest = rest.trim_start();
        if !rest.starts_with('"') {
            return Err(ParseETagError::Unquoted);
        }
        if rest.len() < 2 || !rest.ends_with('"') {
            return Err(ParseETagError::Malformed);
        }
        let value = &rest[1..rest.len() - 1];
        if value.contains('"') {
            return Err(ParseETagError::Malformed);
        }
        Ok(Self {
            value: value.into(),
            weak,
        })
    }
}

#[cfg(feature = "http")]
impl ETag {
    /// Parse a comma-separated list of `ETag`s from a header value
    /// (e.g. the body of an `If-Match` or `If-None-Match` header).
    ///
    /// Returns an error on the first malformed entry.
    ///
    /// ```
    /// use api_bones::etag::ETag;
    ///
    /// let tags = ETag::parse_list("\"a\", W/\"b\", \"c\"").unwrap();
    /// assert_eq!(tags.len(), 3);
    /// assert_eq!(tags[0], ETag::strong("a"));
    /// assert_eq!(tags[1], ETag::weak("b"));
    /// ```
    pub fn parse_list(s: &str) -> Result<Vec<Self>, ParseETagError> {
        // Split on commas that are outside quoted sections.
        let mut out = Vec::new();
        let mut start = 0usize;
        let mut in_quotes = false;
        let bytes = s.as_bytes();
        for (i, &b) in bytes.iter().enumerate() {
            match b {
                b'"' => in_quotes = !in_quotes,
                b',' if !in_quotes => {
                    let piece = &s[start..i];
                    if !piece.trim().is_empty() {
                        out.push(piece.parse::<Self>()?);
                    }
                    start = i + 1;
                }
                _ => {}
            }
        }
        let tail = &s[start..];
        if !tail.trim().is_empty() {
            out.push(tail.parse::<Self>()?);
        }
        if out.is_empty() {
            return Err(ParseETagError::Empty);
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// IfMatch
// ---------------------------------------------------------------------------

/// Models the `If-Match` conditional request header (RFC 7232 §3.1).
///
/// A request with `If-Match: *` matches any existing representation.
/// A request with a list of `ETags` matches if the current `ETag` is in the list
/// (using strong comparison).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "serde", serde(tag = "type", content = "tags"))]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "proptest", derive(proptest_derive::Arbitrary))]
pub enum IfMatch {
    /// Matches any existing representation (`If-Match: *`).
    Any,
    /// Matches if the current `ETag` is in this list (strong comparison).
    Tags(Vec<ETag>),
}

impl IfMatch {
    /// Returns `true` if the given `current` `ETag` satisfies this `If-Match`
    /// condition using strong comparison per RFC 7232.
    ///
    /// ```
    /// use api_bones::etag::{ETag, IfMatch};
    ///
    /// // Wildcard matches everything.
    /// assert!(IfMatch::Any.matches(&ETag::strong("v1")));
    ///
    /// // Tag list uses strong comparison.
    /// let cond = IfMatch::Tags(vec![ETag::strong("v1"), ETag::strong("v2")]);
    /// assert!(cond.matches(&ETag::strong("v1")));
    /// assert!(!cond.matches(&ETag::strong("v3")));
    /// ```
    #[must_use]
    pub fn matches(&self, current: &ETag) -> bool {
        match self {
            Self::Any => true,
            Self::Tags(tags) => tags.iter().any(|t| t.matches(current)),
        }
    }
}

// ---------------------------------------------------------------------------
// IfNoneMatch
// ---------------------------------------------------------------------------

/// Models the `If-None-Match` conditional request header (RFC 7232 §3.2).
///
/// A request with `If-None-Match: *` fails if *any* representation exists.
/// A request with a list of `ETags` fails (i.e., condition is false) if the
/// current `ETag` matches any of the listed `ETags` (weak comparison).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "serde", serde(tag = "type", content = "tags"))]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "proptest", derive(proptest_derive::Arbitrary))]
pub enum IfNoneMatch {
    /// Condition fails if any representation exists (`If-None-Match: *`).
    Any,
    /// Condition fails if the current `ETag` weakly matches any of these.
    Tags(Vec<ETag>),
}

impl IfNoneMatch {
    /// Returns `true` when the condition is **satisfied** (i.e., the server
    /// should proceed with the request).
    ///
    /// Per RFC 7232 §3.2, the condition is satisfied when the current `ETag`
    /// does **not** weakly match any tag in the list (or no representation
    /// exists for `Any`).
    ///
    /// ```
    /// use api_bones::etag::{ETag, IfNoneMatch};
    ///
    /// // Wildcard is never satisfied (any representation exists).
    /// assert!(!IfNoneMatch::Any.matches(&ETag::strong("v1")));
    ///
    /// // Satisfied when current tag is NOT in the list.
    /// let cond = IfNoneMatch::Tags(vec![ETag::strong("v1")]);
    /// assert!(cond.matches(&ETag::strong("v2")));
    /// assert!(!cond.matches(&ETag::strong("v1")));
    /// ```
    #[must_use]
    pub fn matches(&self, current: &ETag) -> bool {
        match self {
            // * means "fail if anything exists" — condition NOT satisfied
            Self::Any => false,
            // condition satisfied only when current is NOT in the list
            Self::Tags(tags) => !tags.iter().any(|t| t.matches_weak(current)),
        }
    }
}

// ---------------------------------------------------------------------------
// Axum feature: TypedHeader support
// ---------------------------------------------------------------------------

#[cfg(feature = "axum")]
#[allow(clippy::result_large_err)]
mod axum_support {
    use super::{ETag, IfMatch, IfNoneMatch};
    use crate::error::ApiError;
    use axum::extract::FromRequestParts;
    use axum::http::HeaderValue;
    use axum::http::request::Parts;
    use axum::response::{IntoResponseParts, ResponseParts};

    impl IntoResponseParts for ETag {
        type Error = std::convert::Infallible;

        fn into_response_parts(self, mut res: ResponseParts) -> Result<ResponseParts, Self::Error> {
            let val = HeaderValue::from_str(&self.to_string())
                .expect("ETag display value is always valid ASCII");
            res.headers_mut().insert(axum::http::header::ETAG, val);
            Ok(res)
        }
    }

    fn header_str<'a>(
        parts: &'a Parts,
        name: &axum::http::HeaderName,
    ) -> Result<&'a str, ApiError> {
        parts
            .headers
            .get(name)
            .ok_or_else(|| ApiError::bad_request(format!("missing {name} header")))?
            .to_str()
            .map_err(|_| ApiError::bad_request(format!("{name} header is not valid ASCII")))
    }

    fn parse_condition(raw: &str) -> Result<(bool, Vec<ETag>), ApiError> {
        let trimmed = raw.trim();
        if trimmed == "*" {
            return Ok((true, Vec::new()));
        }
        let tags = ETag::parse_list(trimmed).map_err(|e| ApiError::bad_request(format!("{e}")))?;
        Ok((false, tags))
    }

    impl<S: Send + Sync> FromRequestParts<S> for IfMatch {
        type Rejection = ApiError;

        async fn from_request_parts(
            parts: &mut Parts,
            _state: &S,
        ) -> Result<Self, Self::Rejection> {
            let raw = header_str(parts, &axum::http::header::IF_MATCH)?;
            let (is_any, tags) = parse_condition(raw)?;
            if is_any {
                Ok(Self::Any)
            } else {
                Ok(Self::Tags(tags))
            }
        }
    }

    impl<S: Send + Sync> FromRequestParts<S> for IfNoneMatch {
        type Rejection = ApiError;

        async fn from_request_parts(
            parts: &mut Parts,
            _state: &S,
        ) -> Result<Self, Self::Rejection> {
            let raw = header_str(parts, &axum::http::header::IF_NONE_MATCH)?;
            let (is_any, tags) = parse_condition(raw)?;
            if is_any {
                Ok(Self::Any)
            } else {
                Ok(Self::Tags(tags))
            }
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
    // ETag construction
    // -----------------------------------------------------------------------

    #[test]
    fn etag_strong_construction() {
        let t = ETag::strong("abc");
        assert_eq!(t.value, "abc");
        assert!(!t.weak);
    }

    #[test]
    fn etag_weak_construction() {
        let t = ETag::weak("xyz");
        assert_eq!(t.value, "xyz");
        assert!(t.weak);
    }

    // -----------------------------------------------------------------------
    // Display / formatting
    // -----------------------------------------------------------------------

    #[test]
    fn etag_strong_display() {
        assert_eq!(ETag::strong("v1").to_string(), "\"v1\"");
    }

    #[test]
    fn etag_weak_display() {
        assert_eq!(ETag::weak("v1").to_string(), "W/\"v1\"");
    }

    // -----------------------------------------------------------------------
    // RFC 7232 comparison
    // -----------------------------------------------------------------------

    #[test]
    fn etag_strong_matches_same_strong() {
        let a = ETag::strong("abc");
        let b = ETag::strong("abc");
        assert!(a.matches(&b));
    }

    #[test]
    fn etag_strong_does_not_match_different_value() {
        let a = ETag::strong("abc");
        let b = ETag::strong("def");
        assert!(!a.matches(&b));
    }

    #[test]
    fn etag_strong_does_not_match_weak() {
        let a = ETag::strong("abc");
        let b = ETag::weak("abc");
        assert!(!a.matches(&b));
    }

    #[test]
    fn etag_weak_does_not_match_strong() {
        let a = ETag::weak("abc");
        let b = ETag::strong("abc");
        assert!(!a.matches(&b));
    }

    #[test]
    fn etag_weak_does_not_match_weak_strong_comparison() {
        let a = ETag::weak("abc");
        let b = ETag::weak("abc");
        assert!(!a.matches(&b));
    }

    #[test]
    fn etag_weak_matches_same_value_weak_comparison() {
        let a = ETag::weak("abc");
        let b = ETag::strong("abc");
        assert!(a.matches_weak(&b));
    }

    #[test]
    fn etag_weak_comparison_both_weak() {
        let a = ETag::weak("abc");
        let b = ETag::weak("abc");
        assert!(a.matches_weak(&b));
    }

    #[test]
    fn etag_weak_comparison_different_values() {
        let a = ETag::weak("abc");
        let b = ETag::weak("def");
        assert!(!a.matches_weak(&b));
    }

    // -----------------------------------------------------------------------
    // IfMatch
    // -----------------------------------------------------------------------

    #[test]
    fn if_match_any_always_matches() {
        assert!(IfMatch::Any.matches(&ETag::strong("x")));
        assert!(IfMatch::Any.matches(&ETag::weak("x")));
    }

    #[test]
    fn if_match_tags_strong_match() {
        let cond = IfMatch::Tags(vec![ETag::strong("abc"), ETag::strong("def")]);
        assert!(cond.matches(&ETag::strong("abc")));
        assert!(cond.matches(&ETag::strong("def")));
    }

    #[test]
    fn if_match_tags_no_match() {
        let cond = IfMatch::Tags(vec![ETag::strong("abc")]);
        assert!(!cond.matches(&ETag::strong("xyz")));
    }

    #[test]
    fn if_match_tags_weak_etag_does_not_match() {
        let cond = IfMatch::Tags(vec![ETag::strong("abc")]);
        assert!(!cond.matches(&ETag::weak("abc")));
    }

    // -----------------------------------------------------------------------
    // IfNoneMatch
    // -----------------------------------------------------------------------

    #[test]
    fn if_none_match_any_never_satisfied() {
        assert!(!IfNoneMatch::Any.matches(&ETag::strong("x")));
    }

    #[test]
    fn if_none_match_tags_satisfied_when_not_present() {
        let cond = IfNoneMatch::Tags(vec![ETag::strong("abc")]);
        assert!(cond.matches(&ETag::strong("xyz")));
    }

    #[test]
    fn if_none_match_tags_not_satisfied_when_present_strong() {
        let cond = IfNoneMatch::Tags(vec![ETag::strong("abc")]);
        assert!(!cond.matches(&ETag::strong("abc")));
    }

    #[test]
    fn if_none_match_tags_not_satisfied_weak_comparison() {
        let cond = IfNoneMatch::Tags(vec![ETag::weak("abc")]);
        assert!(!cond.matches(&ETag::strong("abc")));
    }

    #[test]
    fn if_none_match_tags_not_satisfied_both_weak() {
        let cond = IfNoneMatch::Tags(vec![ETag::weak("abc")]);
        assert!(!cond.matches(&ETag::weak("abc")));
    }

    // -----------------------------------------------------------------------
    // Serde round-trips
    // -----------------------------------------------------------------------

    #[cfg(feature = "serde")]
    #[test]
    fn etag_serde_round_trip_strong() {
        let t = ETag::strong("abc123");
        let json = serde_json::to_value(&t).unwrap();
        let back: ETag = serde_json::from_value(json).unwrap();
        assert_eq!(back, t);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn etag_serde_round_trip_weak() {
        let t = ETag::weak("xyz");
        let json = serde_json::to_value(&t).unwrap();
        let back: ETag = serde_json::from_value(json).unwrap();
        assert_eq!(back, t);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn if_match_any_serde_round_trip() {
        let cond = IfMatch::Any;
        let json = serde_json::to_value(&cond).unwrap();
        let back: IfMatch = serde_json::from_value(json).unwrap();
        assert_eq!(back, cond);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn if_match_tags_serde_round_trip() {
        let cond = IfMatch::Tags(vec![ETag::strong("abc"), ETag::weak("def")]);
        let json = serde_json::to_value(&cond).unwrap();
        let back: IfMatch = serde_json::from_value(json).unwrap();
        assert_eq!(back, cond);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn if_none_match_any_serde_round_trip() {
        let cond = IfNoneMatch::Any;
        let json = serde_json::to_value(&cond).unwrap();
        let back: IfNoneMatch = serde_json::from_value(json).unwrap();
        assert_eq!(back, cond);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn if_none_match_tags_serde_round_trip() {
        let cond = IfNoneMatch::Tags(vec![ETag::strong("v1")]);
        let json = serde_json::to_value(&cond).unwrap();
        let back: IfNoneMatch = serde_json::from_value(json).unwrap();
        assert_eq!(back, cond);
    }

    // -----------------------------------------------------------------------
    // Axum integration
    // -----------------------------------------------------------------------

    #[cfg(feature = "axum")]
    #[test]
    fn etag_into_response_parts_sets_etag_header() {
        use axum::response::IntoResponse;

        let response = (ETag::strong("abc123"), axum::http::StatusCode::OK).into_response();
        let etag_header = response.headers().get(axum::http::header::ETAG);
        assert_eq!(etag_header.unwrap().to_str().unwrap(), "\"abc123\"");
    }

    // -----------------------------------------------------------------------
    // FromStr / parse_list (http feature)
    // -----------------------------------------------------------------------

    #[cfg(feature = "http")]
    #[test]
    fn etag_from_str_strong() {
        let t: ETag = "\"v1\"".parse().unwrap();
        assert_eq!(t, ETag::strong("v1"));
    }

    #[cfg(feature = "http")]
    #[test]
    fn etag_from_str_weak() {
        let t: ETag = "W/\"v1\"".parse().unwrap();
        assert_eq!(t, ETag::weak("v1"));
    }

    #[cfg(feature = "http")]
    #[test]
    fn etag_from_str_rejects_unquoted() {
        assert_eq!("v1".parse::<ETag>(), Err(ParseETagError::Unquoted));
    }

    #[cfg(feature = "http")]
    #[test]
    fn etag_from_str_rejects_empty() {
        assert_eq!("".parse::<ETag>(), Err(ParseETagError::Empty));
        assert_eq!("   ".parse::<ETag>(), Err(ParseETagError::Empty));
    }

    #[cfg(feature = "http")]
    #[test]
    fn etag_from_str_rejects_missing_closing_quote() {
        assert!("\"v1".parse::<ETag>().is_err());
    }

    #[cfg(feature = "http")]
    #[test]
    fn etag_from_str_rejects_embedded_quote() {
        assert_eq!("\"a\"b\"".parse::<ETag>(), Err(ParseETagError::Malformed));
    }

    #[cfg(feature = "http")]
    #[test]
    fn etag_from_str_trims_whitespace() {
        let t: ETag = "  \"v1\"  ".parse().unwrap();
        assert_eq!(t, ETag::strong("v1"));
    }

    #[cfg(feature = "http")]
    #[test]
    fn etag_round_trip_strong() {
        let t = ETag::strong("abc123");
        assert_eq!(t.to_string().parse::<ETag>(), Ok(t));
    }

    #[cfg(feature = "http")]
    #[test]
    fn etag_round_trip_weak() {
        let t = ETag::weak("xyz");
        assert_eq!(t.to_string().parse::<ETag>(), Ok(t));
    }

    #[cfg(feature = "http")]
    #[test]
    fn etag_parse_list_multiple() {
        let tags = ETag::parse_list("\"a\", W/\"b\", \"c\"").unwrap();
        assert_eq!(tags.len(), 3);
        assert_eq!(tags[0], ETag::strong("a"));
        assert_eq!(tags[1], ETag::weak("b"));
        assert_eq!(tags[2], ETag::strong("c"));
    }

    #[cfg(feature = "http")]
    #[test]
    fn etag_parse_list_single() {
        let tags = ETag::parse_list("\"only\"").unwrap();
        assert_eq!(tags, vec![ETag::strong("only")]);
    }

    #[cfg(feature = "http")]
    #[test]
    fn etag_parse_list_empty_errors() {
        assert!(ETag::parse_list("").is_err());
        assert!(ETag::parse_list("   ").is_err());
    }

    #[cfg(feature = "http")]
    #[test]
    fn etag_parse_list_propagates_bad_entry() {
        assert!(ETag::parse_list("\"a\", bad, \"c\"").is_err());
    }

    #[cfg(feature = "axum")]
    mod axum_extractor_tests {
        use super::super::*;
        use axum::extract::FromRequestParts;
        use axum::http::Request;

        async fn extract_if_match(header: Option<&str>) -> Result<IfMatch, ApiError> {
            let mut builder = Request::builder();
            if let Some(v) = header {
                builder = builder.header("if-match", v);
            }
            let req = builder.body(()).unwrap();
            let (mut parts, ()) = req.into_parts();
            IfMatch::from_request_parts(&mut parts, &()).await
        }

        async fn extract_if_none_match(header: Option<&str>) -> Result<IfNoneMatch, ApiError> {
            let mut builder = Request::builder();
            if let Some(v) = header {
                builder = builder.header("if-none-match", v);
            }
            let req = builder.body(()).unwrap();
            let (mut parts, ()) = req.into_parts();
            IfNoneMatch::from_request_parts(&mut parts, &()).await
        }

        use crate::error::ApiError;

        #[tokio::test]
        async fn if_match_wildcard() {
            let r = extract_if_match(Some("*")).await.unwrap();
            assert_eq!(r, IfMatch::Any);
        }

        #[tokio::test]
        async fn if_match_tag_list() {
            let r = extract_if_match(Some("\"a\", W/\"b\"")).await.unwrap();
            assert_eq!(r, IfMatch::Tags(vec![ETag::strong("a"), ETag::weak("b")]));
        }

        #[tokio::test]
        async fn if_match_missing_header_is_bad_request() {
            let err = extract_if_match(None).await.unwrap_err();
            assert_eq!(err.status, 400);
        }

        #[tokio::test]
        async fn if_match_malformed_is_bad_request() {
            let err = extract_if_match(Some("not-a-tag")).await.unwrap_err();
            assert_eq!(err.status, 400);
        }

        #[tokio::test]
        async fn if_none_match_wildcard() {
            let r = extract_if_none_match(Some("*")).await.unwrap();
            assert_eq!(r, IfNoneMatch::Any);
        }

        #[tokio::test]
        async fn if_none_match_tag_list() {
            let r = extract_if_none_match(Some("\"v1\"")).await.unwrap();
            assert_eq!(r, IfNoneMatch::Tags(vec![ETag::strong("v1")]));
        }

        #[tokio::test]
        async fn if_match_non_ascii_header_rejected() {
            // Header value bytes outside ASCII → to_str() fails → bad_request.
            let req = Request::builder()
                .header("if-match", &[0xFFu8][..])
                .body(())
                .unwrap();
            let (mut parts, ()) = req.into_parts();
            let err = IfMatch::from_request_parts(&mut parts, &())
                .await
                .unwrap_err();
            assert_eq!(err.status, 400);
        }

        #[tokio::test]
        async fn if_none_match_missing_is_bad_request() {
            let err = extract_if_none_match(None).await.unwrap_err();
            assert_eq!(err.status, 400);
        }
    }

    #[cfg(feature = "axum")]
    #[test]
    fn etag_weak_into_response_parts_sets_etag_header() {
        use axum::response::IntoResponse;

        let response = (ETag::weak("xyz"), axum::http::StatusCode::OK).into_response();
        let etag_header = response.headers().get(axum::http::header::ETAG);
        assert_eq!(etag_header.unwrap().to_str().unwrap(), "W/\"xyz\"");
    }
}
