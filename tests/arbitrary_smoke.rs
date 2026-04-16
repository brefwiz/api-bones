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
        smoke::<shared_types::ErrorCode>(1000);
    }

    #[test]
    fn smoke_error_type_mode() {
        smoke::<shared_types::ErrorTypeMode>(1000);
    }

    #[test]
    fn smoke_validation_error() {
        smoke::<shared_types::ValidationError>(1000);
    }

    #[test]
    fn smoke_api_error() {
        smoke::<shared_types::ApiError>(1000);
    }

    #[test]
    fn smoke_etag() {
        smoke::<shared_types::ETag>(1000);
    }

    #[test]
    fn smoke_if_match() {
        smoke::<shared_types::IfMatch>(1000);
    }

    #[test]
    fn smoke_if_none_match() {
        smoke::<shared_types::IfNoneMatch>(1000);
    }

    #[test]
    fn smoke_health_status() {
        smoke::<shared_types::health::HealthStatus>(1000);
    }

    #[test]
    fn smoke_health_check() {
        smoke::<shared_types::health::HealthCheck>(1000);
    }

    #[test]
    fn smoke_liveness_response() {
        smoke::<shared_types::health::LivenessResponse>(1000);
    }

    #[test]
    fn smoke_readiness_response() {
        smoke::<shared_types::health::ReadinessResponse>(1000);
    }

    #[test]
    fn smoke_link() {
        smoke::<shared_types::links::Link>(1000);
    }

    #[test]
    fn smoke_links() {
        smoke::<shared_types::links::Links>(1000);
    }

    #[test]
    fn smoke_error_response() {
        smoke::<shared_types::models::ErrorResponse>(1000);
    }

    #[test]
    fn smoke_paginated_response() {
        smoke::<shared_types::pagination::PaginatedResponse<u32>>(1000);
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
            if let Ok(p) = shared_types::pagination::PaginationParams::arbitrary(&mut u) {
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
        smoke::<shared_types::pagination::CursorPaginatedResponse<String>>(1000);
    }

    #[test]
    fn smoke_cursor_pagination() {
        smoke::<shared_types::pagination::CursorPagination>(1000);
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
            if let Ok(p) = shared_types::pagination::CursorPaginationParams::arbitrary(&mut u)
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
        smoke::<shared_types::query::SortDirection>(1000);
    }

    #[test]
    fn smoke_sort_params() {
        smoke::<shared_types::query::SortParams>(1000);
    }

    #[test]
    fn smoke_filter_entry() {
        smoke::<shared_types::query::FilterEntry>(1000);
    }

    #[test]
    fn smoke_filter_params() {
        smoke::<shared_types::query::FilterParams>(1000);
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
            if let Ok(p) = shared_types::query::SearchParams::arbitrary(&mut u) {
                assert!(
                    !p.query.is_empty() && p.query.len() <= 500,
                    "SearchParams.query length out of range: {}",
                    p.query.len()
                );
            }
        }
    }

    #[test]
    fn smoke_ratelimit_info() {
        smoke::<shared_types::ratelimit::RateLimitInfo>(1000);
    }

    #[test]
    fn smoke_response_meta() {
        smoke::<shared_types::response::ResponseMeta>(1000);
    }

    #[test]
    fn smoke_response_links() {
        smoke::<shared_types::response::Links>(1000);
    }

    #[test]
    fn smoke_api_response() {
        smoke::<shared_types::response::ApiResponse<u32>>(1000);
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
        fn proptest_error_code(v in any::<shared_types::ErrorCode>()) {
            let _ = v;
        }

        #[test]
        fn proptest_validation_error(v in any::<shared_types::ValidationError>()) {
            let _ = v;
        }

        #[test]
        fn proptest_api_error(v in any::<shared_types::ApiError>()) {
            let _ = v;
        }

        #[test]
        fn proptest_etag(v in any::<shared_types::ETag>()) {
            let _ = v;
        }

        #[test]
        fn proptest_health_status(v in any::<shared_types::health::HealthStatus>()) {
            let _ = v;
        }

        #[test]
        fn proptest_liveness_response(v in any::<shared_types::health::LivenessResponse>()) {
            let _ = v;
        }

        #[test]
        fn proptest_pagination_params_constrained(
            p in any::<shared_types::pagination::PaginationParams>()
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
            p in any::<shared_types::pagination::CursorPaginationParams>()
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
            p in any::<shared_types::query::SearchParams>()
        ) {
            prop_assert!(
                !p.query.is_empty() && p.query.len() <= 500,
                "SearchParams.query length out of range: {}",
                p.query.len()
            );
        }

        #[test]
        fn proptest_ratelimit_info(v in any::<shared_types::ratelimit::RateLimitInfo>()) {
            let _ = v;
        }

        #[test]
        fn proptest_response_meta(v in any::<shared_types::response::ResponseMeta>()) {
            let _ = v;
        }

        #[test]
        fn proptest_api_response(v in any::<shared_types::response::ApiResponse<u32>>()) {
            let _ = v;
        }
    }
}
