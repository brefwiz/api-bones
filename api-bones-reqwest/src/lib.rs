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
//! # Example
//!
//! ```rust,no_run
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! use api_bones_reqwest::{RequestBuilderExt, ResponseExt};
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

use core::future::Future;

use api_bones::{ApiError, RateLimitInfo};
use reqwest::{RequestBuilder, Response};

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
    /// use api_bones_reqwest::RequestBuilderExt;
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
    /// use api_bones_reqwest::RequestBuilderExt;
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
    /// use api_bones_reqwest::RequestBuilderExt;
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
        self.header("authorization", format!("Bearer {}", token.as_ref()))
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
    /// use api_bones_reqwest::ResponseExt;
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
    /// use api_bones_reqwest::ResponseExt;
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
    /// use api_bones_reqwest::{RequestBuilderExt, ResponseExt};
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
        for link_val in self.headers().get_all("link") {
            let Ok(s) = link_val.to_str() else {
                continue;
            };
            for entry in s.split(',') {
                let entry = entry.trim();
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
                .is_some_and(|ct| ct.contains("application/problem+json"));

            if is_problem {
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
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|s| u16::try_from(s).ok())
                    .unwrap_or(status.as_u16());
                return Err(map_status_to_api_error(code_status, detail));
            }

            return Err(ApiError::new(
                api_bones::ErrorCode::InternalServerError,
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
    let mut parts = entry.split(';');
    let url_part = parts.next()?.trim();
    let url = url_part
        .strip_prefix('<')
        .and_then(|s| s.strip_suffix('>'))?;

    let is_next = parts.any(|p| {
        let p = p.trim();
        p == "rel=\"next\"" || p == "rel=next"
    });

    if is_next { Some(url.to_owned()) } else { None }
}

fn map_status_to_api_error(status: u16, detail: String) -> ApiError {
    use api_bones::ErrorCode;
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
#[allow(clippy::significant_drop_tightening)]
mod tests {
    use super::*;

    #[test]
    fn map_status_401() {
        let err = map_status_to_api_error(401, "unauth".into());
        assert_eq!(err.status, 401);
    }

    #[test]
    fn map_status_403() {
        let err = map_status_to_api_error(403, "forbidden".into());
        assert_eq!(err.status, 403);
    }

    #[test]
    fn map_status_409() {
        let err = map_status_to_api_error(409, "conflict".into());
        assert_eq!(err.status, 409);
    }

    #[test]
    fn map_status_422() {
        let err = map_status_to_api_error(422, "unprocessable".into());
        assert_eq!(err.status, 422);
    }

    #[test]
    fn map_status_429() {
        let err = map_status_to_api_error(429, "rate limited".into());
        assert_eq!(err.status, 429);
    }

    #[test]
    fn map_status_500() {
        let err = map_status_to_api_error(500, "ise".into());
        assert_eq!(err.status, 500);
    }

    #[test]
    fn map_status_502() {
        let err = map_status_to_api_error(502, "bad gateway".into());
        assert_eq!(err.status, 502);
    }

    #[test]
    fn map_status_503() {
        let err = map_status_to_api_error(503, "unavailable".into());
        assert_eq!(err.status, 503);
    }

    #[test]
    fn map_status_504() {
        let err = map_status_to_api_error(504, "timeout".into());
        assert_eq!(err.status, 504);
    }

    #[tokio::test]
    async fn request_builder_with_request_id() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/")
            .match_header("x-request-id", "req-abc")
            .with_status(200)
            .with_body("{}")
            .create_async()
            .await;

        let client = reqwest::Client::new();
        let resp = client
            .get(server.url())
            .with_request_id("req-abc")
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 200);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn request_builder_with_idempotency_key() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/")
            .match_header("idempotency-key", "idem-123")
            .with_status(201)
            .with_body("{}")
            .create_async()
            .await;

        let client = reqwest::Client::new();
        let resp = client
            .post(server.url())
            .with_idempotency_key("idem-123")
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 201);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn request_builder_with_bearer_token() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/")
            .match_header("authorization", "Bearer my.token")
            .with_status(200)
            .with_body("{}")
            .create_async()
            .await;

        let client = reqwest::Client::new();
        let resp = client
            .get(server.url())
            .with_bearer_token("my.token")
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 200);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn rate_limit_info_present() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/")
            .with_status(200)
            .with_header("x-ratelimit-limit", "100")
            .with_header("x-ratelimit-remaining", "42")
            .with_header("x-ratelimit-reset", "1700000000")
            .with_header("retry-after", "5")
            .with_body("{}")
            .create_async()
            .await;

        let resp = reqwest::get(server.url()).await.unwrap();
        let rl = resp.rate_limit_info().unwrap();
        assert_eq!(rl.limit, 100);
        assert_eq!(rl.remaining, 42);
        assert_eq!(rl.reset, 1_700_000_000);
        assert_eq!(rl.retry_after, Some(5));
    }

    #[tokio::test]
    async fn rate_limit_info_missing_headers_returns_none() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/")
            .with_status(200)
            .with_body("{}")
            .create_async()
            .await;

        let resp = reqwest::get(server.url()).await.unwrap();
        assert!(resp.rate_limit_info().is_none());
    }

    #[tokio::test]
    async fn rate_limit_info_without_retry_after() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/")
            .with_status(200)
            .with_header("x-ratelimit-limit", "50")
            .with_header("x-ratelimit-remaining", "10")
            .with_header("x-ratelimit-reset", "9999")
            .with_body("{}")
            .create_async()
            .await;

        let resp = reqwest::get(server.url()).await.unwrap();
        let rl = resp.rate_limit_info().unwrap();
        assert_eq!(rl.retry_after, None);
    }

    #[tokio::test]
    async fn next_page_url_present() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/")
            .with_status(200)
            .with_header(
                "link",
                r#"<https://api.example.com/items?after=xyz>; rel="next""#,
            )
            .with_body("[]")
            .create_async()
            .await;

        let resp = reqwest::get(server.url()).await.unwrap();
        assert_eq!(
            resp.next_page_url(),
            Some("https://api.example.com/items?after=xyz".to_owned())
        );
    }

    #[tokio::test]
    async fn next_page_url_absent() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/")
            .with_status(200)
            .with_body("[]")
            .create_async()
            .await;

        let resp = reqwest::get(server.url()).await.unwrap();
        assert!(resp.next_page_url().is_none());
    }

    #[tokio::test]
    async fn problem_json_or_json_success() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"value": 42}"#)
            .create_async()
            .await;

        let resp = reqwest::get(server.url()).await.unwrap();
        let body: serde_json::Value = resp.problem_json_or_json().await.unwrap();
        assert_eq!(body["value"], 42);
    }

    #[tokio::test]
    async fn problem_json_or_json_problem_response() {
        let mut server = mockito::Server::new_async().await;
        let problem_body =
            r#"{"type":"about:blank","title":"Not Found","status":404,"detail":"item missing"}"#;
        let _mock = server
            .mock("GET", "/")
            .with_status(404)
            .with_header("content-type", "application/problem+json")
            .with_body(problem_body)
            .create_async()
            .await;

        let resp = reqwest::get(server.url()).await.unwrap();
        let err: api_bones::ApiError = resp
            .problem_json_or_json::<serde_json::Value>()
            .await
            .unwrap_err();
        assert_eq!(err.status, 404);
    }

    #[tokio::test]
    async fn problem_json_or_json_non_problem_error_response() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/")
            .with_status(500)
            .with_header("content-type", "text/plain")
            .with_body("Internal Server Error")
            .create_async()
            .await;

        let resp = reqwest::get(server.url()).await.unwrap();
        let err: api_bones::ApiError = resp
            .problem_json_or_json::<serde_json::Value>()
            .await
            .unwrap_err();
        assert_eq!(err.status, 500);
    }

    #[test]
    fn map_status_418_defaults_to_bad_request() {
        let err = map_status_to_api_error(418, "teapot".into());
        assert_eq!(err.status, 400);
    }

    #[tokio::test]
    async fn next_page_url_non_utf8_link_header_is_skipped() {
        use tokio::io::AsyncWriteExt;
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = [0u8; 4096];
                let _ = tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await;
                let response: &[u8] =
                    b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nLink: \xff\r\n\r\n[]";
                let _ = stream.write_all(response).await;
            }
        });

        let url = format!("http://{addr}/");
        if let Ok(resp) = reqwest::get(&url).await {
            assert!(resp.next_page_url().is_none());
        }
    }

    #[tokio::test]
    async fn next_page_url_with_only_prev_link() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/")
            .with_status(200)
            .with_header(
                "link",
                r#"<https://api.example.com/items?before=abc>; rel="prev""#,
            )
            .with_body("[]")
            .create_async()
            .await;

        let resp = reqwest::get(server.url()).await.unwrap();
        assert!(resp.next_page_url().is_none());
    }

    #[test]
    fn parse_link_next_empty_entry_returns_none() {
        assert!(parse_link_next("").is_none());
    }

    #[test]
    fn parse_link_next_malformed_url_no_closing_angle_returns_none() {
        let entry = "<https://example.com; rel=\"next\"";
        assert!(parse_link_next(entry).is_none());
    }

    #[tokio::test]
    async fn problem_json_or_json_problem_response_invalid_json_body() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/")
            .with_status(404)
            .with_header("content-type", "application/problem+json")
            .with_body("not json at all")
            .create_async()
            .await;

        let resp = reqwest::get(server.url()).await.unwrap();
        let err: api_bones::ApiError = resp
            .problem_json_or_json::<serde_json::Value>()
            .await
            .unwrap_err();
        assert_eq!(err.status, 400);
    }

    #[tokio::test]
    async fn problem_json_or_json_success_invalid_json_body() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("not json")
            .create_async()
            .await;

        let resp = reqwest::get(server.url()).await.unwrap();
        let err: api_bones::ApiError = resp
            .problem_json_or_json::<serde_json::Value>()
            .await
            .unwrap_err();
        assert_eq!(err.status, 400);
    }

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
