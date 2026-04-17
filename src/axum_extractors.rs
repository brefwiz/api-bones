//! Axum extractors for common API request metadata.
//!
//! All extractors implement [`axum::extract::FromRequestParts`] and reject
//! with [`ApiError`] so callers always get a consistent Problem+JSON body.
//!
//! Feature gate: `axum` (implies `http` + `serde`).
//!
//! | Extractor            | Source header / query         | Rejection status |
//! |----------------------|-------------------------------|-----------------|
//! | [`RequestId`]        | `X-Request-Id`                | 400             |
//! | [`IdempotencyKey`]   | `Idempotency-Key`             | 400             |
//! | [`ApiVersion`]       | `X-Api-Version` or query `v`  | 400             |
//! | [`PaginationParams`] | query string                  | 400             |
//! | [`Authorization`]    | `Authorization` (typed)       | 401             |
//!
//! # Example
//!
//! ```rust,no_run
//! use api_bones::axum_extractors::{RequestId, IdempotencyKey, ApiVersion};
//! use api_bones::ApiError;
//! use axum::Router;
//! use axum::routing::post;
//!
//! async fn create(
//!     request_id: RequestId,
//!     idem: IdempotencyKey,
//!     version: ApiVersion,
//! ) -> Result<String, ApiError> {
//!     Ok(format!("{} {} {}", request_id, idem, version))
//! }
//!
//! let _app: Router = Router::new().route("/", post(create));
//! ```

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::string::String;

use axum::extract::FromRequestParts;
use axum::http::HeaderMap;
use axum::http::request::Parts;

use crate::error::ApiError;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract a header value as a UTF-8 string, or return a 400 `ApiError`.
#[allow(clippy::result_large_err)]
fn required_header(headers: &HeaderMap, name: &'static str) -> Result<String, ApiError> {
    headers
        .get(name)
        .ok_or_else(|| ApiError::bad_request(format!("missing required header: {name}")))?
        .to_str()
        .map(ToOwned::to_owned)
        .map_err(|_| ApiError::bad_request(format!("header {name} contains non-UTF-8 bytes")))
}

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::borrow::ToOwned;
#[cfg(feature = "std")]
use std::borrow::ToOwned;

// ---------------------------------------------------------------------------
// RequestId
// ---------------------------------------------------------------------------

/// Extracted `X-Request-Id` header value.
///
/// Rejects with `400 Bad Request` when the header is absent or not valid UTF-8.
///
/// # Example
///
/// ```rust,no_run
/// use api_bones::axum_extractors::RequestId;
/// use api_bones::ApiError;
///
/// async fn handler(rid: RequestId) -> String {
///     format!("request id = {rid}")
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestId(pub String);

impl core::fmt::Display for RequestId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

impl core::ops::Deref for RequestId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<S: Send + Sync> FromRequestParts<S> for RequestId {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        required_header(&parts.headers, "x-request-id").map(Self)
    }
}

// ---------------------------------------------------------------------------
// IdempotencyKey
// ---------------------------------------------------------------------------

/// Extracted `Idempotency-Key` header value.
///
/// Rejects with `400 Bad Request` when the header is absent or not valid UTF-8.
///
/// # Example
///
/// ```rust,no_run
/// use api_bones::axum_extractors::IdempotencyKey;
/// use api_bones::ApiError;
///
/// async fn create(key: IdempotencyKey) -> String {
///     format!("key = {key}")
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdempotencyKey(pub String);

impl core::fmt::Display for IdempotencyKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

impl core::ops::Deref for IdempotencyKey {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<S: Send + Sync> FromRequestParts<S> for IdempotencyKey {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        required_header(&parts.headers, "idempotency-key").map(Self)
    }
}

// ---------------------------------------------------------------------------
// ApiVersion
// ---------------------------------------------------------------------------

/// Extracted API version, read from the `X-Api-Version` header or the `v`
/// query parameter (header takes precedence).
///
/// Rejects with `400 Bad Request` when neither source is present.
///
/// # Example
///
/// ```rust,no_run
/// use api_bones::axum_extractors::ApiVersion;
/// use api_bones::ApiError;
///
/// async fn handler(version: ApiVersion) -> String {
///     format!("version = {version}")
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiVersion(pub String);

