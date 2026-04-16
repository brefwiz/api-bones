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
    smoke::<api_bones::ErrorCode, _>(200, |_| {});
}

#[test]
fn fake_error_type_mode() {
    smoke::<api_bones::ErrorTypeMode, _>(200, |_| {});
}

#[test]
fn fake_validation_error() {
    smoke::<api_bones::ValidationError, _>(200, |v| {
        assert!(!v.field.is_empty());
        assert!(!v.message.is_empty());
    });
}

#[test]
fn fake_api_error_status_in_range() {
    smoke::<api_bones::ApiError, _>(200, |v| {
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
    smoke::<api_bones::ApiError, _>(50, |v| {
        let json = serde_json::to_value(&v).expect("serialize");
        let back: api_bones::ApiError = serde_json::from_value(json).expect("deserialize");
        assert_eq!(back.status, v.status);
        assert_eq!(back.code, v.code);
    });
}

// ---------------------------------------------------------------------------
// etag module
// ---------------------------------------------------------------------------

#[test]
fn fake_etag() {
    smoke::<api_bones::ETag, _>(200, |v| {
        assert!(!v.value.is_empty());
    });
}

#[test]
fn fake_if_match() {
    smoke::<api_bones::IfMatch, _>(200, |_| {});
}

#[test]
fn fake_if_none_match() {
    smoke::<api_bones::IfNoneMatch, _>(200, |_| {});
}

// ---------------------------------------------------------------------------
// health module
// ---------------------------------------------------------------------------

#[test]
fn fake_health_status() {
    smoke::<api_bones::HealthStatus, _>(200, |_| {});
}

#[test]
fn fake_health_check() {
    smoke::<api_bones::health::HealthCheck, _>(200, |v| {
        assert!(!v.component_type.is_empty());
    });
}

#[test]
fn fake_liveness_response() {
    smoke::<api_bones::LivenessResponse, _>(200, |v| {
        assert!(!v.version.is_empty());
        assert!(!v.service_id.is_empty());
    });
}

#[test]
fn fake_readiness_response() {
    smoke::<api_bones::ReadinessResponse, _>(200, |_| {});
}

// ---------------------------------------------------------------------------
// links module
// ---------------------------------------------------------------------------

#[test]
fn fake_link() {
    smoke::<api_bones::links::Link, _>(200, |v| {
        assert!(!v.rel.is_empty());
        assert!(!v.href.is_empty());
    });
}

#[test]
fn fake_links() {
    smoke::<api_bones::links::Links, _>(200, |_| {});
}

// ---------------------------------------------------------------------------
// models module
// ---------------------------------------------------------------------------

#[test]
fn fake_error_response() {
    smoke::<api_bones::models::ErrorResponse, _>(200, |_| {});
}

// ---------------------------------------------------------------------------
// pagination module
// ---------------------------------------------------------------------------

#[test]
fn fake_paginated_response() {
    smoke::<api_bones::pagination::PaginatedResponse<u32>, _>(200, |_| {});
}

#[test]
fn fake_pagination_params_limit_in_range() {
    smoke::<api_bones::PaginationParams, _>(200, |v| {
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
    smoke::<api_bones::pagination::CursorPaginatedResponse<String>, _>(200, |_| {});
}

#[test]
fn fake_cursor_pagination() {
    smoke::<api_bones::CursorPagination, _>(200, |_| {});
}

#[test]
fn fake_cursor_pagination_params_limit_in_range() {
    smoke::<api_bones::pagination::CursorPaginationParams, _>(200, |v| {
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
    smoke::<api_bones::SortDirection, _>(200, |_| {});
}

#[test]
fn fake_sort_params() {
    smoke::<api_bones::SortParams, _>(200, |v| {
        assert!(!v.sort_by.is_empty());
    });
}

#[test]
fn fake_filter_entry() {
    smoke::<api_bones::FilterEntry, _>(200, |v| {
        assert!(!v.field.is_empty());
        assert!(!v.operator.is_empty());
    });
}

#[test]
fn fake_filter_params() {
    smoke::<api_bones::FilterParams, _>(200, |_| {});
}

#[test]
fn fake_search_params_query_constraints() {
    smoke::<api_bones::SearchParams, _>(200, |v| {
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
    smoke::<api_bones::RateLimitInfo, _>(200, |v| {
        assert!(v.remaining <= v.limit, "remaining > limit");
    });
}

// ---------------------------------------------------------------------------
// audit module
// ---------------------------------------------------------------------------

#[test]
fn fake_audit_info() {
    smoke::<api_bones::AuditInfo, _>(200, |_| {});
}

#[cfg(all(feature = "chrono", feature = "serde"))]
#[test]
fn fake_audit_info_serde_roundtrip() {
    smoke::<api_bones::AuditInfo, _>(50, |v| {
        let json = serde_json::to_value(&v).expect("serialize");
        let back: api_bones::AuditInfo = serde_json::from_value(json).expect("deserialize");
        assert_eq!(back, v);
    });
}

// ---------------------------------------------------------------------------
// response module
// ---------------------------------------------------------------------------

#[test]
fn fake_response_meta() {
    smoke::<api_bones::response::ResponseMeta, _>(200, |_| {});
}

#[test]
fn fake_response_links() {
    smoke::<api_bones::response::Links, _>(200, |_| {});
}

#[test]
fn fake_api_response() {
    smoke::<api_bones::response::ApiResponse<u32>, _>(200, |_| {});
}

// ---------------------------------------------------------------------------
// slug module
// ---------------------------------------------------------------------------

#[test]
fn fake_slug() {
    smoke::<api_bones::Slug, _>(200, |slug| {
        assert!(api_bones::Slug::new(slug.as_str()).is_ok());
        assert!(!slug.as_str().is_empty());
        assert!(slug.as_str().len() <= 128);
    });
}

// ---------------------------------------------------------------------------
// bulk module
// ---------------------------------------------------------------------------

#[test]
fn fake_bulk_request() {
    smoke::<api_bones::bulk::BulkRequest<u32>, _>(200, |v| {
        assert!(!v.items.is_empty());
    });
}

#[test]
fn fake_bulk_item_result() {
    smoke::<api_bones::bulk::BulkItemResult<u32>, _>(200, |_| {});
}

#[test]
fn fake_bulk_response() {
    smoke::<api_bones::bulk::BulkResponse<u32>, _>(200, |v| {
        assert!(!v.results.is_empty());
    });
}
