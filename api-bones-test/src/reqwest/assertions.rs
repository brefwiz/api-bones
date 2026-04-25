use api_bones::error::{ApiError, ErrorCode};
use api_bones::pagination::PaginatedResponse;
use api_bones::response::ApiResponse;
use serde::de::DeserializeOwned;

/// Assert a `2xx` JSON envelope response from reqwest; return the unwrapped payload.
///
/// # Panics
///
/// Panics with an informative message if the response is not `2xx` or not a
/// valid `application/json` envelope.
pub async fn assert_envelope_reqwest<T: DeserializeOwned>(resp: reqwest::Response) -> T {
    let status = resp.status();
    assert!(status.is_success(), "expected 2xx, got {status}");
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        ct.starts_with("application/json"),
        "expected application/json content-type, got {ct:?}"
    );
    let envelope: ApiResponse<T> = resp.json().await.expect("failed to parse envelope");
    envelope.into_inner()
}

/// Assert a paginated `2xx` reqwest response; return the `PaginatedResponse`.
pub async fn assert_paginated_reqwest<T: DeserializeOwned>(
    resp: reqwest::Response,
) -> PaginatedResponse<T> {
    let status = resp.status();
    assert!(status.is_success(), "expected 2xx, got {status}");
    let envelope: ApiResponse<PaginatedResponse<T>> = resp
        .json()
        .await
        .expect("failed to parse paginated envelope");
    envelope.into_inner()
}

/// Assert a `application/problem+json` reqwest response with the expected code.
pub async fn assert_problem_json_reqwest(
    resp: reqwest::Response,
    expected_code: ErrorCode,
) -> ApiError {
    let status = resp.status();
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    assert!(
        ct.starts_with("application/problem+json"),
        "expected application/problem+json content-type, got {ct:?}"
    );
    let err: ApiError = resp
        .json()
        .await
        .expect("failed to parse problem+json body");
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
