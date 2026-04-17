//! `Vary` response header helper.
//!
//! The [`Vary`] type represents the set of request header names that affect
//! the cacheability of a response (RFC 7231 §7.1.4). A wildcard `*` value
//! indicates that the response is uncacheable regardless of request headers.
//!
//! # Example
//!
//! ```rust
//! use api_bones::vary::Vary;
//!
//! let mut vary = Vary::new();
//! vary.add("Accept");
//! vary.add("Accept-Encoding");
//! assert!(vary.contains("accept"));          // case-insensitive
//! assert_eq!(vary.to_string(), "Accept, Accept-Encoding");
//!
//! let wildcard = Vary::wildcard();
//! assert!(wildcard.is_wildcard());
//! assert_eq!(wildcard.to_string(), "*");
//! ```

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use core::{fmt, str::FromStr};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Vary
// ---------------------------------------------------------------------------

/// The `Vary` response header (RFC 7231 §7.1.4).
///
/// Encodes the set of request header field names that were used to select the
/// representation. The special value `*` means any request header may affect
/// the response.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum Vary {
    /// `Vary: *` — response is uncacheable by shared caches.
    Wildcard,
    /// `Vary: <header-name>, ...` — a specific list of header names.
    Headers(Vec<String>),
}

impl Vary {
    /// Create an empty `Vary` with no header names.
    #[must_use]
    pub fn new() -> Self {
        Self::Headers(Vec::new())
    }

    /// Create a wildcard `Vary: *`.
    #[must_use]
    pub fn wildcard() -> Self {
        Self::Wildcard
    }

    /// Returns `true` if this is the wildcard variant.
    #[must_use]
    pub fn is_wildcard(&self) -> bool {
        matches!(self, Self::Wildcard)
    }

    /// Returns the list of header names, or `None` if this is a wildcard.
    #[must_use]
    pub fn headers(&self) -> Option<&[String]> {
        match self {
            Self::Wildcard => None,
            Self::Headers(h) => Some(h),
        }
    }

    /// Add a header name to the `Vary` list.
    ///
    /// If `self` is [`Vary::Wildcard`] this is a no-op. Duplicate names
    /// (case-insensitive) are not added a second time.
    ///
    /// ```
    /// use api_bones::vary::Vary;
    ///
    /// let mut v = Vary::new();
    /// v.add("Accept");
    /// v.add("accept"); // duplicate — ignored
    /// assert_eq!(v.to_string(), "Accept");
    /// ```
    pub fn add(&mut self, name: impl Into<String>) {
        if let Self::Headers(headers) = self {
            let name = name.into();
            let lower = name.to_lowercase();
            if !headers.iter().any(|h| h.to_lowercase() == lower) {
                headers.push(name);
            }
        }
    }

    /// Remove a header name from the `Vary` list (case-insensitive).
    ///
    /// Returns `true` if the name was present and removed. Always returns
    /// `false` for the wildcard variant.
    ///
    /// ```
    /// use api_bones::vary::Vary;
    ///
    /// let mut v = Vary::new();
    /// v.add("Accept");
    /// assert!(v.remove("ACCEPT"));
    /// assert_eq!(v.to_string(), "");
    /// ```
    pub fn remove(&mut self, name: &str) -> bool {
        if let Self::Headers(headers) = self {
            let lower = name.to_lowercase();
            let before = headers.len();
            headers.retain(|h| h.to_lowercase() != lower);
            return headers.len() < before;
        }
        false
    }

    /// Returns `true` if the named header is in the `Vary` list
    /// (case-insensitive).
    ///
    /// Always returns `false` for the wildcard variant.
    ///
    /// ```
    /// use api_bones::vary::Vary;
    ///
    /// let mut v = Vary::new();
    /// v.add("Accept-Encoding");
    /// assert!(v.contains("accept-encoding"));
    /// assert!(!v.contains("accept"));
    /// ```
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        match self {
            Self::Wildcard => false,
            Self::Headers(headers) => {
                let lower = name.to_lowercase();
                headers.iter().any(|h| h.to_lowercase() == lower)
            }
        }
    }

    /// Returns the number of header names in the list, or `None` for wildcard.
    #[must_use]
    pub fn len(&self) -> Option<usize> {
        match self {
            Self::Wildcard => None,
            Self::Headers(h) => Some(h.len()),
        }
    }

    /// Returns `true` when the list is empty (not a wildcard, and no names).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Wildcard => false,
            Self::Headers(h) => h.is_empty(),
        }
    }
}

