//! RFC 7807 / 9457 `ProblemJson` response type.
//!
//! Demonstrates converting an [`ApiError`] to a [`ProblemJson`], adding
//! extension members (trace ID, request ID), and serving it with Axum using
//! the correct `application/problem+json` Content-Type.
//!
//! Run: `cargo run --example problem_json --features axum`

use api_bones::{ApiError, ErrorCode, ProblemJson, ValidationError};
use axum::{Router, routing::get};

/// Handler that returns a plain 404 as `ProblemJson`.
async fn get_booking() -> Result<String, ProblemJson> {
    let err = ApiError::not_found("Booking 42 not found");
    let mut problem = ProblemJson::from(err);
    problem.extend("trace_id", "span-abc-123");
    Err(problem)
}

/// Handler returning a validation error with extension members.
async fn create_booking() -> Result<String, ProblemJson> {
    let err =
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
        ]);
    let mut problem = ProblemJson::from(err);
    problem.extend("trace_id", "span-xyz-789");
    Err(problem)
}

/// Handler built directly without an `ApiError`.
async fn custom_problem() -> Result<String, ProblemJson> {
    let problem = ProblemJson::new(
        "urn:api-bones:error:rate-limited",
        "Rate Limited",
        429,
        "Too many requests — retry after 60 s",
    )
    .with_instance("urn:uuid:00000000-0000-0000-0000-000000000001");
    Err(problem)
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/bookings/{id}", get(get_booking))
        .route("/bookings", get(create_booking))
        .route("/rate-limited", get(custom_problem));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .expect("failed to bind");

    println!("Listening on http://127.0.0.1:3000");
    println!("Try: curl -s http://127.0.0.1:3000/bookings/42 | jq");
    println!("Try: curl -s http://127.0.0.1:3000/bookings | jq");
    println!("Try: curl -s http://127.0.0.1:3000/rate-limited | jq");

    axum::serve(listener, app).await.expect("server error");
}
