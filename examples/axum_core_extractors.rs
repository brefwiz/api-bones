//! Core-type axum extractors.
//!
//! Demonstrates that `RequestId`, `IdempotencyKey`, `ApiVersion`, and
//! `Authorization` can be used directly as axum handler parameters — no
//! wrapper newtypes needed.
//!
//! Run: `cargo run --example axum_core_extractors --features axum`

use api_bones::ApiError;
use api_bones::axum_extractors::Authorization;
use api_bones::idempotency::IdempotencyKey;
use api_bones::request_id::RequestId;
use api_bones::version::ApiVersion;
use axum::{Json, Router, routing::post};
use serde_json::{Value, json};

/// POST /bookings
///
/// Requires:
///   X-Request-Id: <uuid-v4>
///   Idempotency-Key: <1-255 printable ASCII>
///   X-Api-Version: v1  (or ?v=v1)
///   Authorization: Bearer <token>
async fn create_booking(
    request_id: RequestId,
    idem: IdempotencyKey,
    version: ApiVersion,
    auth: Authorization,
) -> Result<Json<Value>, ApiError> {
    auth.require_scheme("Bearer")?;

    Ok(Json(json!({
        "request_id": request_id.to_string(),
        "idempotency_key": idem.as_str(),
        "api_version": version.to_string(),
        "token_prefix": &auth.credentials[..auth.credentials.len().min(8)],
    })))
}

#[tokio::main]
async fn main() {
    let app = Router::new().route("/bookings", post(create_booking));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .expect("failed to bind");

    println!("Listening on http://127.0.0.1:3000");
    println!(
        "Try: curl -s -X POST http://127.0.0.1:3000/bookings \\\n\
         \t-H 'X-Request-Id: 550e8400-e29b-41d4-a716-446655440000' \\\n\
         \t-H 'Idempotency-Key: my-op-001' \\\n\
         \t-H 'X-Api-Version: v1' \\\n\
         \t-H 'Authorization: Bearer secret.token' | jq"
    );

    axum::serve(listener, app).await.expect("server error");
}