impl Default for Vary {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Display / FromStr
// ---------------------------------------------------------------------------

impl fmt::Display for Vary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Wildcard => f.write_str("*"),
            Self::Headers(headers) => {
                for (i, h) in headers.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    f.write_str(h)?;
                }
                Ok(())
            }
        }
    }
}

/// Error returned when parsing a `Vary` header fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseVaryError;

impl fmt::Display for ParseVaryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid Vary header")
    }
}

#[cfg(feature = "std")]
impl std::error::Error for ParseVaryError {}

impl FromStr for Vary {
    type Err = ParseVaryError;

    /// Parse a `Vary` header value.
    ///
    /// ```
    /// use api_bones::vary::Vary;
    ///
    /// let v: Vary = "*".parse().unwrap();
    /// assert!(v.is_wildcard());
    ///
    /// let v: Vary = "Accept, Accept-Encoding".parse().unwrap();
    /// assert!(v.contains("Accept-Encoding"));
    /// ```
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if s == "*" {
            return Ok(Self::Wildcard);
        }
        let mut vary = Self::new();
        for part in s.split(',') {
            let part = part.trim();
            if !part.is_empty() {
                vary.add(part.to_string());
            }
        }
        Ok(vary)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_is_empty() {
        let v = Vary::new();
        assert!(v.is_empty());
        assert!(!v.is_wildcard());
        assert_eq!(v.to_string(), "");
    }

    #[test]
    fn wildcard() {
        let v = Vary::wildcard();
        assert!(v.is_wildcard());
        assert!(!v.is_empty());
        assert_eq!(v.to_string(), "*");
    }

    #[test]
    fn add_and_contains() {
        let mut v = Vary::new();
        v.add("Accept");
        v.add("Accept-Encoding");
        assert!(v.contains("Accept"));
        assert!(v.contains("accept"));
        assert!(v.contains("ACCEPT-ENCODING"));
        assert!(!v.contains("Content-Type"));
        assert_eq!(v.len(), Some(2));
    }

    #[test]
    fn add_deduplicates() {
        let mut v = Vary::new();
        v.add("Accept");
        v.add("accept");
        assert_eq!(v.len(), Some(1));
        assert_eq!(v.to_string(), "Accept");
    }

    #[test]
    fn remove_present() {
        let mut v = Vary::new();
        v.add("Accept");
        v.add("Content-Type");
        assert!(v.remove("accept"));
        assert!(!v.contains("Accept"));
        assert_eq!(v.len(), Some(1));
    }

    #[test]
    fn remove_absent_returns_false() {
        let mut v = Vary::new();
        v.add("Accept");
        assert!(!v.remove("Content-Type"));
    }

    #[test]
    fn add_on_wildcard_is_noop() {
        let mut v = Vary::wildcard();
        v.add("Accept");
        assert!(v.is_wildcard());
    }

    #[test]
    fn remove_on_wildcard_returns_false() {
        let mut v = Vary::wildcard();
        assert!(!v.remove("Accept"));
    }

    #[test]
    fn display_multiple() {
        let mut v = Vary::new();
        v.add("Accept");
        v.add("Accept-Encoding");
        assert_eq!(v.to_string(), "Accept, Accept-Encoding");
    }

    #[test]
    fn parse_wildcard() {
        let v: Vary = "*".parse().unwrap();
        assert!(v.is_wildcard());
    }

    #[test]
    fn parse_header_list() {
        let v: Vary = "Accept, Accept-Encoding".parse().unwrap();
        assert!(v.contains("Accept"));
        assert!(v.contains("Accept-Encoding"));
        assert_eq!(v.len(), Some(2));
    }

    #[test]
    fn roundtrip() {
        let mut v = Vary::new();
        v.add("Accept");
        v.add("Origin");
        let s = v.to_string();
        let parsed: Vary = s.parse().unwrap();
        assert_eq!(parsed, v);
    }
}
