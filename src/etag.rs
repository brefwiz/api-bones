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
mod axum_support {
    use super::ETag;
    use axum::http::HeaderValue;
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

    #[cfg(feature = "axum")]
    #[test]
    fn etag_weak_into_response_parts_sets_etag_header() {
        use axum::response::IntoResponse;

        let response = (ETag::weak("xyz"), axum::http::StatusCode::OK).into_response();
        let etag_header = response.headers().get(axum::http::header::ETAG);
        assert_eq!(etag_header.unwrap().to_str().unwrap(), "W/\"xyz\"");
    }
}
