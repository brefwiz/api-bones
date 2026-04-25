#![cfg(feature = "axum")]

use api_bones::error::ErrorCode;
use api_bones::response::ApiResponse;
use api_bones_test::axum::{TestServer, assert_status};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::{Json, Router, routing::get};

fn envelope_router() -> Router {
    Router::new()
        .route(
            "/ok",
            get(|| async {
                let resp: ApiResponse<u32> = ApiResponse::builder(42u32).build();
                (StatusCode::OK, Json(resp))
            }),
        )
        .route(
            "/problem",
            get(|| async {
                let err = api_bones::error::ApiError::not_found("item not found");
                (
                    StatusCode::NOT_FOUND,
                    [(axum::http::header::CONTENT_TYPE, "application/problem+json")],
                    Json(err),
                )
                    .into_response()
            }),
        )
}

#[tokio::test]
async fn assert_envelope_passes_on_ok_response() {
    let server = TestServer::new(envelope_router());
    let value: u32 = server.get_envelope("/ok").await;
    assert_eq!(value, 42);
}

#[tokio::test]
async fn assert_problem_json_passes_on_not_found() {
    let server = TestServer::new(envelope_router());
    let err = server
        .get_problem("/problem", ErrorCode::ResourceNotFound)
        .await;
    assert_eq!(err.code, ErrorCode::ResourceNotFound);
}

#[tokio::test]
async fn assert_status_helper() {
    let server = TestServer::new(envelope_router());
    let resp = server.inner().get("/ok").await;
    assert_status(&resp, StatusCode::OK);
}
