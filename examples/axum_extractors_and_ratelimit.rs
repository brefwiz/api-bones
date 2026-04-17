//! Axum extractors and structured rate-limit bodies.
//!
//! Demonstrates:
//! - `PaginationParams`, `CursorPaginationParams`, `SortParams` as axum extractors
//! - `IfMatch` / `IfNoneMatch` as axum extractors
//! - `ApiError::rate_limited_with(RateLimitInfo)` producing a 429 with a
//!   structured `rate_limit` body (inline in `ApiError`, an extension member
//!   in `ProblemJson`)
//!
//! Run: `cargo run --example axum_extractors_and_ratelimit --features axum`

use api_bones::etag::{IfMatch, IfNoneMatch};
use api_bones::pagination::{CursorPaginationParams, PaginationParams};
use api_bones::query::SortParams;
use api_bones::ratelimit::RateLimitInfo;
use api_bones::{ApiError, ProblemJson};
use axum::{Router, routing::get};

/// List endpoint using offset pagination + sort, all via extractors.
///
/// A request like `GET /bookings?limit=50&offset=100&sort_by=created_at&direction=desc`
/// is validated automatically: `limit=0` or `limit=101` is rejected with a
/// Problem+JSON 400 body before this handler runs.
async fn list_bookings(page: PaginationParams, sort: SortParams) -> String {
    format!(
        "offset={} limit={} sort_by={} direction={:?}",
        page.offset(),
        page.limit(),
        sort.sort_by,
        sort.direction
    )
}

/// Cursor-paginated feed endpoint.
async fn feed(page: CursorPaginationParams) -> String {
    format!(
        "limit={} after={}",
        page.limit(),
        page.after().unwrap_or("<first page>")
    )
}

/// Update endpoint gated by `If-Match` (optimistic concurrency control).
async fn update_booking(if_match: IfMatch) -> Result<String, ApiError> {
    // Pretend the current ETag for resource 42 is `"v7"`.
    let current = api_bones::etag::ETag::strong("v7");
    if !if_match.matches(&current) {
        return Err(ApiError::new(
            api_bones::error::ErrorCode::Conflict,
            "ETag does not match current resource",
        ));
    }
    Ok("updated to v8".to_string())
}

/// GET endpoint using `If-None-Match` for client caching.
async fn get_booking(if_none_match: IfNoneMatch) -> Result<String, ApiError> {
    let current = api_bones::etag::ETag::strong("v7");
    if !if_none_match.matches(&current) {
        // Condition not satisfied → client already has this version → 304 in real code.
        return Err(ApiError::new(
            api_bones::error::ErrorCode::Conflict,
            "Not modified",
        ));
    }
    Ok("booking body".to_string())
}

/// Endpoint that returns a 429 with structured quota data in the body.
async fn quota() -> Result<String, ApiError> {
    let info = RateLimitInfo::new(100, 0, 1_700_000_000).retry_after(30);
    Err(ApiError::rate_limited_with(info))
}

fn main() {
    // The router compiles because each handler parameter implements
    // `FromRequestParts` — no custom newtypes required.
    let _app: Router = Router::new()
        .route("/bookings", get(list_bookings))
        .route("/feed", get(feed))
        .route("/bookings/42", get(get_booking).put(update_booking))
        .route("/quota", get(quota));

    // Show the wire shape of the 429 body.
    let info = RateLimitInfo::new(100, 0, 1_700_000_000).retry_after(30);
    let err = ApiError::rate_limited_with(info);
    let problem = ProblemJson::from(err);
    println!(
        "{}",
        serde_json::to_string_pretty(&problem).expect("serialization is infallible")
    );
}
