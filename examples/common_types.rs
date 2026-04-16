//! Common primitive types: `ResourceId`, `Timestamp`, and `ErrorResponse`.
//!
//! Demonstrates UUID v4 resource identifiers, RFC 3339 timestamp parsing,
//! and the simple `ErrorResponse` model.
//!
//! Run: `cargo run --example common_types`

use api_bones::{ErrorResponse, ResourceId, Timestamp, new_resource_id, parse_timestamp};

fn main() {
    // -- ResourceId (UUID v4) --
    println!("=== ResourceId ===");
    let id: ResourceId = new_resource_id();
    println!("Generated UUID v4: {id}");
    println!("Version:           {}", id.get_version_num());

    // -- Timestamp (chrono DateTime<Utc>) --
    println!("\n=== Timestamp ===");
    let ts: Timestamp = parse_timestamp("2026-04-06T12:00:00Z").expect("valid RFC 3339");
    println!("Parsed:  {ts}");
    println!("RFC3339: {}", ts.to_rfc3339());
    println!("Now:     {}", chrono::Utc::now());

    // -- ErrorResponse (simple model) --
    println!("\n=== ErrorResponse ===");
    let err = ErrorResponse::new("Something went wrong");
    let json = serde_json::to_string_pretty(&err).expect("serialization");
    println!("{json}");
}
