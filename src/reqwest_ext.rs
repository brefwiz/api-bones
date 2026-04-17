//! Reqwest client adapter for api-bones types.
//!
//! Extension traits that enrich [`reqwest::RequestBuilder`] and
//! [`reqwest::Response`] with api-bones conveniences:
//!
//! - [`RequestBuilderExt`] — attach `X-Request-Id`, `Idempotency-Key`, and
//!   custom `Authorization` headers.
//! - [`ResponseExt`] — extract `Problem+JSON` errors, parse
//!   `X-RateLimit-*` headers, and follow RFC 5988 `Link` pagination.
//!
//! Feature gate: `reqwest` (see `Cargo.toml`).
//!
//! # Example
//!
//! ```rust,no_run
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! use api_bones::reqwest_ext::{RequestBuilderExt, ResponseExt};
//!
//! let client = reqwest::Client::new();
//! let response = client
//!     .post("https://api.example.com/orders")
//!     .with_request_id("req-abc-123")
//!     .with_idempotency_key("idem-xyz")
//!     .send()
//!     .await?;
//!
//! let rate_limit = response.rate_limit_info();
//! let next_page = response.next_page_url();
//! let body: serde_json::Value = response.problem_json_or_json().await?;
//! # Ok(())
//! # }
//! ```

use reqwest::{RequestBuilder, Response};

use crate::error::ApiError;
use crate::ratelimit::RateLimitInfo;

// ---------------------------------------------------------------------------
// RequestBuilderExt
// ---------------------------------------------------------------------------

/// Extension methods for [`reqwest::RequestBuilder`].
pub trait RequestBuilderExt: Sized {
    /// Attach an `X-Request-Id` header.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use api_bones::reqwest_ext::RequestBuilderExt;
    ///
    /// let _builder = reqwest::Client::new()
    ///     .get("https://api.example.com/")
    ///     .with_request_id("req-001");
    /// ```
    #[must_use]
    fn with_request_id(self, id: impl AsRef<str>) -> Self;

    /// Attach an `Idempotency-Key` header.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use api_bones::reqwest_ext::RequestBuilderExt;
    ///
    /// let _builder = reqwest::Client::new()
    ///     .post("https://api.example.com/orders")
    ///     .with_idempotency_key("key-unique-123");
    /// ```
    #[must_use]
    fn with_idempotency_key(self, key: impl AsRef<str>) -> Self;

    /// Attach a `Authorization: Bearer <token>` header.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use api_bones::reqwest_ext::RequestBuilderExt;
    ///
    /// let _builder = reqwest::Client::new()
    ///     .get("https://api.example.com/protected")
    ///     .with_bearer_token("my.jwt.token");
    /// ```
    #[must_use]
    fn with_bearer_token(self, token: impl AsRef<str>) -> Self;
}

impl RequestBuilderExt for RequestBuilder {
    fn with_request_id(self, id: impl AsRef<str>) -> Self {
        self.header("x-request-id", id.as_ref())
    }

    fn with_idempotency_key(self, key: impl AsRef<str>) -> Self {
        self.header("idempotency-key", key.as_ref())
    }

    fn with_bearer_token(self, token: impl AsRef<str>) -> Self {
        self.header(
            "authorization",
            format!("Bearer {}", token.as_ref()),
        )
    }
}

// ---------------------------------------------------------------------------
// ResponseExt
// ---------------------------------------------------------------------------

/// Extension methods for [`reqwest::Response`].
pub trait ResponseExt {
    /// Parse `X-RateLimit-*` headers into a [`RateLimitInfo`].
    ///
    /// Returns `None` if the required rate-limit headers are absent or
    /// unparseable.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use api_bones::reqwest_ext::ResponseExt;
    ///
    /// # async fn example() -> reqwest::Result<()> {
    /// let resp = reqwest::get("https://api.example.com/").await?;
    /// if let Some(rl) = resp.rate_limit_info() {
    ///     println!("remaining: {}", rl.remaining);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    fn rate_limit_info(&self) -> Option<RateLimitInfo>;

    /// Parse the RFC 5988 `Link: <url>; rel="next"` header and return the
    /// URL for the next page, if present.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use api_bones::reqwest_ext::ResponseExt;
    ///
    /// # async fn example() -> reqwest::Result<()> {
    /// let resp = reqwest::get("https://api.example.com/items").await?;
    /// if let Some(next) = resp.next_page_url() {
    ///     println!("next: {next}");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    fn next_page_url(&self) -> Option<String>;

    /// Consume the response, returning the deserialized body.
    ///
    /// - If the status code indicates an error (`>= 400`) and the
    ///   `Content-Type` contains `application/problem+json`, the body is
    ///   parsed as an [`ApiError`] and returned as `Err`.
    /// - Otherwise the body JSON is deserialized into `T` and returned as
    ///   `Ok`.
    ///
    /// # Errors
    ///
    /// Returns `Err(ApiError)` for:
    /// - Problem+JSON error responses (`>= 400` with correct content type).
    /// - Non-Problem+JSON error responses (`>= 400`).
    /// - JSON deserialization failures.
    /// - Network / transport errors from reqwest.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use api_bones::reqwest_ext::{RequestBuilderExt, ResponseExt};
    ///
    /// # async fn example() -> Result<(), api_bones::ApiError> {
    /// let resp = reqwest::Client::new()
    ///     .get("https://api.example.com/items")
    ///     .send()
    ///     .await
    ///     .map_err(|e| api_bones::ApiError::bad_request(e.to_string()))?;
    ///
    /// let body: serde_json::Value = resp.problem_json_or_json().await?;
    /// # Ok(())
    /// # }
    /// ```
    fn problem_json_or_json<T: serde::de::DeserializeOwned>(
        self,
    ) -> impl Future<Output = Result<T, ApiError>> + Send;
}

