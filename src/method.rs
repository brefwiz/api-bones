//! HTTP method types.
//!
//! [`HttpMethod`] is a typed enum of all standard HTTP verbs, with
//! `as_str()`, `FromStr`, `Display`, and safety/idempotency predicates.
//!
//! # Example
//!
//! ```rust
//! use api_bones::method::HttpMethod;
//!
//! let m = HttpMethod::Get;
//! assert_eq!(m.as_str(), "GET");
//! assert!(m.is_safe());
//! assert!(m.is_idempotent());
//!
//! let m2: HttpMethod = "POST".parse().unwrap();
//! assert_eq!(m2, HttpMethod::Post);
//! assert!(!m2.is_safe());
//! ```

use core::{fmt, str::FromStr};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// HttpMethod
// ---------------------------------------------------------------------------

/// Standard HTTP request methods (RFC 9110 §9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "UPPERCASE"))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "proptest", derive(proptest_derive::Arbitrary))]
pub enum HttpMethod {
    /// Transfer a current representation of the target resource.
    Get,
    /// Same as GET, but do not transfer the response body.
    Head,
    /// Perform resource-specific processing on the request payload.
    Post,
    /// Replace all current representations of the target resource.
    Put,
    /// Remove all current representations of the target resource.
    Delete,
    /// Establish a tunnel to the server identified by the target resource.
    Connect,
    /// Describe the communication options for the target resource.
    Options,
    /// Perform a message loop-back test along the path to the target resource.
    Trace,
    /// Apply a set of changes to the target resource.
    Patch,
}

impl HttpMethod {
    /// Return the uppercase string representation of this method.
    ///
    /// ```
    /// use api_bones::method::HttpMethod;
    ///
    /// assert_eq!(HttpMethod::Delete.as_str(), "DELETE");
    /// ```
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Head => "HEAD",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
            Self::Connect => "CONNECT",
            Self::Options => "OPTIONS",
            Self::Trace => "TRACE",
            Self::Patch => "PATCH",
        }
    }

    /// Returns `true` for methods that are safe (read-only, no side effects).
    ///
    /// Safe methods per RFC 9110 §9.2.1: GET, HEAD, OPTIONS, TRACE.
    ///
    /// ```
    /// use api_bones::method::HttpMethod;
    ///
    /// assert!(HttpMethod::Get.is_safe());
    /// assert!(!HttpMethod::Post.is_safe());
    /// ```
    #[must_use]
    pub const fn is_safe(&self) -> bool {
        matches!(self, Self::Get | Self::Head | Self::Options | Self::Trace)
    }

    /// Returns `true` for methods that are idempotent.
    ///
    /// Idempotent methods per RFC 9110 §9.2.2: GET, HEAD, PUT, DELETE, OPTIONS, TRACE.
    ///
    /// ```
    /// use api_bones::method::HttpMethod;
    ///
    /// assert!(HttpMethod::Put.is_idempotent());
    /// assert!(!HttpMethod::Post.is_idempotent());
    /// ```
    #[must_use]
    pub const fn is_idempotent(&self) -> bool {
        matches!(
            self,
            Self::Get | Self::Head | Self::Put | Self::Delete | Self::Options | Self::Trace
        )
    }
}

// ---------------------------------------------------------------------------
// Display / FromStr
// ---------------------------------------------------------------------------

impl fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error returned when parsing an [`HttpMethod`] from a string fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseHttpMethodError;

impl fmt::Display for ParseHttpMethodError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("unknown HTTP method")
    }
}

#[cfg(feature = "std")]
impl std::error::Error for ParseHttpMethodError {}

impl FromStr for HttpMethod {
    type Err = ParseHttpMethodError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim() {
            "GET" => Ok(Self::Get),
            "HEAD" => Ok(Self::Head),
            "POST" => Ok(Self::Post),
            "PUT" => Ok(Self::Put),
            "DELETE" => Ok(Self::Delete),
            "CONNECT" => Ok(Self::Connect),
            "OPTIONS" => Ok(Self::Options),
            "TRACE" => Ok(Self::Trace),
            "PATCH" => Ok(Self::Patch),
            _ => Err(ParseHttpMethodError),
        }
    }
}

// ---------------------------------------------------------------------------
// Interop with `http` crate
// ---------------------------------------------------------------------------

