//! Smoke tests for the `arbitrary` and `proptest` features.
//!
//! Generates 1 000 random instances of every public type and asserts that
//! no panics occur.  These tests also serve as a compile-time proof that
//! all public types implement the expected traits.

// ---------------------------------------------------------------------------
// arbitrary smoke tests
// ---------------------------------------------------------------------------

#[cfg(feature = "arbitrary")]
mod arbitrary_tests {
    use arbitrary::{Arbitrary, Unstructured};

    /// Generate `n` random instances of `T` from a pre-built byte buffer and
    /// assert that no panic occurs.
    fn smoke<T: for<'a> Arbitrary<'a>>(n: usize) {
        // A fixed seed so the test is deterministic.
        let raw: Vec<u8> = (0u8..=255).cycle().take(n * 256).collect();
        let mut u = Unstructured::new(&raw);
        for _ in 0..n {
            // Re-wrap if we run out of bytes.
            if u.is_empty() {
                u = Unstructured::new(&raw);
            }
            let _ = T::arbitrary(&mut u);
        }
    }

    #[test]
    fn smoke_error_code() {
        smoke::<api_bones::ErrorCode>(1000);
    }

    #[test]
    fn smoke_error_type_mode() {
        smoke::<api_bones::ErrorTypeMode>(1000);
    }

    #[test]
    fn smoke_validation_error() {
        smoke::<api_bones::ValidationError>(1000);
    }

    #[test]
    fn smoke_api_error() {
        smoke::<api_bones::ApiError>(1000);
    }

    #[test]
    fn smoke_etag() {
        smoke::<api_bones::ETag>(1000);
    }

    #[test]
    fn smoke_if_match() {
        smoke::<api_bones::IfMatch>(1000);
    }

    #[test]
    fn smoke_if_none_match() {
        smoke::<api_bones::IfNoneMatch>(1000);
    }

    #[test]
    fn smoke_health_status() {
        smoke::<api_bones::health::HealthStatus>(1000);
    }

    #[test]
    fn smoke_health_check() {
        smoke::<api_bones::health::HealthCheck>(1000);
    }

    #[test]
    fn smoke_liveness_response() {
        smoke::<api_bones::health::LivenessResponse>(1000);
    }

    #[test]
    fn smoke_readiness_response() {
        smoke::<api_bones::health::ReadinessResponse>(1000);
    }

    #[test]
    fn smoke_link() {
        smoke::<api_bones::links::Link>(1000);
    }

    #[test]
    fn smoke_links() {
        smoke::<api_bones::links::Links>(1000);
    }

    #[test]
    fn smoke_paginated_response() {
        smoke::<api_bones::pagination::PaginatedResponse<u32>>(1000);
    }

    #[test]
    fn smoke_pagination_params_constrained() {
        use arbitrary::{Arbitrary, Unstructured};
        let raw: Vec<u8> = (0u8..=255).cycle().take(256_000).collect();
        let mut u = Unstructured::new(&raw);
        for _ in 0..1000 {
            if u.is_empty() {
                u = Unstructured::new(&raw);
            }
            if let Ok(p) = api_bones::pagination::PaginationParams::arbitrary(&mut u) {
                // Constraint: limit must be None or Some(1..=100)
                if let Some(limit) = p.limit {
                    assert!(
                        (1..=100).contains(&limit),
                        "PaginationParams.limit out of range: {limit}"
                    );
                }
            }
        }
    }

    #[test]
    fn smoke_cursor_paginated_response() {
        smoke::<api_bones::pagination::CursorPaginatedResponse<String>>(1000);
    }

    #[test]
    fn smoke_cursor_pagination() {
        smoke::<api_bones::pagination::CursorPagination>(1000);
    }

    #[test]
    fn smoke_cursor_pagination_params_constrained() {
        use arbitrary::{Arbitrary, Unstructured};
        let raw: Vec<u8> = (0u8..=255).cycle().take(256_000).collect();
        let mut u = Unstructured::new(&raw);
        for _ in 0..1000 {
            if u.is_empty() {
                u = Unstructured::new(&raw);
            }
            if let Ok(p) = api_bones::pagination::CursorPaginationParams::arbitrary(&mut u)
                && let Some(limit) = p.limit
            {
                assert!(
                    (1..=100).contains(&limit),
                    "CursorPaginationParams.limit out of range: {limit}"
                );
            }
        }
    }

    #[test]
    fn smoke_sort_direction() {
        smoke::<api_bones::query::SortDirection>(1000);
    }

    #[test]
    fn smoke_sort_params() {
        smoke::<api_bones::query::SortParams>(1000);
    }

