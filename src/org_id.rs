// SPDX-License-Identifier: LicenseRef-Proprietary
//! Tenant identifier newtype, transported via the `X-Org-Id` HTTP header.
//!
//! # Example
//!
//! ```rust
//! use api_bones::org_id::OrgId;
//!
//! let id = OrgId::generate();
//! assert_eq!(id.inner().get_version_num(), 4);
//! assert_eq!(OrgId::HEADER_NAME, "X-Org-Id");
//! ```

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::string::{String, ToString};
use core::fmt;
use core::str::FromStr;
#[cfg(feature = "serde")]
use serde::{Deserialize, Deserializer, Serialize};

// ---------------------------------------------------------------------------
// OrgIdError
// ---------------------------------------------------------------------------

/// Error returned when parsing an [`OrgId`] from a string fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrgIdError {
    /// The string is not a valid UUID.
    InvalidUuid(uuid::Error),
}

impl fmt::Display for OrgIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUuid(e) => write!(f, "invalid org ID: {e}"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for OrgIdError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidUuid(e) => Some(e),
        }
    }
}

// ---------------------------------------------------------------------------
// OrgId
// ---------------------------------------------------------------------------

/// A UUID v4 tenant identifier, typically propagated via the `X-Org-Id`
/// HTTP header.
///
/// Use [`OrgId::generate`] to create a fresh identifier, or [`FromStr`] /
/// [`TryFrom`] to parse one from an incoming header.
///
/// The `Display` implementation produces the canonical hyphenated UUID string
/// (e.g. `550e8400-e29b-41d4-a716-446655440000`).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize), serde(transparent))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "utoipa", schema(value_type = String, format = "uuid"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct OrgId(uuid::Uuid);

impl OrgId {
    /// Wrap an existing [`uuid::Uuid`] as an `OrgId`.
    ///
    /// ```rust
    /// use api_bones::org_id::OrgId;
    ///
    /// let id = OrgId::new(uuid::Uuid::nil());
    /// assert_eq!(id.to_string(), "00000000-0000-0000-0000-000000000000");
    /// ```
    #[must_use]
    pub const fn new(id: uuid::Uuid) -> Self {
        Self(id)
    }

    /// Generate a new random `OrgId` (UUID v4).
    ///
    /// ```rust
    /// use api_bones::org_id::OrgId;
    ///
    /// let id = OrgId::generate();
    /// assert_eq!(id.inner().get_version_num(), 4);
    /// ```
    #[must_use]
    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4())
    }

    /// Return the inner [`uuid::Uuid`].
    ///
    /// ```rust
    /// use api_bones::org_id::OrgId;
    ///
    /// let uuid = uuid::Uuid::nil();
    /// let id = OrgId::new(uuid);
    /// assert_eq!(id.inner(), uuid);
    /// ```
    #[must_use]
    pub fn inner(&self) -> uuid::Uuid {
        self.0
    }
}

// ---------------------------------------------------------------------------
// HeaderId trait impl
// ---------------------------------------------------------------------------

#[cfg(feature = "std")]
impl crate::header_id::HeaderId for OrgId {
    const HEADER_NAME: &'static str = "X-Org-Id";

    fn as_str(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Owned(self.0.to_string())
    }
}

#[cfg(all(not(feature = "std"), feature = "alloc"))]
impl crate::header_id::HeaderId for OrgId {
    const HEADER_NAME: &'static str = "X-Org-Id";

    fn as_str(&self) -> alloc::borrow::Cow<'_, str> {
        alloc::borrow::Cow::Owned(self.0.to_string())
    }
}

// ---------------------------------------------------------------------------
// Standard trait impls
// ---------------------------------------------------------------------------

impl Default for OrgId {
    fn default() -> Self {
        Self::generate()
    }
}

impl fmt::Display for OrgId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl From<uuid::Uuid> for OrgId {
    fn from(id: uuid::Uuid) -> Self {
        Self(id)
    }
}

impl From<OrgId> for uuid::Uuid {
    fn from(o: OrgId) -> Self {
        o.0
    }
}

impl FromStr for OrgId {
    type Err = OrgIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        uuid::Uuid::parse_str(s)
            .map(Self)
            .map_err(OrgIdError::InvalidUuid)
    }
}

impl TryFrom<&str> for OrgId {
    type Error = OrgIdError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl TryFrom<String> for OrgId {
    type Error = OrgIdError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

// ---------------------------------------------------------------------------
// Serde
// ---------------------------------------------------------------------------

#[cfg(feature = "serde")]
impl<'de> Deserialize<'de> for OrgId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse::<Self>().map_err(serde::de::Error::custom)
    }
}

// ---------------------------------------------------------------------------
// Axum extractor
// ---------------------------------------------------------------------------

