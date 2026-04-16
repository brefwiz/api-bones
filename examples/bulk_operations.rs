//! Bulk operation envelope types.
//!
//! Demonstrates `BulkRequest`, `BulkResponse`, and `BulkItemResult`
//! for batch API endpoints with per-item success/failure tracking.
//!
//! Run: `cargo run --example bulk_operations`

use api_bones::{ApiError, BulkItemResult, BulkRequest, BulkResponse};

fn main() {
    // -- Build a bulk request --
    let request: BulkRequest<String> = BulkRequest {
        items: vec!["create-a".into(), "create-b".into(), "create-c".into()],
    };
    println!("BulkRequest: {} items", request.items.len());
    print_json("  ", &request);

    // -- Simulate mixed results --
    let response: BulkResponse<String> = BulkResponse {
        results: vec![
            BulkItemResult::Success {
                data: "created-a".into(),
            },
            BulkItemResult::Failure {
                index: 1,
                error: ApiError::bad_request("duplicate name"),
            },
            BulkItemResult::Success {
                data: "created-c".into(),
            },
        ],
    };
    println!("\nBulkResponse:");
    println!("  succeeded:    {}", response.succeeded_count());
    println!("  failed:       {}", response.failed_count());
    println!("  has_failures: {}", response.has_failures());

    for (i, result) in response.results.iter().enumerate() {
        println!(
            "  [{}] is_success={}, is_failure={}",
            i,
            result.is_success(),
            result.is_failure()
        );
    }
    print_json("\n  ", &response);
}

fn print_json(prefix: &str, value: &impl serde::Serialize) {
    let json = serde_json::to_string_pretty(value).expect("serialization");
    for line in json.lines() {
        println!("{prefix}{line}");
    }
}
