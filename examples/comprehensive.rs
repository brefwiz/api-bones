//! Comprehensive api-bones showcase.
//!
//! Demonstrates every public type and feature in the crate:
//! errors (RFC 9457), pagination, health checks, response envelopes,
//! HATEOAS links, slugs, ETags, bulk operations, audit metadata,
//! rate limiting, query parameters, and common types.
//!
//! Run: `cargo run --example comprehensive`

use api_bones::{
    // Common types
    AuditInfo,
    ResourceId,
    Timestamp,
    new_resource_id,
    parse_timestamp,
    // Errors (RFC 9457)
    ApiError,
    ErrorCode,
    ErrorTypeMode,
    ValidationError,
    set_error_type_mode,
    // Health checks (RFC 8458)
    HealthCheck,
    HealthStatus,
    LivenessResponse,
    ReadinessResponse,
    // Pagination
    CursorPaginatedResponse,
    CursorPagination,
    CursorPaginationParams,
    PaginatedResponse,
    PaginationParams,
    // Response envelope
    ApiResponse,
    ResponseMeta,
    // HATEOAS links
    links::{Link, Links},
    // ETag / conditional requests (RFC 7232)
    ETag,
    IfMatch,
    IfNoneMatch,
    // Bulk operations
    BulkItemResult,
    BulkRequest,
    BulkResponse,
    // Rate limiting
    RateLimitInfo,
    // URL-safe slugs
    Slug,
    // Query parameter types
    FilterEntry,
    FilterParams,
    SearchParams,
    SortDirection,
    SortParams,
    // Error response model
    ErrorResponse,
};