    #[test]
    fn smoke_filter_entry() {
        smoke::<api_bones::query::FilterEntry>(1000);
    }

    #[test]
    fn smoke_filter_params() {
        smoke::<api_bones::query::FilterParams>(1000);
    }

    #[test]
    fn smoke_search_params_constrained() {
        use arbitrary::{Arbitrary, Unstructured};
        let raw: Vec<u8> = (0u8..=255).cycle().take(256_000).collect();
        let mut u = Unstructured::new(&raw);
        for _ in 0..1000 {
            if u.is_empty() {
                u = Unstructured::new(&raw);
            }
            if let Ok(p) = api_bones::query::SearchParams::arbitrary(&mut u) {
                assert!(
                    !p.query.is_empty() && p.query.len() <= 500,
                    "SearchParams.query length out of range: {}",
                    p.query.len()
                );
            }
        }
    }

    #[test]
    fn smoke_principal() {
        smoke::<api_bones::Principal>(1000);
    }

    #[test]
    fn smoke_principal_id() {
        smoke::<api_bones::PrincipalId>(1000);
    }

    #[test]
    fn smoke_principal_kind() {
        smoke::<api_bones::PrincipalKind>(1000);
    }

    #[test]
    fn smoke_audit_info() {
        smoke::<api_bones::AuditInfo>(1000);
    }

    #[test]
    fn smoke_ratelimit_info() {
        smoke::<api_bones::ratelimit::RateLimitInfo>(1000);
    }

    #[test]
    fn smoke_response_meta() {
        smoke::<api_bones::response::ResponseMeta>(1000);
    }

    #[test]
    fn smoke_response_links() {
        smoke::<api_bones::links::Links>(1000);
    }

    #[test]
    fn smoke_api_response() {
        smoke::<api_bones::response::ApiResponse<u32>>(1000);
    }
}

// ---------------------------------------------------------------------------
// proptest smoke tests
// ---------------------------------------------------------------------------

#[cfg(feature = "proptest")]
mod proptest_tests {
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn proptest_error_code(v in any::<api_bones::ErrorCode>()) {
            let _ = v;
        }

        #[test]
        fn proptest_validation_error(v in any::<api_bones::ValidationError>()) {
            let _ = v;
        }

        #[test]
        fn proptest_api_error(v in any::<api_bones::ApiError>()) {
            let _ = v;
        }

        #[test]
        fn proptest_etag(v in any::<api_bones::ETag>()) {
            let _ = v;
        }

        #[test]
        fn proptest_health_status(v in any::<api_bones::health::HealthStatus>()) {
            let _ = v;
        }

        #[test]
        fn proptest_liveness_response(v in any::<api_bones::health::LivenessResponse>()) {
            let _ = v;
        }

        #[test]
        fn proptest_pagination_params_constrained(
            p in any::<api_bones::pagination::PaginationParams>()
        ) {
            if let Some(limit) = p.limit {
                prop_assert!(
                    (1..=100).contains(&limit),
                    "PaginationParams.limit out of range: {limit}"
                );
            }
        }

        #[test]
        fn proptest_cursor_pagination_params_constrained(
            p in any::<api_bones::pagination::CursorPaginationParams>()
        ) {
            if let Some(limit) = p.limit {
                prop_assert!(
                    (1..=100).contains(&limit),
                    "CursorPaginationParams.limit out of range: {limit}"
                );
            }
        }

        #[test]
        fn proptest_search_params_constrained(
            p in any::<api_bones::query::SearchParams>()
        ) {
            prop_assert!(
                !p.query.is_empty() && p.query.len() <= 500,
                "SearchParams.query length out of range: {}",
                p.query.len()
            );
        }

        #[test]
        fn proptest_principal(v in any::<api_bones::Principal>()) {
            let _ = v;
        }

        #[test]
        fn proptest_principal_id(v in any::<api_bones::PrincipalId>()) {
            let _ = v;
        }

        #[test]
        fn proptest_principal_kind(v in any::<api_bones::PrincipalKind>()) {
            let _ = v;
        }

        #[test]
        fn proptest_audit_info(v in any::<api_bones::AuditInfo>()) {
            let _ = v;
        }

        #[test]
        fn proptest_ratelimit_info(v in any::<api_bones::ratelimit::RateLimitInfo>()) {
            let _ = v;
        }

        #[test]
        fn proptest_response_meta(v in any::<api_bones::response::ResponseMeta>()) {
            let _ = v;
        }

        #[test]
        fn proptest_api_response(v in any::<api_bones::response::ApiResponse<u32>>()) {
            let _ = v;
        }
    }
}
