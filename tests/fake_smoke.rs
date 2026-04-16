//! Smoke tests for the `fake` feature.
//!
//! Generates 200 random instances of every public type and asserts:
//! - No panics occur.
//! - Domain invariants are preserved (status codes, query constraints, etc.).
//! - Serde round-trips succeed for all serde-enabled types.

#![cfg(feature = "fake")]

use fake::{Fake, Faker};

// ---------------------------------------------------------------------------
// Helper: generate N instances of T and run assertions
// ---------------------------------------------------------------------------

fn smoke<T, F>(n: usize, assert_fn: F)
where
    T: fake::Dummy<fake::Faker>,
    F: Fn(T),
{
    for _ in 0..n {
        let v: T = Faker.fake();
        assert_fn(v);
    }
}

// ---------------------------------------------------------------------------
// error module
// ---------------------------------------------------------------------------

#[test]
fn fake_error_code() {
    smoke::<shared_types::ErrorCode, _>(200, |_| {});
}

#[test]
fn fake_error_type_mode() {
    smoke::<shared_types::ErrorTypeMode, _>(200, |_| {});
}

#[test]
fn fake_validation_error() {
    smoke::<shared_types::ValidationError, _>(200, |v| {
        assert!(!v.field.is_empty());
        assert!(!v.message.is_empty());
    });
}

#[test]
fn fake_api_error_status_in_range() {
    smoke::<shared_types::ApiError, _>(200, |v| {
        assert!(
            (100..=599).contains(&v.status),
            "ApiError.status out of range: {}",
            v.status
        );
    });
}

#[cfg(feature = "serde")]
#[test]
fn fake_api_error_serde_roundtrip() {
    smoke::<shared_types::ApiError, _>(50, |v| {
        let json = serde_json::to_value(&v).expect("serialize");
        let back: shared_types::ApiError = serde_json::from_value(json).expect("deserialize");
        assert_eq!(back.status, v.status);
        assert_eq!(back.code, v.code);
    });
}

// ---------------------------------------------------------------------------
// etag module
// ---------------------------------------------------------------------------

#[test]
fn fake_etag() {
    smoke::<shared_types::ETag, _>(200, |v| {
        assert!(!v.value.is_empty());
    });
}

#[test]
fn fake_if_match() {
    smoke::<shared_types::IfMatch, _>(200, |_| {});
}

#[test]
fn fake_if_none_match() {
    smoke::<shared_types::IfNoneMatch, _>(200, |_| {});
}

// ---------------------------------------------------------------------------
// health module
// ---------------------------------------------------------------------------

#[test]
fn fake_health_status() {
    smoke::<shared_types::HealthStatus, _>(200, |_| {});
}

#[test]
fn fake_health_check() {
    smoke::<shared_types::health::HealthCheck, _>(200, |v| {
        assert!(!v.component_type.is_empty());
    });
}

#[test]
fn fake_liveness_response() {
    smoke::<shared_types::LivenessResponse, _>(200, |v| {
        assert!(!v.version.is_empty());
        assert!(!v.service_id.is_empty());
    });
}

#[test]
fn fake_readiness_response() {
    smoke::<shared_types::ReadinessResponse, _>(200, |_| {});
}

// ---------------------------------------------------------------------------
// links module
// ---------------------------------------------------------------------------

#[test]
fn fake_link() {
    smoke::<shared_types::links::Link, _>(200, |v| {
        assert!(!v.rel.is_empty());
        assert!(!v.href.is_empty());
    });
}

#[test]
fn fake_links() {
    smoke::<shared_types::links::Links, _>(200, |_| {});
}

// ---------------------------------------------------------------------------
// models module
// ---------------------------------------------------------------------------

#[test]
fn fake_error_response() {
    smoke::<shared_types::models::ErrorResponse, _>(200, |_| {});
}

// ---------------------------------------------------------------------------
// pagination module
// ---------------------------------------------------------------------------

#[test]
fn fake_paginated_response() {
    smoke::<shared_types::pagination::PaginatedResponse<u32>, _>(200, |_| {});
}

