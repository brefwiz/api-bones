//! Axum extractors for common API request metadata.
//!
//! Most extractors are implemented directly on the core types via
//! [`axum::extract::FromRequestParts`] when the `axum` feature is enabled:
//!
//! | Core type                              | Source header / query        | Rejection |
//! |----------------------------------------|------------------------------|-----------|
//! | [`crate::request_id::RequestId`]       | `X-Request-Id`               | 400       |
//! | [`crate::idempotency::IdempotencyKey`] | `Idempotency-Key`            | 400       |
//! | [`crate::version::ApiVersion`]         | `X-Api-Version` or `?v=`     | 400       |
//!
//! This module adds [`Authorization`], which has no core-type equivalent.
//!
//! Feature gate: `axum` (implies `http` + `serde`).
//!
//! # Example
//!
//! ```rust,no_run
//! use api_bones::request_id::RequestId;
//! use api_bones::idempotency::IdempotencyKey;
//! use api_bones::version::ApiVersion;
//! use api_bones::axum_extractors::Authorization;
//! use api_bones::ApiError;
//! use axum::Router;
//! use axum::routing::post;
//!
//! async fn create(
//!     request_id: RequestId,
//!     idem: IdempotencyKey,
//!     version: ApiVersion,
//!     auth: Authorization,
//! ) -> Result<String, ApiError> {
//!     auth.require_scheme("Bearer")?;
//!     Ok(format!("{} {} {}", request_id, idem, version))
//! }
//!
//! let _app: Router = Router::new().route("/", post(create));
//! ```

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::string::String;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use crate::error::ApiError;

// ---------------------------------------------------------------------------
// Authorization
// ---------------------------------------------------------------------------

/// Typed `Authorization` header extractor.
///
/// The scheme is parsed out of the header value. A request like
/// `Authorization: Bearer <token>` yields `Authorization { scheme: "Bearer",
/// credentials: "<token>" }`.
///
/// Rejects with `401 Unauthorized` when the header is missing or malformed.
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
    use crate::idempotency::IdempotencyKey;
    use crate::request_id::RequestId;
    use crate::version::ApiVersion;
    use axum::extract::FromRequestParts;
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
        let rid = extract_request_id(&[("x-request-id", "550e8400-e29b-41d4-a716-446655440000")])
            .await
            .unwrap();
        assert_eq!(rid.to_string(), "550e8400-e29b-41d4-a716-446655440000");
    }

    #[tokio::test]
    async fn request_id_missing_rejects_400() {
        let err = extract_request_id(&[]).await.unwrap_err();
        assert_eq!(err.status, 400);
    }

    #[tokio::test]
    async fn request_id_invalid_uuid_rejects_400() {
        let err = extract_request_id(&[("x-request-id", "not-a-uuid")])
            .await
            .unwrap_err();
        assert_eq!(err.status, 400);
    }

    #[tokio::test]
    async fn request_id_deref_to_inner() {
        let rid = extract_request_id(&[("x-request-id", "550e8400-e29b-41d4-a716-446655440000")])
            .await
            .unwrap();
        assert_eq!(rid.as_uuid().get_version_num(), 4);
    }

    #[tokio::test]
    async fn idempotency_key_present() {
        let key = extract_idempotency(&[("idempotency-key", "key-xyz")])
            .await
            .unwrap();
        assert_eq!(key.as_str(), "key-xyz");
    }

    #[tokio::test]
    async fn idempotency_key_missing_rejects_400() {
        let err = extract_idempotency(&[]).await.unwrap_err();
        assert_eq!(err.status, 400);
    }

    #[tokio::test]
    async fn idempotency_key_too_long_rejects_400() {
        let long_key = "a".repeat(256);
        let err = extract_idempotency(&[("idempotency-key", long_key.as_str())])
            .await
            .unwrap_err();
        assert_eq!(err.status, 400);
    }

    #[tokio::test]
    async fn api_version_from_header() {
        let v = extract_version("/", &[("x-api-version", "v2")])
            .await
            .unwrap();
        assert_eq!(v, ApiVersion::Simple(2));
    }

    #[tokio::test]
    async fn api_version_from_query() {
        let v = extract_version("/?v=v3", &[]).await.unwrap();
        assert_eq!(v, ApiVersion::Simple(3));
    }

    #[tokio::test]
    async fn api_version_header_takes_precedence() {
        let v = extract_version("/?v=v3", &[("x-api-version", "v2")])
            .await
            .unwrap();
        assert_eq!(v, ApiVersion::Simple(2));
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
        let rid = extract_request_id(&[("x-request-id", "550e8400-e29b-41d4-a716-446655440000")])
            .await
            .unwrap();
        assert_eq!(rid.to_string(), "550e8400-e29b-41d4-a716-446655440000");
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
    async fn request_id_non_utf8_rejects_400() {
        use axum::http::{Request, header::HeaderValue};
        let bad_val = HeaderValue::from_bytes(b"\xff\xfe").unwrap();
        let req = Request::builder().uri("/").body(()).unwrap();
        let (mut parts, ()) = req.into_parts();
        parts.headers.insert("x-request-id", bad_val);
        let err = RequestId::from_request_parts(&mut parts, &())
            .await
            .unwrap_err();
        assert_eq!(err.status, 400);
    }

    #[tokio::test]
    async fn idempotency_key_non_utf8_rejects_400() {
        use axum::http::{Request, header::HeaderValue};
        let bad_val = HeaderValue::from_bytes(b"\xff\xfe").unwrap();
        let req = Request::builder().uri("/").body(()).unwrap();
        let (mut parts, ()) = req.into_parts();
        parts.headers.insert("idempotency-key", bad_val);
        let err = IdempotencyKey::from_request_parts(&mut parts, &())
            .await
            .unwrap_err();
        assert_eq!(err.status, 400);
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
        let err = extract_auth(Some(" token-only")).await.unwrap_err();
        assert_eq!(err.status, 401);
    }

    #[tokio::test]
    async fn api_version_from_query_with_preceding_params() {
        let v = extract_version("/?other=foo&v=v5", &[]).await.unwrap();
        assert_eq!(v, ApiVersion::Simple(5));
    }

    #[tokio::test]
    async fn api_version_invalid_header_value_rejects_400() {
        let err = extract_version("/", &[("x-api-version", "not-a-version")])
            .await
            .unwrap_err();
        assert_eq!(err.status, 400);
    }

    #[tokio::test]
    async fn api_version_invalid_query_value_rejects_400() {
        let err = extract_version("/?v=not-a-version", &[]).await.unwrap_err();
        assert_eq!(err.status, 400);
    }
}
