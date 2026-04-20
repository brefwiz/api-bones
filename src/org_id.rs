// SPDX-License-Identifier: MIT
//! Tenant identifier newtype, transported via the `X-Org-Id` HTTP header.
//!
//! # Example
//!
//! ```rust
//! use api_bones::org_id::OrgId;
//! use api_bones::header_id::HeaderId;
//!
//! let id = OrgId::generate();
//! assert_eq!(id.inner().get_version_num(), 4);
//! assert_eq!(OrgId::HEADER_NAME, "X-Org-Id");
//! ```

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::string::{String, ToString};
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;
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
// Header parser (non-extractor; for callers without an AuthLayer)
// ---------------------------------------------------------------------------

/// Error returned by [`OrgId::try_from_headers`].
#[cfg(feature = "http")]
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum OrgIdHeaderError {
    /// The `X-Org-Id` header was not present.
    Missing,
    /// The `X-Org-Id` header value was not valid UTF-8.
    NotUtf8,
    /// The `X-Org-Id` header value was not a valid UUID.
    Invalid(OrgIdError),
}

#[cfg(feature = "http")]
impl fmt::Display for OrgIdHeaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => write!(f, "missing required header: X-Org-Id"),
            Self::NotUtf8 => write!(f, "header X-Org-Id contains non-UTF-8 bytes"),
            Self::Invalid(e) => write!(f, "invalid X-Org-Id: {e}"),
        }
    }
}

#[cfg(all(feature = "http", feature = "std"))]
impl std::error::Error for OrgIdHeaderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Invalid(e) => Some(e),
            Self::Missing | Self::NotUtf8 => None,
        }
    }
}

#[cfg(feature = "http")]
impl OrgId {
    /// Parse an [`OrgId`] from the `X-Org-Id` entry of an [`http::HeaderMap`].
    ///
    /// This parser is intended for callers that do **not** run behind an
    /// `AuthLayer` — e.g. webhook-signature verifiers, out-of-band tooling —
    /// and therefore cannot use [`OrganizationContext`]. Handlers served by
    /// axum routers must consume `OrganizationContext` instead, per
    /// ADR platform/0015.
    ///
    /// If the header carries multiple values, the first one wins (standard
    /// [`http::HeaderMap::get`] semantics).
    ///
    /// # Errors
    ///
    /// - [`OrgIdHeaderError::Missing`] — no `X-Org-Id` header on the map
    /// - [`OrgIdHeaderError::NotUtf8`] — header value contains bytes outside ASCII/UTF-8
    /// - [`OrgIdHeaderError::Invalid`] — header value is not a well-formed UUID
    ///
    /// # Examples
    ///
    /// ```
    /// use api_bones::org_id::OrgId;
    /// use http::HeaderMap;
    ///
    /// let mut headers = HeaderMap::new();
    /// headers.insert("x-org-id", "550e8400-e29b-41d4-a716-446655440000".parse().unwrap());
    /// let id = OrgId::try_from_headers(&headers).unwrap();
    /// assert_eq!(id.to_string(), "550e8400-e29b-41d4-a716-446655440000");
    /// ```
    pub fn try_from_headers(headers: &http::HeaderMap) -> Result<Self, OrgIdHeaderError> {
        let raw = headers
            .get("x-org-id")
            .ok_or(OrgIdHeaderError::Missing)?
            .to_str()
            .map_err(|_| OrgIdHeaderError::NotUtf8)?;
        raw.parse::<Self>().map_err(OrgIdHeaderError::Invalid)
    }
}

// ---------------------------------------------------------------------------
// OrgPath — X-Org-Path header newtype
// ---------------------------------------------------------------------------

/// An ordered org-path (root to self, inclusive), transported via `X-Org-Path`.
///
/// Wire format: comma-separated UUID strings, e.g.
/// `"550e8400-e29b-41d4-a716-446655440000,660e8400-e29b-41d4-a716-446655440001"`.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize), serde(transparent))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "utoipa", schema(value_type = String))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct OrgPath(Vec<OrgId>);

impl OrgPath {
    /// Construct from a vec of `OrgId`s.
    #[must_use]
    pub fn new(path: Vec<OrgId>) -> Self {
        Self(path)
    }

    /// Borrow the path as a slice.
    #[must_use]
    pub fn as_slice(&self) -> &[OrgId] {
        &self.0
    }

    /// Consume and return the inner Vec.
    #[must_use]
    pub fn into_inner(self) -> Vec<OrgId> {
        self.0
    }
}

#[cfg(feature = "std")]
impl crate::header_id::HeaderId for OrgPath {
    const HEADER_NAME: &'static str = "X-Org-Path";
    fn as_str(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Owned(
            self.0
                .iter()
                .map(std::string::ToString::to_string)
                .collect::<Vec<_>>()
                .join(","),
        )
    }
}

#[cfg(all(not(feature = "std"), feature = "alloc"))]
impl crate::header_id::HeaderId for OrgPath {
    const HEADER_NAME: &'static str = "X-Org-Path";
    fn as_str(&self) -> alloc::borrow::Cow<'_, str> {
        alloc::borrow::Cow::Owned(
            self.0
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join(","),
        )
    }
}