#[cfg(feature = "http")]
mod http_interop {
    use super::HttpMethod;

    impl From<HttpMethod> for http::Method {
        fn from(m: HttpMethod) -> Self {
            match m {
                HttpMethod::Get => Self::GET,
                HttpMethod::Head => Self::HEAD,
                HttpMethod::Post => Self::POST,
                HttpMethod::Put => Self::PUT,
                HttpMethod::Delete => Self::DELETE,
                HttpMethod::Connect => Self::CONNECT,
                HttpMethod::Options => Self::OPTIONS,
                HttpMethod::Trace => Self::TRACE,
                HttpMethod::Patch => Self::PATCH,
            }
        }
    }

    impl TryFrom<http::Method> for HttpMethod {
        type Error = super::ParseHttpMethodError;

        fn try_from(m: http::Method) -> Result<Self, Self::Error> {
            m.as_str().parse()
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_str_round_trips() {
        let methods = [
            HttpMethod::Get,
            HttpMethod::Head,
            HttpMethod::Post,
            HttpMethod::Put,
            HttpMethod::Delete,
            HttpMethod::Connect,
            HttpMethod::Options,
            HttpMethod::Trace,
            HttpMethod::Patch,
        ];
        for m in methods {
            let s = m.as_str();
            let parsed: HttpMethod = s.parse().expect("should parse");
            assert_eq!(parsed, m, "round-trip failed for {s}");
        }
    }

    #[test]
    fn display_equals_as_str() {
        assert_eq!(HttpMethod::Get.to_string(), HttpMethod::Get.as_str());
        assert_eq!(HttpMethod::Patch.to_string(), "PATCH");
    }

    #[test]
    fn is_safe() {
        assert!(HttpMethod::Get.is_safe());
        assert!(HttpMethod::Head.is_safe());
        assert!(HttpMethod::Options.is_safe());
        assert!(HttpMethod::Trace.is_safe());
        assert!(!HttpMethod::Post.is_safe());
        assert!(!HttpMethod::Put.is_safe());
        assert!(!HttpMethod::Delete.is_safe());
        assert!(!HttpMethod::Connect.is_safe());
        assert!(!HttpMethod::Patch.is_safe());
    }

    #[test]
    fn is_idempotent() {
        assert!(HttpMethod::Get.is_idempotent());
        assert!(HttpMethod::Head.is_idempotent());
        assert!(HttpMethod::Put.is_idempotent());
        assert!(HttpMethod::Delete.is_idempotent());
        assert!(HttpMethod::Options.is_idempotent());
        assert!(HttpMethod::Trace.is_idempotent());
        assert!(!HttpMethod::Post.is_idempotent());
        assert!(!HttpMethod::Connect.is_idempotent());
        assert!(!HttpMethod::Patch.is_idempotent());
    }

    #[test]
    fn parse_unknown_errors() {
        assert!("BREW".parse::<HttpMethod>().is_err());
        assert!("get".parse::<HttpMethod>().is_err());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_round_trip() {
        let m = HttpMethod::Patch;
        let json = serde_json::to_string(&m).unwrap();
        assert_eq!(json, r#""PATCH""#);
        let back: HttpMethod = serde_json::from_str(&json).unwrap();
        assert_eq!(back, m);
    }

    #[cfg(feature = "http")]
    #[test]
    fn http_crate_round_trip() {
        let pairs = [
            (HttpMethod::Get, http::Method::GET),
            (HttpMethod::Head, http::Method::HEAD),
            (HttpMethod::Post, http::Method::POST),
            (HttpMethod::Put, http::Method::PUT),
            (HttpMethod::Delete, http::Method::DELETE),
            (HttpMethod::Connect, http::Method::CONNECT),
            (HttpMethod::Options, http::Method::OPTIONS),
            (HttpMethod::Trace, http::Method::TRACE),
            (HttpMethod::Patch, http::Method::PATCH),
        ];
        for (our, theirs) in pairs {
            let converted: http::Method = our.clone().into();
            assert_eq!(converted, theirs);
            let back: HttpMethod = converted.try_into().unwrap();
            assert_eq!(back, our);
        }
    }

    #[test]
    fn parse_error_display() {
        let err = "BREW".parse::<HttpMethod>().unwrap_err();
        assert_eq!(err.to_string(), "unknown HTTP method");
    }
}
