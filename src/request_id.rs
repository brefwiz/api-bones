//! Standalone `RequestId` newtype for tracing HTTP requests end-to-end.
//!
//! `RequestId` is a UUID v4 wrapper that surfaces the `X-Request-Id` header
//! convention used across many API frameworks and proxies. It is reusable in
//! both [`crate::error::ApiError`] and [`crate::response::ResponseMeta`].
//!
//! # Example
//!
//! ```rust
//! use api_bones::request_id::RequestId;
//!
//! let id = RequestId::new();
//! assert_eq!(id.header_name(), "X-Request-Id");
//! assert!(!id.to_string().is_empty());
//! ```

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::string::{String, ToString};
use core::fmt;
use core::str::FromStr;
#[cfg(feature = "serde")]
use serde::{Deserialize, Deserializer, Serialize};

// ---------------------------------------------------------------------------
// RequestIdError
// ---------------------------------------------------------------------------

/// Error returned when parsing a [`RequestId`] from a string fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestIdParseError(uuid::Error);

impl fmt::Display for RequestIdParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid request ID: {}", self.0)
    }
}

#[cfg(feature = "std")]
impl std::error::Error for RequestIdParseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

// ---------------------------------------------------------------------------
// RequestId
// ---------------------------------------------------------------------------

/// A UUID v4 request identifier, typically propagated via the `X-Request-Id`
/// HTTP header.
///
/// Use [`RequestId::new`] to generate a fresh identifier, or
/// [`RequestId::from_str`] / [`TryFrom`] to parse one from an incoming header.
///
/// The `Display` implementation produces the canonical hyphenated UUID string
/// (e.g. `550e8400-e29b-41d4-a716-446655440000`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct RequestId(uuid::Uuid);

impl RequestId {
    /// Generate a new random `RequestId` (UUID v4).
    ///
    /// ```rust
    /// use api_bones::request_id::RequestId;
    ///
    /// let id = RequestId::new();
    /// assert_eq!(id.as_uuid().get_version_num(), 4);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }

    /// Wrap an existing [`uuid::Uuid`] as a `RequestId`.
    ///
    /// ```rust
    /// use api_bones::request_id::RequestId;
    ///
    /// let id = RequestId::from_uuid(uuid::Uuid::nil());
    /// assert_eq!(id.to_string(), "00000000-0000-0000-0000-000000000000");
    /// ```
    #[must_use]
    pub fn from_uuid(id: uuid::Uuid) -> Self {
        Self(id)
    }

    /// Return the inner [`uuid::Uuid`].
    #[must_use]
    pub fn as_uuid(&self) -> uuid::Uuid {
        self.0
    }

    /// The canonical HTTP header name for this identifier: `X-Request-Id`.
    #[must_use]
    pub fn header_name(&self) -> &'static str {
        "X-Request-Id"
    }

    /// Return the hyphenated UUID string representation.
    #[must_use]
    pub fn as_str(&self) -> String {
        self.0.to_string()
    }
}

impl Default for RequestId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl From<uuid::Uuid> for RequestId {
    fn from(id: uuid::Uuid) -> Self {
        Self(id)
    }
}

impl From<RequestId> for uuid::Uuid {
    fn from(r: RequestId) -> Self {
        r.0
    }
}

impl FromStr for RequestId {
    type Err = RequestIdParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        uuid::Uuid::parse_str(s)
            .map(Self)
            .map_err(RequestIdParseError)
    }
}

impl TryFrom<&str> for RequestId {
    type Error = RequestIdParseError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl TryFrom<String> for RequestId {
    type Error = RequestIdParseError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

// ---------------------------------------------------------------------------
// Serde
// ---------------------------------------------------------------------------

#[cfg(feature = "serde")]
impl<'de> Deserialize<'de> for RequestId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse::<Self>().map_err(serde::de::Error::custom)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_generates_v4() {
        let id = RequestId::new();
        assert_eq!(id.as_uuid().get_version_num(), 4);
    }

    #[test]
    fn from_uuid_roundtrip() {
        let uuid = uuid::Uuid::nil();
        let id = RequestId::from_uuid(uuid);
        assert_eq!(id.as_uuid(), uuid);
    }

    #[test]
    fn display_is_hyphenated_uuid() {
        let id = RequestId::from_uuid(uuid::Uuid::nil());
        assert_eq!(id.to_string(), "00000000-0000-0000-0000-000000000000");
    }

    #[test]
    fn header_name() {
        let id = RequestId::new();
        assert_eq!(id.header_name(), "X-Request-Id");
    }

    #[test]
    fn from_str_valid() {
        let s = "550e8400-e29b-41d4-a716-446655440000";
        let id: RequestId = s.parse().unwrap();
        assert_eq!(id.to_string(), s);
    }

    #[test]
    fn from_str_invalid() {
        assert!("not-a-uuid".parse::<RequestId>().is_err());
    }

    #[test]
    fn try_from_str() {
        let s = "00000000-0000-0000-0000-000000000000";
        let id = RequestId::try_from(s).unwrap();
        assert_eq!(id.to_string(), s);
    }

    #[test]
    fn from_into_uuid() {
        let uuid = uuid::Uuid::new_v4();
        let id = RequestId::from(uuid);
        let back: uuid::Uuid = id.into();
        assert_eq!(back, uuid);
    }

    #[test]
    fn default_generates_new() {
        let id = RequestId::default();
        assert_eq!(id.as_uuid().get_version_num(), 4);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_roundtrip() {
        let id = RequestId::from_uuid(uuid::Uuid::nil());
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, r#""00000000-0000-0000-0000-000000000000""#);
        let back: RequestId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, id);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_deserialize_invalid_rejects() {
        let result: Result<RequestId, _> = serde_json::from_str(r#""not-a-uuid""#);
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // Coverage gaps: RequestIdParseError Display, source, RequestId::as_str
    // -----------------------------------------------------------------------

    #[test]
    fn request_id_as_str() {
        let id = RequestId::from_uuid(uuid::Uuid::nil());
        assert_eq!(id.as_str(), "00000000-0000-0000-0000-000000000000");
    }

    #[test]
    fn parse_error_display() {
        let err = "not-a-uuid".parse::<RequestId>().unwrap_err();
        let s = err.to_string();
        assert!(s.contains("invalid request ID"));
    }

    #[cfg(feature = "std")]
    #[test]
    fn parse_error_source() {
        use std::error::Error as _;
        let err = "not-a-uuid".parse::<RequestId>().unwrap_err();
        assert!(err.source().is_some());
    }

    #[test]
    fn try_from_string_valid() {
        let s = "550e8400-e29b-41d4-a716-446655440000".to_owned();
        let id = RequestId::try_from(s).unwrap();
        assert_eq!(id.to_string(), "550e8400-e29b-41d4-a716-446655440000");
    }
}