use core::future::Future;

impl ResponseExt for Response {
    fn rate_limit_info(&self) -> Option<RateLimitInfo> {
        let parse = |name: &str| -> Option<u64> {
            self.headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse().ok())
        };
        let limit = parse("x-ratelimit-limit")?;
        let remaining = parse("x-ratelimit-remaining")?;
        let reset = parse("x-ratelimit-reset")?;
        let retry_after = parse("retry-after");
        Some(RateLimitInfo {
            limit,
            remaining,
            reset,
            retry_after,
        })
    }

    fn next_page_url(&self) -> Option<String> {
        // Parse Link header(s): `<url>; rel="next"`
        for link_val in self.headers().get_all("link") {
            let Ok(s) = link_val.to_str() else {
                continue;
            };
            // Each Link header may contain comma-separated entries.
            for entry in s.split(',') {
                let entry = entry.trim();
                // A link entry looks like: <https://...>; rel="next"
                if let Some(url) = parse_link_next(entry) {
                    return Some(url);
                }
            }
        }
        None
    }

    async fn problem_json_or_json<T: serde::de::DeserializeOwned>(self) -> Result<T, ApiError> {
        let status = self.status();
        if status.is_client_error() || status.is_server_error() {
            let is_problem = self
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .map(|ct| ct.contains("application/problem+json"))
                .unwrap_or(false);

            if is_problem {
                // Attempt to parse as ProblemJson and convert to ApiError.
                let body: serde_json::Value = self
                    .json()
                    .await
                    .map_err(|e| ApiError::bad_request(e.to_string()))?;
                let detail = body
                    .get("detail")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown error")
                    .to_owned();
                let code_status = body
                    .get("status")
                    .and_then(|v| v.as_u64())
                    .and_then(|s| u16::try_from(s).ok())
                    .unwrap_or(status.as_u16());
                return Err(map_status_to_api_error(code_status, detail));
            }

            return Err(ApiError::new(
                crate::error::ErrorCode::InternalServerError,
                format!("HTTP {}", status.as_u16()),
            ));
        }

        self.json::<T>()
            .await
            .map_err(|e| ApiError::bad_request(e.to_string()))
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn parse_link_next(entry: &str) -> Option<String> {
    // Split on `;`: first part is `<url>`, rest are params.
    let mut parts = entry.split(';');
    let url_part = parts.next()?.trim();
    let url = url_part
        .strip_prefix('<')
        .and_then(|s| s.strip_suffix('>'))?;

    let is_next = parts.any(|p| {
        let p = p.trim();
        // Match rel="next" or rel=next
        p == "rel=\"next\"" || p == "rel=next"
    });

    if is_next {
        Some(url.to_owned())
    } else {
        None
    }
}

fn map_status_to_api_error(status: u16, detail: String) -> ApiError {
    use crate::error::ErrorCode;
    let code = match status {
        400 => ErrorCode::BadRequest,
        401 => ErrorCode::Unauthorized,
        403 => ErrorCode::Forbidden,
        404 => ErrorCode::ResourceNotFound,
        409 => ErrorCode::Conflict,
        422 => ErrorCode::UnprocessableEntity,
        429 => ErrorCode::RateLimited,
        500 => ErrorCode::InternalServerError,
        502 => ErrorCode::BadGateway,
        503 => ErrorCode::ServiceUnavailable,
        504 => ErrorCode::GatewayTimeout,
        _ if status >= 500 => ErrorCode::InternalServerError,
        _ => ErrorCode::BadRequest,
    };
    ApiError::new(code, detail)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_link_next_basic() {
        let entry = r#"<https://api.example.com/items?after=abc>; rel="next""#;
        assert_eq!(
            parse_link_next(entry),
            Some("https://api.example.com/items?after=abc".to_owned())
        );
    }

    #[test]
    fn parse_link_next_no_match() {
        let entry = r#"<https://api.example.com/items?before=abc>; rel="prev""#;
        assert!(parse_link_next(entry).is_none());
    }

    #[test]
    fn parse_link_next_unquoted_rel() {
        let entry = "<https://example.com/next>; rel=next";
        assert_eq!(
            parse_link_next(entry),
            Some("https://example.com/next".to_owned())
        );
    }

    #[test]
    fn map_status_400() {
        let err = map_status_to_api_error(400, "bad".into());
        assert_eq!(err.status, 400);
    }

    #[test]
    fn map_status_404() {
        let err = map_status_to_api_error(404, "not found".into());
        assert_eq!(err.status, 404);
    }

    #[test]
    fn map_status_unknown_5xx() {
        let err = map_status_to_api_error(599, "oops".into());
        assert_eq!(err.status, 500);
    }
}