fn main() {
    // =========================================================================
    // 1. Common types: ResourceId, Timestamp
    // =========================================================================
    println!("=== 1. Common types ===");

    let id: ResourceId = new_resource_id();
    println!("New resource ID (UUID v4): {id}");

    let ts: Timestamp = parse_timestamp("2026-04-06T12:00:00Z").expect("valid RFC 3339");
    println!("Parsed timestamp: {ts}");
    println!("Current time:     {}", chrono::Utc::now());

    // =========================================================================
    // 2. Errors — RFC 9457 Problem Details
    // =========================================================================
    println!("\n=== 2. Errors (RFC 9457) ===");

    // Configure the error type URI mode (URN vs URL)
    set_error_type_mode(ErrorTypeMode::Urn {
        namespace: "api-bones".into(),
    });
    println!(
        "Error type mode: URN (namespace = {:?})",
        api_bones::urn_namespace()
    );

    // Convenience constructors
    let not_found = ApiError::not_found("Booking 42 not found");
    println!("\nApiError::not_found:");
    println!("  title:  {}", not_found.title);
    println!("  status: {}", not_found.status);
    println!("  code:   {:?}", not_found.code);
    println!("  detail: {}", not_found.detail);
    print_json("  ", &not_found);

    // Builder pattern
    let built = ApiError::builder()
        .code(ErrorCode::ValidationFailed)
        .detail("Request body failed validation")
        .build();
    println!("\nApiError::builder:");
    print_json("  ", &built);

    // With validation errors
    let validation_err = ApiError::validation_failed("Invalid input").with_errors(vec![
        ValidationError {
            field: "/email".into(),
            message: "must be a valid email address".into(),
            rule: Some("email".into()),
        },
        ValidationError {
            field: "/age".into(),
            message: "must be at least 18".into(),
            rule: Some("min".into()),
        },
    ]);
    println!("\nWith validation errors:");
    print_json("  ", &validation_err);

    // With request ID
    let with_req_id = ApiError::internal("unexpected failure").with_request_id(new_resource_id());
    println!("\nWith request ID:");
    print_json("  ", &with_req_id);

    // All error codes
    println!("\nAll ErrorCode variants:");
    let codes = [
        ErrorCode::BadRequest,
        ErrorCode::ValidationFailed,
        ErrorCode::Unauthorized,
        ErrorCode::InvalidCredentials,
        ErrorCode::TokenExpired,
        ErrorCode::TokenInvalid,
        ErrorCode::Forbidden,
        ErrorCode::InsufficientPermissions,
        ErrorCode::ResourceNotFound,
        ErrorCode::Conflict,
        ErrorCode::ResourceAlreadyExists,
        ErrorCode::UnprocessableEntity,
        ErrorCode::RateLimited,
        ErrorCode::InternalServerError,
        ErrorCode::ServiceUnavailable,
    ];
    for code in &codes {
        let err = ApiError::new(code.clone(), "example");
        println!("  {:?} → status={}, title={:?}", code, err.status, err.title);
    }

    // Client vs server error helpers
    let client_err = ApiError::bad_request("oops");
    let server_err = ApiError::internal("boom");
    println!("\nis_client_error: bad_request={}, internal={}", client_err.is_client_error(), server_err.is_client_error());
    println!("is_server_error: bad_request={}, internal={}", client_err.is_server_error(), server_err.is_server_error());

    // ErrorTypeMode::Url
    set_error_type_mode(ErrorTypeMode::Url {
        base_url: "https://docs.example.com/errors".into(),
    });
    let url_mode_err = ApiError::not_found("example");
    println!("\nURL mode error type:");
    print_json("  ", &url_mode_err);

    // Reset to URN for remaining output
    set_error_type_mode(ErrorTypeMode::Urn {
        namespace: "api-bones".into(),
    });

    // =========================================================================
    // 3. Health checks — RFC 8458
    // =========================================================================
    println!("\n=== 3. Health checks (RFC 8458) ===");

    // Liveness
    let liveness = LivenessResponse::pass("1.6.0", "booking-service");
    println!("Liveness:");
    print_json("  ", &liveness);

    // Readiness — healthy
    let healthy = ReadinessResponse::builder()
        .version("1.6.0")
        .service_id("booking-service")
        .add_check("postgres:connection", HealthCheck::pass("datastore"))
        .add_check("redis:ping", HealthCheck::pass("datastore"))
        .add_check(
            "s3:latency",
            HealthCheck::builder()
                .component_type("system")
                .status(HealthStatus::Pass)
                .output("p99 = 12ms")
                .time("2026-04-06T12:00:00Z")
                .build(),
        )
        .build();
    println!("\nReadiness (healthy):");
    println!("  status={}, http_status={}", healthy.status, healthy.http_status());
    print_json("  ", &healthy);

    // Readiness — degraded
    let degraded = ReadinessResponse::builder()
        .version("1.6.0")
        .service_id("booking-service")
        .add_check("postgres:connection", HealthCheck::pass("datastore"))
        .add_check("redis:latency", HealthCheck::warn("datastore", "p99 > 200ms"))
        .build();
    println!("\nReadiness (degraded): status={}, http_status={}", degraded.status, degraded.http_status());

    // Readiness — unhealthy
    let unhealthy = ReadinessResponse::builder()
        .version("1.6.0")
        .service_id("booking-service")
        .add_check("postgres:connection", HealthCheck::fail("datastore", "connection refused"))
        .build();
    println!("Readiness (unhealthy): status={}, http_status={}", unhealthy.status, unhealthy.http_status());

    // HealthStatus helpers
    println!("\nHealthStatus helpers:");
    println!("  Pass.is_available()={}", HealthStatus::Pass.is_available());
    println!("  Warn.is_available()={}", HealthStatus::Warn.is_available());
    println!("  Fail.is_available()={}", HealthStatus::Fail.is_available());

    // =========================================================================
    // 4. Pagination
    // =========================================================================
    println!("\n=== 4. Pagination ===");

    // Offset-based
    let params = PaginationParams {
        limit: Some(10),
        offset: Some(20),
    };
    println!("PaginationParams: limit={}, offset={}", params.limit(), params.offset());

    let items = vec!["alpha", "bravo", "charlie"];
    let page = PaginatedResponse::new(items, 50, &params);
    println!("\nOffset-based PaginatedResponse:");
    print_json("  ", &page);

    // Cursor-based
    let cursor_params = CursorPaginationParams {
        limit: Some(25),
        after: Some("eyJpZCI6NDJ9".into()),
    };
    println!("\nCursorPaginationParams: limit={}, after={:?}", cursor_params.limit(), cursor_params.after());

    let cursor_page = CursorPaginatedResponse::new(
        vec!["delta", "echo"],
        CursorPagination::more("eyJpZCI6NDR9"),
    );
    println!("\nCursor-based response (has more):");
    print_json("  ", &cursor_page);

    let last = CursorPaginatedResponse::new(vec!["foxtrot"], CursorPagination::last_page());
    println!("\nCursor-based response (last page):");
    print_json("  ", &last);

    // Default params
    let defaults = PaginationParams::default();
    println!("\nDefault PaginationParams: limit={}, offset={}", defaults.limit(), defaults.offset());

    // =========================================================================
    // 5. Response envelope (ApiResponse)
    // =========================================================================
    println!("\n=== 5. Response envelope ===");

    // Minimal
    let minimal: ApiResponse<&str> = ApiResponse::builder("hello").build();
    println!("Minimal ApiResponse:");
    print_json("  ", &minimal);

    // Full with meta + links
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
        api_bones::response::Links::new()
            .self_link("/rooms/42")
            .next("/rooms?after=42")
            .prev("/rooms?before=42"),
    )
    .build();
    println!("\nFull ApiResponse with meta + links:");
    print_json("  ", &full);

    // Composing with PaginatedResponse
    let paginated_envelope = ApiResponse::builder(PaginatedResponse::new(
        vec!["item1", "item2"],
        100,
        &PaginationParams::default(),
    ))
    .meta(ResponseMeta::new().request_id("req-list-001"))
    .build();
    println!("\nApiResponse wrapping PaginatedResponse:");
    print_json("  ", &paginated_envelope);

    // =========================================================================
    // 6. HATEOAS links
    // =========================================================================
    println!("\n=== 6. HATEOAS links ===");

    let links = Links::new()
        .push(Link::self_link("/bookings/42"))
        .push(Link::next("/bookings?page=3"))
        .push(Link::prev("/bookings?page=1"))
        .push(Link::first("/bookings?page=1"))
        .push(Link::last("/bookings?page=10"))
        .push(Link::related("/guests/7"))
        .push(Link::new("cancel", "/bookings/42/cancel").method("POST"));

    println!("Links collection ({} links):", links.len());
    for link in links.iter() {
        println!(
            "  rel={:10} href={} {}",
            link.rel,
            link.href,
            link.method.as_deref().map_or(String::new(), |m| format!("[{m}]"))
        );
    }
    println!("Find 'self': {:?}", links.find("self").map(|l| &l.href));
    println!("Find 'edit': {:?}", links.find("edit"));
    print_json("JSON: ", &links);

    // =========================================================================
    // 7. ETags & conditional requests (RFC 7232)
    // =========================================================================
    println!("\n=== 7. ETags (RFC 7232) ===");

    let strong = ETag::strong("abc123");
    let weak = ETag::weak("abc123");
    println!("Strong ETag: {strong}");
    println!("Weak   ETag: {weak}");
    println!("Strong matches strong (same value):  {}", strong.matches(&ETag::strong("abc123")));
    println!("Strong matches strong (diff value):  {}", strong.matches(&ETag::strong("xyz")));
    println!("Strong matches weak (same value):    {}", strong.matches(&weak));
    println!("Weak comparison (same value):        {}", strong.matches_weak(&weak));

    // If-Match
    let if_match_any = IfMatch::Any;
    let if_match_tags = IfMatch::Tags(vec![ETag::strong("abc123"), ETag::strong("def456")]);
    println!("\nIf-Match::Any matches strong:   {}", if_match_any.matches(&strong));
    println!("If-Match::Tags matches strong:  {}", if_match_tags.matches(&strong));
    println!("If-Match::Tags matches unknown: {}", if_match_tags.matches(&ETag::strong("unknown")));

    // If-None-Match
    let if_none_match = IfNoneMatch::Tags(vec![ETag::strong("abc123")]);
    println!("\nIf-None-Match::Tags satisfied (unknown): {}", if_none_match.matches(&ETag::strong("unknown")));
    println!("If-None-Match::Tags satisfied (known):   {}", if_none_match.matches(&strong));

    // =========================================================================
    // 8. Bulk operations
    // =========================================================================
    println!("\n=== 8. Bulk operations ===");

    let bulk_req: BulkRequest<String> = BulkRequest {
        items: vec!["create-a".into(), "create-b".into(), "create-c".into()],
    };
    println!("BulkRequest: {} items", bulk_req.items.len());
    print_json("  ", &bulk_req);

    let bulk_resp: BulkResponse<String> = BulkResponse {
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
    println!("  succeeded: {}", bulk_resp.succeeded_count());
    println!("  failed:    {}", bulk_resp.failed_count());
    println!("  has_failures: {}", bulk_resp.has_failures());
    print_json("  ", &bulk_resp);

    // =========================================================================
    // 9. Audit metadata
    // =========================================================================
    println!("\n=== 9. Audit metadata ===");

    let mut audit = AuditInfo::now(Some("alice".into()));
    println!("Created:    {} by {:?}", audit.created_at, audit.created_by);
    println!("Updated:    {} by {:?}", audit.updated_at, audit.updated_by);

    audit.touch(Some("bob".into()));
    println!("After touch: {} by {:?}", audit.updated_at, audit.updated_by);
    print_json("  ", &audit);

    // =========================================================================
    // 10. Rate limiting
    // =========================================================================
    println!("\n=== 10. Rate limiting ===");

    let rate_ok = RateLimitInfo::new(100, 42, 1_700_000_000);
    println!("Rate limit (ok):       remaining={}, exceeded={}", rate_ok.remaining, rate_ok.is_exceeded());
    print_json("  ", &rate_ok);

    let rate_exceeded = RateLimitInfo::new(100, 0, 1_700_000_000).retry_after(60);
    println!("\nRate limit (exceeded): remaining={}, exceeded={}, retry_after={:?}",
        rate_exceeded.remaining, rate_exceeded.is_exceeded(), rate_exceeded.retry_after);
    print_json("  ", &rate_exceeded);

    // =========================================================================
    // 11. URL-safe slugs
    // =========================================================================
    println!("\n=== 11. Slugs ===");

    let slug = Slug::new("hello-world").expect("valid slug");
    println!("Slug::new(\"hello-world\"): {slug}");

    let from_title = Slug::from_title("Hello, World! 2026 — Edition #3");
    println!("Slug::from_title(\"Hello, World! 2026 — Edition #3\"): {from_title}");

    let from_empty = Slug::from_title("");
    println!("Slug::from_title(\"\"): {from_empty}");

    println!("\nInvalid slug examples:");
    for (input, expected) in [
        ("", "Empty"),
        ("Hello", "InvalidChars (uppercase)"),
        ("-leading", "LeadingHyphen"),
        ("trailing-", "TrailingHyphen"),
        ("double--hyphen", "ConsecutiveHyphens"),
    ] {
        println!("  Slug::new({input:?}) → {:?}", Slug::new(input).unwrap_err());
        let _ = expected; // suppress unused
    }

    // =========================================================================
    // 12. Query parameters
    // =========================================================================
    println!("\n=== 12. Query parameters ===");

    // Sort
    let sort_asc = SortParams::asc("created_at");
    let sort_desc = SortParams::desc("price");
    println!("SortParams::asc(\"created_at\"): sort_by={}, direction={:?}", sort_asc.sort_by, sort_asc.direction);
    println!("SortParams::desc(\"price\"):     sort_by={}, direction={:?}", sort_desc.sort_by, sort_desc.direction);
    println!("Default SortDirection: {:?}", SortDirection::default());

    // Filter
    let filters = FilterParams::new(vec![
        FilterEntry {
            field: "status".into(),
            operator: "eq".into(),
            value: "active".into(),
        },
        FilterEntry {
            field: "price".into(),
            operator: "gt".into(),
            value: "100".into(),
        },
    ]);
    println!("\nFilterParams ({} filters):", filters.filters.len());
    print_json("  ", &filters);

    // Search
    let search = SearchParams::with_fields("deluxe suite", vec!["name", "description"]);
    println!("\nSearchParams:");
    print_json("  ", &search);

    // =========================================================================
    // 13. ErrorResponse (simple model)
    // =========================================================================
    println!("\n=== 13. ErrorResponse (simple model) ===");
    let simple_err = ErrorResponse::new("Something went wrong");
    print_json("  ", &simple_err);

    println!("\n=== Done! All api-bones types demonstrated. ===");
}

fn print_json(prefix: &str, value: &impl serde::Serialize) {
    let json = serde_json::to_string_pretty(value).expect("serialization");
    for line in json.lines() {
        println!("{prefix}{line}");
    }
}
