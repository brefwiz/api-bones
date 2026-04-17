//! `CorrelationId` newtype for cross-service request correlation.
//!
//! A `CorrelationId` is distinct from a [`crate::request_id::RequestId`]:
//! - `RequestId` identifies a single HTTP request at the edge.
//! - `CorrelationId` groups related requests across multiple services
//!   (e.g. an entire user-initiated action that fans out to N microservices).
//!
//! The value is an opaque string transported in the `X-Correlation-Id`
//! HTTP header. UUID v4 generation is provided for convenience.
//!
//! # Example
//!
//! ```rust
//! use api_bones::correlation_id::CorrelationId;
//!
//! let id = CorrelationId::new();
//! assert_eq!(id.header_name(), "X-Correlation-Id");
//!
//! let parsed: CorrelationId = "my-correlation-123".parse().unwrap();
//! assert_eq!(parsed.as_str(), "my-correlation-123");
//! ```

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{borrow::ToOwned, string::{String, ToString}};
use core::{fmt, ops::Deref, str::FromStr};
#[cfg(feature = "serde")]
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

// ---------------------------------------------------------------------------
// CorrelationIdError
// ---------------------------------------------------------------------------

/// Error returned when constructing a [`CorrelationId`] from a string fails.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CorrelationIdError {
    /// The input was empty.
    #[error("correlation ID must not be empty")]
    Empty,
    /// The input exceeds 255 characters.
    #[error("correlation ID must not exceed 255 characters")]
    TooLong,
    /// The input contains non-printable or non-ASCII characters.
    #[error("correlation ID may only contain printable ASCII characters (0x20–0x7E)")]
    InvalidChars,
}

// ---------------------------------------------------------------------------
// CorrelationId
// ---------------------------------------------------------------------------

/// An opaque cross-service correlation identifier, transported via
/// `X-Correlation-Id`.
///
/// # Constraints
///
/// - Length: 1–255 characters.
/// - Characters: printable ASCII only (`0x20`–`0x7E`).
///
/// See the [module-level documentation](self) for the distinction between this
/// type and [`crate::request_id::RequestId`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CorrelationId(String);

impl CorrelationId {
    /// Construct a `CorrelationId` from any printable-ASCII string.
    ///
    /// # Errors
    ///
    /// Returns a [`CorrelationIdError`] variant that describes which constraint
    /// failed.
    ///
    /// ```rust
    /// use api_bones::correlation_id::{CorrelationId, CorrelationIdError};
    ///
    /// assert!(CorrelationId::new("flow-abc-123").is_ok());
    /// assert_eq!(CorrelationId::new(""), Err(CorrelationIdError::Empty));
    /// ```
    pub fn new(s: impl AsRef<str>) -> Result<Self, CorrelationIdError> {
        let s = s.as_ref();
        if s.is_empty() {
            return Err(CorrelationIdError::Empty);
        }
        if s.len() > 255 {
            return Err(CorrelationIdError::TooLong);
        }
        if !s.bytes().all(|b| (0x20..=0x7E).contains(&b)) {
            return Err(CorrelationIdError::InvalidChars);
        }
        Ok(Self(s.to_owned()))
    }

    /// Generate a fresh `CorrelationId` backed by a UUID v4.
    ///
    /// ```rust
    /// use api_bones::correlation_id::CorrelationId;
    ///
    /// let id = CorrelationId::new_uuid();
    /// assert_eq!(id.as_str().len(), 36);
    /// ```
    #[must_use]
    pub fn new_uuid() -> Self {
        // UUID hyphenated string is always 36 printable ASCII chars — always valid.
        Self(uuid::Uuid::new_v4().to_string())
    }

    /// Create a new random `CorrelationId` (alias for [`Self::new_uuid`]).
    ///
    /// ```rust
    /// use api_bones::correlation_id::CorrelationId;
    ///
    /// let id = CorrelationId::new_random();
    /// assert!(!id.as_str().is_empty());
    /// ```
    #[must_use]
    pub fn new_random() -> Self {
        Self::new_uuid()
    }

    /// Convenience constructor; same as [`Self::new_uuid`] — generates a new
    /// UUID v4 backed ID.
    ///
    /// Matches the naming convention of [`crate::request_id::RequestId::new`].
    ///
    /// ```rust
    /// use api_bones::correlation_id::CorrelationId;
    ///
    /// let id = CorrelationId::new_id();
    /// assert_eq!(id.as_str().len(), 36);
    /// ```
    #[must_use]
    pub fn new_id() -> Self {
        Self::new_uuid()
    }

