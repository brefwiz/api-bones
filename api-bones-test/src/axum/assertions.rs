use api_bones::error::{ApiError, ErrorCode};
use api_bones::etag::ETag;
use api_bones::pagination::PaginatedResponse;
use api_bones::ratelimit::RateLimitInfo;
use api_bones::response::ApiResponse;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum_test::TestResponse;
use serde::de::DeserializeOwned;

fn assert_content_type(headers: &HeaderMap, expected: &str) {
    let ct = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        ct.starts_with(expected),
        "expected content-type {expected:?}, got {ct:?}"
    );
}

/// Assert a `2xx` JSON envelope response; return the unwrapped payload `T`.
///
/// Panics with an informative message on any assertion failure.
pub async fn assert_envelope<T: DeserializeOwned>(resp: TestResponse) -> T {
    let status = resp.status_code();
    assert!(status.is_success(), "expected 2xx, got {status}");
    assert_content_type(resp.headers(), "application/json");
    let envelope: ApiResponse<T> = resp.json();
    envelope.into_inner()
}

/// Assert a paginated `2xx` response; return `(items, pagination_meta)`.
pub async fn assert_paginated<T: DeserializeOwned>(resp: TestResponse) -> PaginatedResponse<T> {
    let status = resp.status_code();
    assert!(status.is_success(), "expected 2xx, got {status}");
    assert_content_type(resp.headers(), "application/json");
    let envelope: ApiResponse<PaginatedResponse<T>> = resp.json();
    envelope.into_inner()
}

/// Assert a `application/problem+json` response with the expected error code.
pub async fn assert_problem_json(resp: TestResponse, expected_code: ErrorCode) -> ApiError {
    let status = resp.status_code();
    assert_content_type(resp.headers(), "application/problem+json");
    let err: ApiError = resp.json();
    assert_eq!(
        err.status,
        expected_code.status_code(),
        "expected HTTP status {} for {expected_code:?}, got {}",
        expected_code.status_code(),
        err.status
    );
    assert_eq!(
        status.as_u16(),
        expected_code.status_code(),
        "response HTTP status mismatch"
    );
    assert_eq!(
        err.code, expected_code,
        "expected error code {expected_code:?}, got {:?}",
        err.code
    );
    err
}

/// Assert the `ETag` response header is present and return it.
#[must_use]
pub fn assert_etag_present(headers: &HeaderMap) -> ETag {
    let value = headers
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .expect("ETag header missing");
    if let Some(stripped) = value.strip_prefix("W/\"") {
        ETag::weak(stripped.trim_end_matches('"'))
    } else {
        ETag::strong(value.trim_matches('"'))
    }
}

/// Assert the `Location` header equals `expected`.
pub fn assert_location_eq(headers: &HeaderMap, expected: &str) {
    let location = headers
        .get("location")
        .and_then(|v| v.to_str().ok())
        .expect("Location header missing");
    assert_eq!(location, expected, "Location header mismatch");
}

/// Assert rate-limit headers are present; return parsed [`RateLimitInfo`].
#[must_use]
pub fn assert_rate_limit_headers(headers: &HeaderMap) -> RateLimitInfo {
    let limit = headers
        .get("x-ratelimit-limit")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .expect("X-RateLimit-Limit header missing or invalid");
    let remaining = headers
        .get("x-ratelimit-remaining")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .expect("X-RateLimit-Remaining header missing or invalid");
    RateLimitInfo {
        limit,
        remaining,
        reset: headers
            .get("x-ratelimit-reset")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0),
        retry_after: None,
    }
}

/// Assert the HTTP status code equals `expected`.
pub fn assert_status(resp: &TestResponse, expected: StatusCode) {
    let actual = resp.status_code();
    assert_eq!(
        actual, expected,
        "status mismatch: expected {expected}, got {actual}"
    );
}

/// Suppress "unused import" — `Response` is re-exported for caller convenience.
#[allow(dead_code)]
fn _use_response(_: Response) {}
