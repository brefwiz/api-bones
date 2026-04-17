//! `ApiResponse<BulkResponse<T>>` with mixed success/failure items.
//!
//! Shows how to wrap a bulk operation result in the standard API response
//! envelope, inspect per-item outcomes, and serialize to JSON.
//!
//! Run: `cargo run --example bulk_envelope`

use api_bones::response::{ApiResponse, ResponseMeta};
use api_bones::{ApiError, BulkItemResult, BulkRequest, BulkResponse};

/// A domain type representing an order being created in bulk.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct Order {
    id: u32,
    name: String,
}

fn main() {
    // -------------------------------------------------------------------------
    // 1. Build a bulk request
    // -------------------------------------------------------------------------
    let request: BulkRequest<String> = BulkRequest {
        items: vec![
            "create-order-A".into(),
            "create-order-B".into(),
            "create-order-C".into(),
        ],
    };
    println!("BulkRequest: {} items", request.items.len());
    print_json(&request);

    // -------------------------------------------------------------------------
    // 2. Simulate processing — mixed success and failure
    // -------------------------------------------------------------------------
    let results: Vec<BulkItemResult<Order>> = vec![
        BulkItemResult::Success {
            data: Order {
                id: 1,
                name: "order-A".into(),
            },
        },
        BulkItemResult::Failure {
            index: 1,
            error: Box::new(ApiError::conflict("order-B already exists")),
        },
        BulkItemResult::Success {
            data: Order {
                id: 3,
                name: "order-C".into(),
            },
        },
    ];

    let bulk_response: BulkResponse<Order> = BulkResponse { results };

    println!("\nBulkResponse summary:");
    println!("  succeeded: {}", bulk_response.succeeded_count());
    println!("  failed:    {}", bulk_response.failed_count());
    println!("  has_failures: {}", bulk_response.has_failures());

    for (i, result) in bulk_response.results.iter().enumerate() {
        println!(
            "  [{}] success={} failure={}",
            i,
            result.is_success(),
            result.is_failure()
        );
    }

    // -------------------------------------------------------------------------
    // 3. Wrap in ApiResponse envelope
    // -------------------------------------------------------------------------
    let envelope: ApiResponse<&BulkResponse<Order>> = ApiResponse::builder(&bulk_response)
        .meta(
            ResponseMeta::new()
                .request_id("req-bulk-001")
                .version("1.0"),
        )
        .build();

    println!("\nApiResponse<BulkResponse<Order>>:");
    print_json(&envelope);
}

fn print_json(value: &impl serde::Serialize) {
    let json = serde_json::to_string_pretty(value).expect("serialization failed");
    for line in json.lines() {
        println!("  {line}");
    }
}
