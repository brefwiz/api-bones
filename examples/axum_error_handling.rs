//! Axum error handling with `ApiError`.
//!
//! Demonstrates how `ApiError` implements `IntoResponse`, producing
//! RFC 9457 Problem Details JSON with the correct HTTP status code
//! and `application/problem+json` content type.
//!
//! Run: `cargo run --example axum_error_handling --features axum`

use api_bones::{ApiError, ErrorCode, ValidationError};
use axum::{Router, routing::get};

/// Handler that always returns a 404 error.
async fn get_booking() -> Result<String, ApiError> {
    Err(ApiError::not_found("Booking 42 not found"))
}

/// Handler that returns a validation error with field-level details.
async fn create_booking() -> Result<String, ApiError> {
    Err(
        ApiError::new(ErrorCode::ValidationFailed, "Invalid booking request").with_errors(vec![
            ValidationError {
                field: "/start_date".into(),
                message: "must be in the future".into(),
                rule: None,
            },
            ValidationError {
                field: "/guest_count".into(),
                message: "must be at least 1".into(),
                rule: Some("min".into()),
            },
        ]),
    )
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/bookings/{id}", get(get_booking))
        .route("/bookings", get(create_booking));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .expect("failed to bind");

    println!("Listening on http://127.0.0.1:3000");
    println!("Try: curl -s http://127.0.0.1:3000/bookings/42 | jq");
    println!("Try: curl -s http://127.0.0.1:3000/bookings | jq");

    axum::serve(listener, app).await.expect("server error");
}