#[test]
fn fake_pagination_params_limit_in_range() {
    smoke::<shared_types::PaginationParams, _>(200, |v| {
        if let Some(limit) = v.limit {
            assert!(
                (1..=100).contains(&limit),
                "PaginationParams.limit out of range: {limit}"
            );
        }
    });
}

#[test]
fn fake_cursor_paginated_response() {
    smoke::<shared_types::pagination::CursorPaginatedResponse<String>, _>(200, |_| {});
}

#[test]
fn fake_cursor_pagination() {
    smoke::<shared_types::CursorPagination, _>(200, |_| {});
}

#[test]
fn fake_cursor_pagination_params_limit_in_range() {
    smoke::<shared_types::pagination::CursorPaginationParams, _>(200, |v| {
        if let Some(limit) = v.limit {
            assert!(
                (1..=100).contains(&limit),
                "CursorPaginationParams.limit out of range: {limit}"
            );
        }
    });
}

// ---------------------------------------------------------------------------
// query module
// ---------------------------------------------------------------------------

#[test]
fn fake_sort_direction() {
    smoke::<shared_types::SortDirection, _>(200, |_| {});
}

#[test]
fn fake_sort_params() {
    smoke::<shared_types::SortParams, _>(200, |v| {
        assert!(!v.sort_by.is_empty());
    });
}

#[test]
fn fake_filter_entry() {
    smoke::<shared_types::FilterEntry, _>(200, |v| {
        assert!(!v.field.is_empty());
        assert!(!v.operator.is_empty());
    });
}

#[test]
fn fake_filter_params() {
    smoke::<shared_types::FilterParams, _>(200, |_| {});
}

#[test]
fn fake_search_params_query_constraints() {
    smoke::<shared_types::SearchParams, _>(200, |v| {
        assert!(!v.query.is_empty(), "SearchParams.query must not be empty");
        assert!(
            v.query.len() <= 500,
            "SearchParams.query too long: {}",
            v.query.len()
        );
    });
}

// ---------------------------------------------------------------------------
// ratelimit module
// ---------------------------------------------------------------------------

#[test]
fn fake_ratelimit_info() {
    smoke::<shared_types::RateLimitInfo, _>(200, |v| {
        assert!(v.remaining <= v.limit, "remaining > limit");
    });
}

// ---------------------------------------------------------------------------
// audit module
// ---------------------------------------------------------------------------

#[test]
fn fake_audit_info() {
    smoke::<shared_types::AuditInfo, _>(200, |_| {});
}

#[cfg(all(feature = "chrono", feature = "serde"))]
#[test]
fn fake_audit_info_serde_roundtrip() {
    smoke::<shared_types::AuditInfo, _>(50, |v| {
        let json = serde_json::to_value(&v).expect("serialize");
        let back: shared_types::AuditInfo = serde_json::from_value(json).expect("deserialize");
        assert_eq!(back, v);
    });
}

// ---------------------------------------------------------------------------
// response module
// ---------------------------------------------------------------------------

#[test]
fn fake_response_meta() {
    smoke::<shared_types::response::ResponseMeta, _>(200, |_| {});
}

#[test]
fn fake_response_links() {
    smoke::<shared_types::response::Links, _>(200, |_| {});
}

#[test]
fn fake_api_response() {
    smoke::<shared_types::response::ApiResponse<u32>, _>(200, |_| {});
}

// ---------------------------------------------------------------------------
// bulk module
// ---------------------------------------------------------------------------

#[test]
fn fake_bulk_request() {
    smoke::<shared_types::bulk::BulkRequest<u32>, _>(200, |v| {
        assert!(!v.items.is_empty());
    });
}

#[test]
fn fake_bulk_item_result() {
    smoke::<shared_types::bulk::BulkItemResult<u32>, _>(200, |_| {});
}

#[test]
fn fake_bulk_response() {
    smoke::<shared_types::bulk::BulkResponse<u32>, _>(200, |v| {
        assert!(!v.results.is_empty());
    });
}
