//! API response envelope with metadata and links.
//!
//! Demonstrates `ApiResponse`, `ResponseMeta`, and response-level `Links`,
//! including composition with `PaginatedResponse`.
//!
//! Run: `cargo run --example response_envelope`

use api_bones::{
    ApiResponse, PaginatedResponse, PaginationParams, ResponseMeta,
    links::{Link, Links},
    new_resource_id,
};

fn main() {
    // -- Minimal envelope --
    let minimal: ApiResponse<&str> = ApiResponse::builder("hello").build();
    println!("Minimal ApiResponse:");
    print_json("  ", &minimal);

    // -- Full envelope with meta + links --
    let full: ApiResponse<serde_json::Value> = ApiResponse::builder(serde_json::json!({
        "id": new_resource_id().to_string(),
        "name": "Deluxe Suite",
        "price": 299.99
    }))
    .meta(
        ResponseMeta::new()
            .request_id("req-abc-123")
            .version("1.6.0")
            .timestamp(chrono::Utc::now()),
    )
    .links(
        Links::new()
            .push(Link::self_link("/rooms/42"))
            .push(Link::next("/rooms?after=42"))
            .push(Link::prev("/rooms?before=42")),
    )
    .build();
    println!("\nFull ApiResponse with meta + links:");
    print_json("  ", &full);

    // -- Composing with PaginatedResponse --
    let paginated_envelope = ApiResponse::builder(PaginatedResponse::new(
        vec!["item1", "item2"],
        100,
        &PaginationParams::default(),
    ))
    .meta(ResponseMeta::new().request_id("req-list-001"))
    .build();
    println!("\nApiResponse wrapping PaginatedResponse:");
    print_json("  ", &paginated_envelope);
}

fn print_json(prefix: &str, value: &impl serde::Serialize) {
    let json = serde_json::to_string_pretty(value).expect("serialization");
    for line in json.lines() {
        println!("{prefix}{line}");
    }
}