#[cfg(feature = "axum")]
impl<S: Send + Sync> axum::extract::FromRequestParts<S> for OrgId {
    type Rejection = crate::error::ApiError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        let raw = parts
            .headers
            .get("x-org-id")
            .ok_or_else(|| {
                crate::error::ApiError::bad_request("missing required header: x-org-id")
            })?
            .to_str()
            .map_err(|_| {
                crate::error::ApiError::bad_request("header x-org-id contains non-UTF-8 bytes")
            })?;
        raw.parse::<Self>()
            .map_err(|e| crate::error::ApiError::bad_request(format!("invalid X-Org-Id: {e}")))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_wraps_uuid() {
        let uuid = uuid::Uuid::nil();
        let id = OrgId::new(uuid);
        assert_eq!(id.inner(), uuid);
    }

    #[test]
    fn generate_is_v4() {
        let id = OrgId::generate();
        assert_eq!(id.inner().get_version_num(), 4);
    }

    #[test]
    fn display_is_hyphenated_uuid() {
        let id = OrgId::new(uuid::Uuid::nil());
        assert_eq!(id.to_string(), "00000000-0000-0000-0000-000000000000");
    }

    #[test]
    fn from_str_valid() {
        let s = "550e8400-e29b-41d4-a716-446655440000";
        let id: OrgId = s.parse().unwrap();
        assert_eq!(id.to_string(), s);
    }

    #[test]
    fn from_str_invalid() {
        assert!("not-a-uuid".parse::<OrgId>().is_err());
    }

    #[test]
    fn from_into_uuid_roundtrip() {
        let uuid = uuid::Uuid::new_v4();
        let id = OrgId::from(uuid);
        let back: uuid::Uuid = id.into();
        assert_eq!(back, uuid);
    }

    #[test]
    fn default_generates_v4() {
        let id = OrgId::default();
        assert_eq!(id.inner().get_version_num(), 4);
    }

    #[test]
    fn error_display() {
        let err = "not-a-uuid".parse::<OrgId>().unwrap_err();
        let s = err.to_string();
        assert!(s.contains("invalid org ID"));
    }

    #[cfg(feature = "std")]
    #[test]
    fn error_source_is_some() {
        use std::error::Error as _;
        let err = "not-a-uuid".parse::<OrgId>().unwrap_err();
        assert!(err.source().is_some());
    }

    #[test]
    fn try_from_str_valid() {
        let s = "00000000-0000-0000-0000-000000000000";
        let id = OrgId::try_from(s).unwrap();
        assert_eq!(id.to_string(), s);
    }

    #[test]
    fn try_from_string_valid() {
        let s = "550e8400-e29b-41d4-a716-446655440000".to_owned();
        let id = OrgId::try_from(s).unwrap();
        assert_eq!(id.to_string(), "550e8400-e29b-41d4-a716-446655440000");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_roundtrip() {
        let id = OrgId::new(uuid::Uuid::nil());
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, r#""00000000-0000-0000-0000-000000000000""#);
        let back: OrgId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, id);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_invalid_rejects() {
        let result: Result<OrgId, _> = serde_json::from_str(r#""not-a-uuid""#);
        assert!(result.is_err());
    }

    #[test]
    fn header_name_const() {
        use crate::header_id::HeaderId as _;
        let id = OrgId::new(uuid::Uuid::nil());
        assert_eq!(OrgId::HEADER_NAME, "X-Org-Id");
        assert_eq!(id.as_str().as_ref(), "00000000-0000-0000-0000-000000000000");
    }

    #[cfg(feature = "axum")]
    #[tokio::test]
    async fn axum_extract_present() {
        use axum::extract::FromRequestParts;
        use axum::http::Request;

        let uuid_str = "550e8400-e29b-41d4-a716-446655440000";
        let req = Request::builder()
            .header("x-org-id", uuid_str)
            .body(())
            .unwrap();
        let (mut parts, ()) = req.into_parts();
        let id = OrgId::from_request_parts(&mut parts, &()).await.unwrap();
        assert_eq!(id.to_string(), uuid_str);
    }

    #[cfg(feature = "axum")]
    #[tokio::test]
    async fn axum_extract_missing_returns_400() {
        use axum::extract::FromRequestParts;
        use axum::http::Request;

        let req = Request::builder().body(()).unwrap();
        let (mut parts, ()) = req.into_parts();
        let result = OrgId::from_request_parts(&mut parts, &()).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status, 400);
    }

    #[cfg(feature = "axum")]
    #[tokio::test]
    async fn axum_extract_invalid_uuid_returns_400() {
        use axum::extract::FromRequestParts;
        use axum::http::Request;

        let req = Request::builder()
            .header("x-org-id", "not-a-uuid")
            .body(())
            .unwrap();
        let (mut parts, ()) = req.into_parts();
        let result = OrgId::from_request_parts(&mut parts, &()).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status, 400);
    }

    #[cfg(feature = "axum")]
    #[tokio::test]
    async fn axum_extract_non_utf8_returns_400() {
        use axum::extract::FromRequestParts;
        use axum::http::{HeaderValue, Request};

        let req = Request::builder()
            .header("x-org-id", HeaderValue::from_bytes(b"\xff\xfe").unwrap())
            .body(())
            .unwrap();
        let (mut parts, ()) = req.into_parts();
        let result = OrgId::from_request_parts(&mut parts, &()).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status, 400);
    }
}