/// Parse `OrgPath` from a comma-separated UUID header value.
impl FromStr for OrgPath {
    type Err = OrgIdError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            return Ok(Self(Vec::new()));
        }
        s.split(',')
            .map(|part| part.trim().parse::<OrgId>())
            .collect::<Result<Vec<_>, _>>()
            .map(Self)
    }
}

#[cfg(feature = "axum")]
impl<S: Send + Sync> axum::extract::FromRequestParts<S> for OrgPath {
    type Rejection = crate::error::ApiError;
    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        let raw = parts
            .headers
            .get("x-org-path")
            .ok_or_else(|| {
                crate::error::ApiError::bad_request("missing required header: x-org-path")
            })?
            .to_str()
            .map_err(|_| {
                crate::error::ApiError::bad_request("header x-org-path contains non-UTF-8 bytes")
            })?;
        raw.parse::<Self>()
            .map_err(|e| crate::error::ApiError::bad_request(format!("invalid X-Org-Path: {e}")))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::header_id::HeaderId as _;

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

    #[cfg(all(feature = "http", not(miri)))]
    #[test]
    fn try_from_headers_valid() {
        use http::HeaderMap;
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-org-id",
            "550e8400-e29b-41d4-a716-446655440000".parse().unwrap(),
        );
        let id = OrgId::try_from_headers(&headers).unwrap();
        assert_eq!(id.to_string(), "550e8400-e29b-41d4-a716-446655440000");
    }

    #[cfg(all(feature = "http", not(miri)))]
    #[test]
    fn try_from_headers_malformed() {
        use http::HeaderMap;
        let mut headers = HeaderMap::new();
        headers.insert("x-org-id", "not-a-uuid".parse().unwrap());
        let result = OrgId::try_from_headers(&headers);
        assert!(matches!(result, Err(OrgIdHeaderError::Invalid(_))));
    }

    #[cfg(all(feature = "http", not(miri)))]
    #[test]
    fn try_from_headers_missing() {
        use http::HeaderMap;
        let headers = HeaderMap::new();
        let result = OrgId::try_from_headers(&headers);
        assert_eq!(result, Err(OrgIdHeaderError::Missing));
    }

    #[cfg(all(feature = "http", not(miri)))]
    #[test]
    fn try_from_headers_empty() {
        use http::HeaderMap;
        let mut headers = HeaderMap::new();
        headers.insert("x-org-id", "".parse().unwrap());
        let result = OrgId::try_from_headers(&headers);
        assert!(matches!(result, Err(OrgIdHeaderError::Invalid(_))));
    }

    #[cfg(all(feature = "http", not(miri)))]
    #[test]
    fn try_from_headers_multiple_values_uses_first() {
        use http::HeaderMap;
        let mut headers = HeaderMap::new();
        headers.append(
            "x-org-id",
            "550e8400-e29b-41d4-a716-446655440000".parse().unwrap(),
        );
        headers.append(
            "x-org-id",
            "660e8400-e29b-41d4-a716-446655440001".parse().unwrap(),
        );
        let id = OrgId::try_from_headers(&headers).unwrap();
        assert_eq!(id.to_string(), "550e8400-e29b-41d4-a716-446655440000");
    }

    #[cfg(all(feature = "http", not(miri)))]
    #[test]
    fn try_from_headers_non_utf8() {
        use http::{HeaderMap, HeaderValue};
        let mut headers = HeaderMap::new();
        headers.insert("x-org-id", HeaderValue::from_bytes(&[0xFF, 0xFE]).unwrap());
        let result = OrgId::try_from_headers(&headers);
        assert_eq!(result, Err(OrgIdHeaderError::NotUtf8));
    }

    #[cfg(all(feature = "http", not(miri)))]
    #[test]
    fn try_from_headers_error_display_missing() {
        let err = OrgIdHeaderError::Missing;
        let s = err.to_string();
        assert!(s.contains("missing"));
        assert!(s.contains("X-Org-Id"));
    }

    #[cfg(all(feature = "http", not(miri)))]
    #[test]
    fn try_from_headers_error_display_not_utf8() {
        let err = OrgIdHeaderError::NotUtf8;
        let s = err.to_string();
        assert!(s.contains("non-UTF-8"));
    }

    #[cfg(all(feature = "http", not(miri)))]
    #[test]
    fn try_from_headers_error_display_invalid() {
        let err = OrgIdHeaderError::Invalid(OrgIdError::InvalidUuid(
            uuid::Uuid::parse_str("not-a-uuid").unwrap_err(),
        ));
        let s = err.to_string();
        assert!(s.contains("invalid"));
    }

    #[cfg(all(feature = "http", feature = "std", not(miri)))]
    #[test]
    fn try_from_headers_error_source_for_invalid() {
        use std::error::Error as _;
        let err = OrgIdHeaderError::Invalid(OrgIdError::InvalidUuid(
            uuid::Uuid::parse_str("not-a-uuid").unwrap_err(),
        ));
        assert!(err.source().is_some());
    }

    #[cfg(all(feature = "http", feature = "std", not(miri)))]
    #[test]
    fn try_from_headers_error_source_for_missing() {
        use std::error::Error as _;
        let err = OrgIdHeaderError::Missing;
        assert!(err.source().is_none());
    }

    // -- OrgPath tests -------------------------------------------------------

    #[test]
    fn org_path_new_and_as_slice() {
        let id1 = OrgId::generate();
        let id2 = OrgId::generate();
        let path = OrgPath::new(vec![id1, id2]);
        assert_eq!(path.as_slice().len(), 2);
        assert_eq!(path.as_slice()[0], id1);
        assert_eq!(path.as_slice()[1], id2);
    }

    #[test]
    fn org_path_into_inner() {
        let id = OrgId::generate();
        let path = OrgPath::new(vec![id]);
        let inner = path.into_inner();
        assert_eq!(inner.len(), 1);
        assert_eq!(inner[0], id);
    }

    #[test]
    fn org_path_header_name() {
        use crate::header_id::HeaderId as _;
        assert_eq!(OrgPath::HEADER_NAME, "X-Org-Path");
    }

    #[test]
    fn org_path_header_as_str_empty() {
        let path = OrgPath::new(Vec::new());
        assert_eq!(path.as_str().as_ref(), "");
    }

    #[test]
    fn org_path_header_as_str_single() {
        let id = OrgId::new(uuid::Uuid::nil());
        let path = OrgPath::new(vec![id]);
        assert_eq!(
            path.as_str().as_ref(),
            "00000000-0000-0000-0000-000000000000"
        );
    }

    #[test]
    fn org_path_header_as_str_multiple() {
        let id1 = OrgId::new(uuid::Uuid::nil());
        let id2 = OrgId::generate();
        let path = OrgPath::new(vec![id1, id2]);
        let s = path.as_str();
        assert!(s.as_ref().contains("00000000-0000-0000-0000-000000000000"));
        assert!(s.as_ref().contains(','));
    }

    #[test]
    fn org_path_from_str_empty() {
        let path: OrgPath = "".parse().unwrap();
        assert!(path.as_slice().is_empty());
    }

    #[test]
    fn org_path_from_str_single() {
        let s = "550e8400-e29b-41d4-a716-446655440000";
        let path: OrgPath = s.parse().unwrap();
        assert_eq!(path.as_slice().len(), 1);
        assert_eq!(path.as_slice()[0].to_string(), s);
    }

    #[test]
    fn org_path_from_str_multiple() {
        let s = "550e8400-e29b-41d4-a716-446655440000,660e8400-e29b-41d4-a716-446655440001";
        let path: OrgPath = s.parse().unwrap();
        assert_eq!(path.as_slice().len(), 2);
    }

    #[test]
    fn org_path_from_str_invalid() {
        let result: Result<OrgPath, _> = "not-a-uuid".parse();
        assert!(result.is_err());
    }

    #[cfg(feature = "axum")]
    #[tokio::test]
    async fn org_path_axum_extractor_valid() {
        use axum::extract::FromRequestParts;
        use axum::http::Request;

        let uuid_str = "550e8400-e29b-41d4-a716-446655440000";
        let req = Request::builder()
            .header("x-org-path", uuid_str)
            .body(())
            .unwrap();
        let (mut parts, ()) = req.into_parts();
        let path = OrgPath::from_request_parts(&mut parts, &()).await.unwrap();
        assert_eq!(path.as_slice().len(), 1);
        assert_eq!(path.as_slice()[0].to_string(), uuid_str);
    }

    #[cfg(feature = "axum")]
    #[tokio::test]
    async fn org_path_axum_extractor_missing_header() {
        use axum::extract::FromRequestParts;
        use axum::http::Request;

        let req = Request::builder().body(()).unwrap();
        let (mut parts, ()) = req.into_parts();
        let result = OrgPath::from_request_parts(&mut parts, &()).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status, 400);
    }

    #[cfg(feature = "axum")]
    #[tokio::test]
    async fn org_path_axum_extractor_invalid_uuid() {
        use axum::extract::FromRequestParts;
        use axum::http::Request;

        let req = Request::builder()
            .header("x-org-path", "not-a-uuid")
            .body(())
            .unwrap();
        let (mut parts, ()) = req.into_parts();
        let result = OrgPath::from_request_parts(&mut parts, &()).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status, 400);
    }

    #[cfg(feature = "axum")]
    #[tokio::test]
    async fn org_path_axum_extractor_non_utf8_returns_400() {
        use axum::extract::FromRequestParts;
        use axum::http::{Request, header::HeaderValue};

        let mut req = Request::builder().body(()).unwrap();
        req.headers_mut().insert(
            "x-org-path",
            HeaderValue::from_bytes(&[0xFF, 0xFE]).unwrap(),
        );
        let (mut parts, ()) = req.into_parts();
        let result = OrgPath::from_request_parts(&mut parts, &()).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status, 400);
    }
}