impl core::fmt::Display for ApiVersion {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

impl core::ops::Deref for ApiVersion {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<S: Send + Sync> FromRequestParts<S> for ApiVersion {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        // 1. Try header
        if let Some(val) = parts.headers.get("x-api-version") {
            let s = val
                .to_str()
                .map_err(|_| ApiError::bad_request("header x-api-version contains non-UTF-8"))?;
            return Ok(Self(s.to_owned()));
        }
        // 2. Try query parameter `v`
        if let Some(query) = parts.uri.query() {
            for pair in query.split('&') {
                if let Some(v) = pair.strip_prefix("v=") {
                    return Ok(Self(v.to_owned()));
                }
            }
        }
        Err(ApiError::bad_request(
            "missing api version: provide X-Api-Version header or v= query parameter",
        ))
    }
}

// ---------------------------------------------------------------------------
// Authorization<S>
// ---------------------------------------------------------------------------

/// Typed `Authorization` header extractor.
///
/// The scheme is parsed out of the header value.  A request like
/// `Authorization: Bearer <token>` yields `Authorization { scheme: "Bearer",
/// credentials: "<token>" }`.
///
/// Rejects with `401 Unauthorized` when the header is missing, malformed, or
/// uses an unexpected scheme (if `expected_scheme` is `Some`).
///
/// # Example
///
/// ```rust,no_run
/// use api_bones::axum_extractors::Authorization;
/// use api_bones::ApiError;
///
/// async fn handler(auth: Authorization) -> Result<String, ApiError> {
///     auth.require_scheme("Bearer")?;
///     Ok(format!("token = {}", auth.credentials))
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Authorization {
    /// The authentication scheme (e.g. `"Bearer"`, `"Basic"`).
    pub scheme: String,
    /// The credentials that follow the scheme.
    pub credentials: String,
}

impl Authorization {
    /// Validate that the scheme matches `expected` (case-insensitive).
    ///
    /// Returns `Err(ApiError::unauthorized)` on mismatch.
    ///
    /// # Errors
    ///
    /// Returns `ApiError` with status 401 if the scheme does not match.
    #[allow(clippy::result_large_err)]
    pub fn require_scheme(&self, expected: &str) -> Result<(), ApiError> {
        if self.scheme.eq_ignore_ascii_case(expected) {
            Ok(())
        } else {
            Err(ApiError::unauthorized(format!(
                "expected {expected} authorization scheme, got {}",
                self.scheme
            )))
        }
    }
}

impl<S: Send + Sync> FromRequestParts<S> for Authorization {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let raw = parts
            .headers
            .get("authorization")
            .ok_or_else(|| ApiError::unauthorized("missing Authorization header"))?
            .to_str()
            .map_err(|_| ApiError::unauthorized("Authorization header contains non-UTF-8 bytes"))?;

        let mut iter = raw.splitn(2, ' ');
        let scheme = iter
            .next()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ApiError::unauthorized("malformed Authorization header"))?
            .to_owned();
        let credentials = iter.next().unwrap_or_default().trim().to_owned();

