//! `RequestIdLayer` — inject and propagate `X-Request-Id`.
//!
//! Demonstrates:
//! - Auto-generating a request ID when none is present
//! - Preserving a client-supplied request ID
//! - The counter incrementing across multiple requests
//!
//! Run: `cargo run --example request_id`

use api_bones_tower::RequestIdLayer;
use http::{Request, Response};
use tower::{Layer, ServiceExt};

#[tokio::main]
async fn main() {
    let layer = RequestIdLayer::new();

    // -- Request with no ID: middleware generates one --
    let svc = layer.layer(tower::service_fn(echo_request_id));
    let req = Request::builder().uri("/hello").body(()).unwrap();
    let resp = svc.oneshot(req).await.unwrap();
    let id = header_str(&resp, "x-request-id");
    println!("No client ID → generated: {id}");
    assert!(id.starts_with("req-"), "expected req-N format, got {id}");

    // -- Second request: counter increments --
    let layer2 = RequestIdLayer::new();
    let svc2 = layer2.layer(tower::service_fn(echo_request_id));
    let req2 = Request::builder().uri("/world").body(()).unwrap();
    let resp2 = svc2.oneshot(req2).await.unwrap();
    let id2 = header_str(&resp2, "x-request-id");
    println!("Second request  → generated: {id2}");

    // -- Client-supplied ID is preserved unchanged --
    let layer3 = RequestIdLayer::new();
    let svc3 = layer3.layer(tower::service_fn(echo_request_id));
    let req3 = Request::builder()
        .uri("/ping")
        .header("x-request-id", "client-trace-abc123")
        .body(())
        .unwrap();
    let resp3 = svc3.oneshot(req3).await.unwrap();
    let id3 = header_str(&resp3, "x-request-id");
    println!("Client-supplied → preserved:  {id3}");
    assert_eq!(id3, "client-trace-abc123");

    println!("\nAll assertions passed.");
}

/// Inner service: echoes back the `x-request-id` that arrived on the request.
async fn echo_request_id(req: Request<()>) -> Result<Response<String>, std::convert::Infallible> {
    let id = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    Ok(Response::new(id))
}

fn header_str<B>(resp: &Response<B>, name: &str) -> String {
    resp.headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned()
}
