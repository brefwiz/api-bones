//! Reqwest client extensions from `api-bones-reqwest`.
//!
//! Demonstrates `RequestBuilderExt` (attach request-id, idempotency-key,
//! bearer token) and `ResponseExt` (rate-limit headers, Link pagination,
//! problem+json error extraction).
//!
//! Run: `cargo run --example client_extensions`

use api_bones_reqwest::{RequestBuilderExt, ResponseExt};

#[tokio::main]
#[allow(clippy::significant_drop_tightening)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // -----------------------------------------------------------------------
    // RequestBuilderExt — header attachment
    // -----------------------------------------------------------------------

    println!("=== RequestBuilderExt ===\n");

    // These are compile-time demonstrations; the requests are not sent.
    let _req = reqwest::Client::new()
        .post("https://api.example.com/orders")
        .with_request_id("req-abc-123")
        .with_idempotency_key("idem-xyz-789")
        .with_bearer_token("eyJhbGciOiJIUzI1NiJ9.example");

    println!("Builder with X-Request-Id, Idempotency-Key, Authorization: Bearer — built ok");

    // -----------------------------------------------------------------------
    // ResponseExt::rate_limit_info — parse headers
    // -----------------------------------------------------------------------

    println!("\n=== ResponseExt::rate_limit_info ===\n");

    // Simulate with mockito
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/items")
        .with_status(200)
        .with_header("x-ratelimit-limit", "100")
        .with_header("x-ratelimit-remaining", "42")
        .with_header("x-ratelimit-reset", "1700000000")
        .with_header("retry-after", "5")
        .with_body("[]")
        .create_async()
        .await;

    let resp = reqwest::get(format!("{}/items", server.url())).await?;

    match resp.rate_limit_info() {
        Some(rl) => {
            println!("  limit:       {}", rl.limit);
            println!("  remaining:   {}", rl.remaining);
            println!("  reset:       {}", rl.reset);
            println!("  retry_after: {:?}", rl.retry_after);
        }
        None => println!("  no rate-limit headers"),
    }

    // -----------------------------------------------------------------------
    // ResponseExt::next_page_url — RFC 5988 Link header
    // -----------------------------------------------------------------------

    println!("\n=== ResponseExt::next_page_url ===\n");

    let _mock2 = server
        .mock("GET", "/paged")
        .with_status(200)
        .with_header(
            "link",
            r#"<https://api.example.com/items?after=xyz>; rel="next", <https://api.example.com/items>; rel="first""#,
        )
        .with_body("[]")
        .create_async()
        .await;

    let resp2 = reqwest::get(format!("{}/paged", server.url())).await?;
    println!("  next page: {:?}", resp2.next_page_url());

    // -----------------------------------------------------------------------
    // ResponseExt::problem_json_or_json — success path
    // -----------------------------------------------------------------------

    println!("\n=== ResponseExt::problem_json_or_json (success) ===\n");

    let _mock3 = server
        .mock("GET", "/resource")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"id": 1, "name": "example"}"#)
        .create_async()
        .await;

    let resp3 = reqwest::get(format!("{}/resource", server.url())).await?;
    let body: serde_json::Value = resp3.problem_json_or_json().await?;
    println!("  body: {body}");

    // -----------------------------------------------------------------------
    // ResponseExt::problem_json_or_json — problem+json error
    // -----------------------------------------------------------------------

    println!("\n=== ResponseExt::problem_json_or_json (problem+json error) ===\n");

    let _mock4 = server
        .mock("GET", "/missing")
        .with_status(404)
        .with_header("content-type", "application/problem+json")
        .with_body(
            r#"{"type":"urn:api-bones:error:resource-not-found","title":"Resource Not Found","status":404,"detail":"item 99 not found"}"#,
        )
        .create_async()
        .await;

    let resp4 = reqwest::get(format!("{}/missing", server.url())).await?;
    match resp4.problem_json_or_json::<serde_json::Value>().await {
        Ok(v) => println!("  unexpected ok: {v}"),
        Err(e) => {
            println!("  error status: {}", e.status);
            println!("  error detail: {}", e.detail);
        }
    }

    Ok(())
}
