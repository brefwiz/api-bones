//! `ProblemJsonLayer` — convert service errors to Problem+JSON responses.
//!
//! Demonstrates:
//! - An inner service error converted to `application/problem+json`
//! - A successful response passing through unchanged
//! - Composing both layers together
//!
//! Run: `cargo run --example problem_json`

use api_bones::ApiError;
use api_bones_tower::{ProblemJsonLayer, RequestIdLayer};
use http::{Request, Response};
use tower::{Layer, ServiceExt};

#[tokio::main]
async fn main() {
    // -- Error path: ApiError becomes a Problem+JSON response --
    let svc = ProblemJsonLayer::new().layer(tower::service_fn(|_req: Request<()>| async move {
        Err::<Response<String>, ApiError>(ApiError::not_found("booking 42"))
    }));

    let req = Request::builder().uri("/bookings/42").body(()).unwrap();
    let resp = svc.oneshot(req).await.unwrap(); // outer error is Infallible

    println!("Status:       {}", resp.status());
    println!(
        "Content-Type: {}",
        resp.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("(none)")
    );
    println!("Body:         {}", resp.body());
    assert_eq!(resp.status().as_u16(), 404);

    // -- Happy path: successful response is untouched --
    let svc_ok = ProblemJsonLayer::new().layer(tower::service_fn(|_req: Request<()>| async move {
        Ok::<_, ApiError>(
            Response::builder()
                .status(200)
                .body("{\"id\":42}".to_owned())
                .unwrap(),
        )
    }));

    let req_ok = Request::builder().uri("/bookings/42").body(()).unwrap();
    let resp_ok = svc_ok.oneshot(req_ok).await.unwrap();
    println!("\nSuccess passthrough: {}", resp_ok.body());
    assert_eq!(resp_ok.status().as_u16(), 200);

    // -- Composed: RequestIdLayer + ProblemJsonLayer --
    let composed = RequestIdLayer::new().layer(ProblemJsonLayer::new().layer(tower::service_fn(
        |_req: Request<()>| async move {
            Err::<Response<String>, ApiError>(ApiError::unauthorized("token expired"))
        },
    )));

    let req_composed = Request::builder().uri("/secure").body(()).unwrap();
    let resp_composed = composed.oneshot(req_composed).await.unwrap();
    let req_id = resp_composed
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("(none)");
    println!(
        "\nComposed — status={}, x-request-id={req_id}",
        resp_composed.status()
    );
    assert_eq!(resp_composed.status().as_u16(), 401);

    println!("\nAll assertions passed.");
}
