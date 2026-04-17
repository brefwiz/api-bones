//! Deprecation marker type (RFC 8594).
//!
//! [`Deprecated`] carries a sunset date and an optional replacement link,
//! and can inject the standard `Deprecation` and `Sunset` response headers
//! defined in [RFC 8594](https://www.rfc-editor.org/rfc/rfc8594).
//!
//! # Example
//!
//! ```rust
//! use api_bones::deprecated::Deprecated;
//!
//! let d = Deprecated::new("2025-12-31")
//!     .with_link("https://api.example.com/v2/docs");
//! assert_eq!(d.sunset, "2025-12-31");
//! assert!(d.link.is_some());
//! ```

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::string::String;
use core::fmt;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Deprecated
// ---------------------------------------------------------------------------

/// Deprecation metadata for an API resource or endpoint.
///
/// Carries the `Sunset` date (RFC 8594) and an optional `Link` to replacement
/// documentation. Use [`inject_headers`](Deprecated::inject_headers) to attach
/// the standard headers to an HTTP response.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Deprecated {
    /// RFC 7231 HTTP-date (or RFC 3339 date) after which the resource is gone.
    ///
    /// Example: `"2025-12-31"` or `"Sat, 31 Dec 2025 00:00:00 GMT"`.
    pub sunset: String,

    /// URL of the replacement resource or migration guide (`rel="successor-version"`).
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub link: Option<String>,
}

impl Deprecated {
    /// Create a new deprecation marker with the given sunset date.
    ///
    /// # Examples
    ///
    /// ```
    /// use api_bones::deprecated::Deprecated;
    ///
    /// let d = Deprecated::new("2025-12-31");
    /// assert_eq!(d.sunset, "2025-12-31");
    /// assert!(d.link.is_none());
    /// ```
    #[must_use]
    pub fn new(sunset: impl Into<String>) -> Self {
        Self {
            sunset: sunset.into(),
            link: None,
        }
    }

    /// Attach a replacement link.
    ///
    /// # Examples
    ///
    /// ```
    /// use api_bones::deprecated::Deprecated;
    ///
    /// let d = Deprecated::new("2025-12-31")
    ///     .with_link("https://api.example.com/v2");
    /// assert_eq!(d.link.as_deref(), Some("https://api.example.com/v2"));
    /// ```
    #[must_use]
    pub fn with_link(mut self, link: impl Into<String>) -> Self {
        self.link = Some(link.into());
        self
    }

    /// Build the value for the `Deprecation` header.
    ///
    /// Per RFC 8594 the value is `true` for a permanently-deprecated resource.
    #[must_use]
    pub fn deprecation_header_value(&self) -> &'static str {
        "true"
    }

    /// Build the value for the `Sunset` header (the sunset date as-is).
    ///
    /// # Examples
    ///
    /// ```
    /// use api_bones::deprecated::Deprecated;
    ///
    /// let d = Deprecated::new("Sat, 31 Dec 2025 00:00:00 GMT");
    /// assert_eq!(d.sunset_header_value(), "Sat, 31 Dec 2025 00:00:00 GMT");
    /// ```
    #[must_use]
    pub fn sunset_header_value(&self) -> &str {
        &self.sunset
    }

    /// Build the `Link` header value for the replacement URL if present.
    ///
    /// Produces `<url>; rel="successor-version"` per RFC 8288.
    ///
    /// # Examples
    ///
    /// ```
    /// use api_bones::deprecated::Deprecated;
    ///
    /// let d = Deprecated::new("2025-12-31")
    ///     .with_link("https://api.example.com/v2");
    /// assert_eq!(
    ///     d.link_header_value().as_deref(),
    ///     Some("<https://api.example.com/v2>; rel=\"successor-version\"")
    /// );
    /// ```
    #[must_use]
    #[cfg(any(feature = "std", feature = "alloc"))]
    pub fn link_header_value(&self) -> Option<String> {
        #[cfg(all(not(feature = "std"), feature = "alloc"))]
        use alloc::format;
        self.link
            .as_deref()
            .map(|url| format!("<{url}>; rel=\"successor-version\""))
    }

    /// Inject `Deprecation`, `Sunset`, and (optionally) `Link` headers into an
    /// [`http::HeaderMap`].
    ///
    /// # Errors
    ///
    /// Returns an error if any header value contains characters invalid for HTTP headers.
    #[cfg(feature = "http")]
    pub fn inject_headers(
        &self,
        headers: &mut http::HeaderMap,
    ) -> Result<(), http::header::InvalidHeaderValue> {
        use http::header::{HeaderName, HeaderValue};

        headers.insert(
            HeaderName::from_static("deprecation"),
            HeaderValue::from_static("true"),
        );
        headers.insert(
            HeaderName::from_static("sunset"),
            HeaderValue::from_str(&self.sunset)?,
        );
        if let Some(link_val) = self.link_header_value() {
            headers.insert(http::header::LINK, HeaderValue::from_str(&link_val)?);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Display
// ---------------------------------------------------------------------------

impl fmt::Display for Deprecated {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Deprecated(sunset={})", self.sunset)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_sets_sunset() {
        let d = Deprecated::new("2025-12-31");
        assert_eq!(d.sunset, "2025-12-31");
        assert!(d.link.is_none());
    }

    #[test]
    fn with_link() {
        let d = Deprecated::new("2025-12-31").with_link("https://example.com/v2");
        assert_eq!(d.link.as_deref(), Some("https://example.com/v2"));
    }

    #[test]
    fn header_values() {
        let d = Deprecated::new("2025-12-31");
        assert_eq!(d.deprecation_header_value(), "true");
        assert_eq!(d.sunset_header_value(), "2025-12-31");
    }

    #[cfg(any(feature = "std", feature = "alloc"))]
    #[test]
    fn link_header_value_format() {
        let d = Deprecated::new("2025-12-31").with_link("https://example.com/v2");
        assert_eq!(
            d.link_header_value().as_deref(),
            Some("<https://example.com/v2>; rel=\"successor-version\"")
        );
    }

    #[cfg(any(feature = "std", feature = "alloc"))]
    #[test]
    fn link_header_value_none() {
        let d = Deprecated::new("2025-12-31");
        assert!(d.link_header_value().is_none());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_round_trip() {
        let d = Deprecated::new("2025-12-31").with_link("https://example.com/v2");
        let json = serde_json::to_value(&d).unwrap();
        assert_eq!(json["sunset"], "2025-12-31");
        assert_eq!(json["link"], "https://example.com/v2");
        let back: Deprecated = serde_json::from_value(json).unwrap();
        assert_eq!(back, d);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_omits_null_link() {
        let d = Deprecated::new("2025-12-31");
        let json = serde_json::to_value(&d).unwrap();
        assert!(json.get("link").is_none());
    }
}