    /// Return the inner string slice.
    ///
    /// ```rust
    /// use api_bones::correlation_id::CorrelationId;
    ///
    /// let id = CorrelationId::new("abc").unwrap();
    /// assert_eq!(id.as_str(), "abc");
    /// ```
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume and return the underlying `String`.
    ///
    /// ```rust
    /// use api_bones::correlation_id::CorrelationId;
    ///
    /// let id = CorrelationId::new("abc").unwrap();
    /// assert_eq!(id.into_string(), "abc");
    /// ```
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }

    /// The canonical HTTP header name: `X-Correlation-Id`.
    ///
    /// ```rust
    /// use api_bones::correlation_id::CorrelationId;
    ///
    /// let id = CorrelationId::new("x").unwrap();
    /// assert_eq!(id.header_name(), "X-Correlation-Id");
    /// ```
    #[must_use]
    pub fn header_name(&self) -> &'static str {
        "X-Correlation-Id"
    }
}

// ---------------------------------------------------------------------------
// Standard trait impls
// ---------------------------------------------------------------------------

impl Deref for CorrelationId {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for CorrelationId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CorrelationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for CorrelationId {
    type Err = CorrelationIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl TryFrom<String> for CorrelationId {
    type Error = CorrelationIdError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::new(s)
    }
}

impl TryFrom<&str> for CorrelationId {
    type Error = CorrelationIdError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Self::new(s)
    }
}

// ---------------------------------------------------------------------------
// Serde
// ---------------------------------------------------------------------------

#[cfg(feature = "serde")]
impl<'de> Deserialize<'de> for CorrelationId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::new(&s).map_err(serde::de::Error::custom)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_id_is_accepted() {
        assert!(CorrelationId::new("flow-abc").is_ok());
        assert!(CorrelationId::new("x").is_ok());
        assert!(CorrelationId::new("abc 123").is_ok()); // space is 0x20 = valid
    }

    #[test]
    fn empty_is_rejected() {
        assert_eq!(CorrelationId::new(""), Err(CorrelationIdError::Empty));
    }

    #[test]
    fn too_long_is_rejected() {
        let s: String = "a".repeat(256);
        assert_eq!(CorrelationId::new(&s), Err(CorrelationIdError::TooLong));
    }

    #[test]
    fn exactly_255_chars_is_accepted() {
        let s: String = "a".repeat(255);
        assert!(CorrelationId::new(&s).is_ok());
    }

    #[test]
    fn control_char_is_rejected() {
        assert_eq!(
            CorrelationId::new("ab\x00c"),
            Err(CorrelationIdError::InvalidChars)
        );
    }

    #[test]
    fn non_ascii_is_rejected() {
        assert_eq!(
            CorrelationId::new("héllo"),
            Err(CorrelationIdError::InvalidChars)
        );
    }

    #[test]
    fn new_uuid_produces_valid_id() {
        let id = CorrelationId::new_uuid();
        assert_eq!(id.as_str().len(), 36);
        assert!(CorrelationId::new(id.as_str()).is_ok());
    }

    #[test]
    fn header_name() {
        let id = CorrelationId::new("x").unwrap();
        assert_eq!(id.header_name(), "X-Correlation-Id");
    }

    #[test]
    fn display() {
        let id = CorrelationId::new("corr-01").unwrap();
        assert_eq!(format!("{id}"), "corr-01");
    }

    #[test]
    fn deref_to_str() {
        let id = CorrelationId::new("abc").unwrap();
        let s: &str = &id;
        assert_eq!(s, "abc");
    }

    #[test]
    fn from_str() {
        let id: CorrelationId = "corr-abc".parse().unwrap();
        assert_eq!(id.as_str(), "corr-abc");
    }

    #[test]
    fn try_from_str() {
        assert!(CorrelationId::try_from("valid").is_ok());
        assert!(CorrelationId::try_from("").is_err());
    }

    #[test]
    fn try_from_string() {
        assert!(CorrelationId::try_from("valid".to_owned()).is_ok());
    }

    #[test]
    fn into_string() {
        let id = CorrelationId::new("abc").unwrap();
        assert_eq!(id.into_string(), "abc");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_roundtrip() {
        let id = CorrelationId::new("corr-xyz-789").unwrap();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, r#""corr-xyz-789""#);
        let back: CorrelationId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, id);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_deserialize_invalid_rejects() {
        let result: Result<CorrelationId, _> = serde_json::from_str(r#""""#);
        assert!(result.is_err());
    }
}