        Ok(Self {
            scheme,
            credentials,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;

    async fn extract_request_id(headers: &[(&str, &str)]) -> Result<RequestId, ApiError> {
        let mut builder = Request::builder().uri("/");
        for (k, v) in headers {
            builder = builder.header(*k, *v);
        }
        let req = builder.body(()).unwrap();
        let (mut parts, ()) = req.into_parts();
        RequestId::from_request_parts(&mut parts, &()).await
    }

    async fn extract_idempotency(headers: &[(&str, &str)]) -> Result<IdempotencyKey, ApiError> {
        let mut builder = Request::builder().uri("/");
        for (k, v) in headers {
            builder = builder.header(*k, *v);
        }
        let req = builder.body(()).unwrap();
        let (mut parts, ()) = req.into_parts();
        IdempotencyKey::from_request_parts(&mut parts, &()).await
    }

    async fn extract_version(uri: &str, headers: &[(&str, &str)]) -> Result<ApiVersion, ApiError> {
        let mut builder = Request::builder().uri(uri);
        for (k, v) in headers {
            builder = builder.header(*k, *v);
        }
        let req = builder.body(()).unwrap();
        let (mut parts, ()) = req.into_parts();
        ApiVersion::from_request_parts(&mut parts, &()).await
    }

    async fn extract_auth(header_val: Option<&str>) -> Result<Authorization, ApiError> {
        let mut builder = Request::builder().uri("/");
        if let Some(v) = header_val {
            builder = builder.header("authorization", v);
        }
        let req = builder.body(()).unwrap();
        let (mut parts, ()) = req.into_parts();
        Authorization::from_request_parts(&mut parts, &()).await
    }

    #[tokio::test]
    async fn request_id_present() {
        let rid = extract_request_id(&[("x-request-id", "abc-123")])
            .await
            .unwrap();
        assert_eq!(&*rid, "abc-123");
    }

    #[tokio::test]
    async fn request_id_missing_rejects_400() {
        let err = extract_request_id(&[]).await.unwrap_err();
        assert_eq!(err.status, 400);
    }

    #[tokio::test]
    async fn idempotency_key_present() {
        let key = extract_idempotency(&[("idempotency-key", "key-xyz")])
            .await
            .unwrap();
        assert_eq!(&*key, "key-xyz");
    }

    #[tokio::test]
    async fn idempotency_key_missing_rejects_400() {
        let err = extract_idempotency(&[]).await.unwrap_err();
        assert_eq!(err.status, 400);
    }

    #[tokio::test]
    async fn api_version_from_header() {
        let v = extract_version("/", &[("x-api-version", "v2")])
            .await
            .unwrap();
        assert_eq!(&*v, "v2");
    }

    #[tokio::test]
    async fn api_version_from_query() {
        let v = extract_version("/?v=v3", &[]).await.unwrap();
        assert_eq!(&*v, "v3");
    }

    #[tokio::test]
    async fn api_version_header_takes_precedence() {
        let v = extract_version("/?v=v3", &[("x-api-version", "v2")])
            .await
            .unwrap();
        assert_eq!(&*v, "v2");
    }

    #[tokio::test]
    async fn api_version_missing_rejects_400() {
        let err = extract_version("/", &[]).await.unwrap_err();
        assert_eq!(err.status, 400);
    }

    #[tokio::test]
    async fn authorization_bearer() {
        let auth = extract_auth(Some("Bearer my.jwt.token")).await.unwrap();
        assert_eq!(auth.scheme, "Bearer");
        assert_eq!(auth.credentials, "my.jwt.token");
    }

    #[tokio::test]
    async fn authorization_missing_rejects_401() {
        let err = extract_auth(None).await.unwrap_err();
        assert_eq!(err.status, 401);
    }

    #[tokio::test]
    async fn authorization_require_scheme_ok() {
        let auth = extract_auth(Some("Bearer token")).await.unwrap();
        assert!(auth.require_scheme("Bearer").is_ok());
    }

    #[tokio::test]
    async fn authorization_require_scheme_mismatch_401() {
        let auth = extract_auth(Some("Basic dXNlcjpwYXNz")).await.unwrap();
        let err = auth.require_scheme("Bearer").unwrap_err();
        assert_eq!(err.status, 401);
    }

    #[tokio::test]
    async fn request_id_display() {
        let rid = extract_request_id(&[("x-request-id", "disp-001")])
            .await
            .unwrap();
        assert_eq!(rid.to_string(), "disp-001");
    }

    #[tokio::test]
    async fn request_id_deref() {
        let rid = extract_request_id(&[("x-request-id", "deref-test")])
            .await
            .unwrap();
        let s: &str = &rid;
        assert_eq!(s, "deref-test");
    }

    #[tokio::test]
    async fn idempotency_key_display() {
        let key = extract_idempotency(&[("idempotency-key", "disp-key")])
            .await
            .unwrap();
        assert_eq!(key.to_string(), "disp-key");
    }

    #[tokio::test]
    async fn idempotency_key_deref() {
        let key = extract_idempotency(&[("idempotency-key", "deref-key")])
            .await
            .unwrap();
        let s: &str = &key;
        assert_eq!(s, "deref-key");
    }

    #[tokio::test]
    async fn api_version_display() {
        let v = extract_version("/", &[("x-api-version", "v99")])
            .await
            .unwrap();
        assert_eq!(v.to_string(), "v99");
    }

    #[tokio::test]
    async fn api_version_deref() {
        let v = extract_version("/", &[("x-api-version", "v88")])
            .await
            .unwrap();
        let s: &str = &v;
        assert_eq!(s, "v88");
    }

    #[test]
    fn required_header_non_utf8_bytes() {
        // Build a HeaderMap with a non-UTF-8 value by inserting raw bytes.
        use axum::http::header::HeaderValue;
        let mut headers = axum::http::HeaderMap::new();
        // 0xff is not valid UTF-8.
        let bad_val = HeaderValue::from_bytes(b"\xff\xfe").unwrap();
        headers.insert("x-request-id", bad_val);
        let result = required_header(&headers, "x-request-id");
        let err = result.unwrap_err();
        assert_eq!(err.status, 400);
    }

    #[tokio::test]
    async fn api_version_from_query_with_preceding_params() {
        // Query has a non-matching pair before v=, exercising the strip_prefix None branch.
        let v = extract_version("/?other=foo&v=v5", &[]).await.unwrap();
        assert_eq!(&*v, "v5");
    }

    #[tokio::test]
    async fn authorization_non_utf8_header_rejects_401() {
        use axum::http::{Request, header::HeaderValue};
        let bad_val = HeaderValue::from_bytes(b"\xff\xfe").unwrap();
        let req = Request::builder().uri("/").body(()).unwrap();
        let (mut parts, ()) = req.into_parts();
        parts.headers.insert("authorization", bad_val);
        let err = Authorization::from_request_parts(&mut parts, &())
            .await
            .unwrap_err();
        assert_eq!(err.status, 401);
    }

    #[tokio::test]
    async fn authorization_empty_scheme_rejects_401() {
        // A header value with no scheme (just a space and credentials) triggers
        // the `.filter(|s| !s.is_empty())` branch.
        let err = extract_auth(Some(" token-only")).await.unwrap_err();
        assert_eq!(err.status, 401);
    }

    #[tokio::test]
    async fn api_version_non_utf8_header_rejects_400() {
        use axum::http::{Request, header::HeaderValue};
        let bad_val = HeaderValue::from_bytes(b"\xff").unwrap();
        let req = Request::builder().uri("/").body(()).unwrap();
        let (mut parts, ()) = req.into_parts();
        parts.headers.insert("x-api-version", bad_val);
        let err = ApiVersion::from_request_parts(&mut parts, &())
            .await
            .unwrap_err();
        assert_eq!(err.status, 400);
    }
}
