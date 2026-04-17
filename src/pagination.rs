//! Pagination types for list endpoints.
//!
//! Supports both offset-based and cursor-based pagination patterns.
//! All types are framework-agnostic — consumers add their own framework
//! derives (e.g. `utoipa::ToSchema`, `utoipa::IntoParams`).
//!
//! # Choosing a pagination strategy
//!
//! ## Offset-based (`PaginatedResponse` + `PaginationParams`)
//! - Best for: admin dashboards, internal tools, small bounded datasets
//! - Supports: random page access (jump to page N), total count
//! - Trade-off: pages can shift when rows are inserted/deleted between requests
//! - Use when: dataset is small (<10k rows), real-time consistency is not critical
//!
//! ## Cursor-based (`CursorPaginatedResponse` + `CursorPaginationParams`)
//! - Best for: public APIs, feeds, large or live datasets
//! - Supports: stable iteration (no skipped/duplicate items on insert)
//! - Trade-off: no random page access, no total count
//! - Use when: dataset is large or frequently mutated, API is public-facing
//! - Industry standard: Stripe, GitHub, Slack all use cursor-based for list endpoints

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{string::String, vec::Vec};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
#[cfg(feature = "validator")]
use validator::Validate;

// ---------------------------------------------------------------------------
// Offset-based pagination (flat, limit/offset contract)
// ---------------------------------------------------------------------------

/// Offset-based paginated response envelope with a flat shape.
///
/// Requires `std` or `alloc`.
///
/// All list endpoints that use `PaginationParams` should wrap their result
/// with this type so SDK consumers always see the same contract.
///
/// ```json
/// {
///   "items": [...],
///   "total_count": 42,
///   "has_more": true,
///   "limit": 20,
///   "offset": 0
/// }
/// ```
#[cfg(any(feature = "std", feature = "alloc"))]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "proptest", derive(proptest_derive::Arbitrary))]
pub struct PaginatedResponse<T> {
    /// The items on this page.
    pub items: Vec<T>,
    /// Total number of items across all pages.
    pub total_count: u64,
    /// Whether more items exist beyond this page.
    pub has_more: bool,
    /// Maximum number of items returned per page.
    pub limit: u64,
    /// Number of items skipped before this page.
    pub offset: u64,
}

#[cfg(any(feature = "std", feature = "alloc"))]
impl<T> PaginatedResponse<T> {
    /// Build a paginated response from items, total count, and the query params.
    ///
    /// `has_more` is set to `true` when `offset + items.len() < total_count`.
    ///
    /// # Examples
    ///
    /// ```
    /// use api_bones::pagination::{PaginatedResponse, PaginationParams};
    ///
    /// let params = PaginationParams::default();
    /// let resp = PaginatedResponse::new(vec![1, 2, 3], 25, &params);
    /// assert!(resp.has_more);
    /// assert_eq!(resp.total_count, 25);
    /// assert_eq!(resp.limit, 20);
    /// assert_eq!(resp.offset, 0);
    ///
    /// let resp = PaginatedResponse::new(vec![1, 2, 3], 3, &params);
    /// assert!(!resp.has_more);
    /// ```
    #[must_use]
    pub fn new(items: Vec<T>, total_count: u64, params: &PaginationParams) -> Self {
        let limit = params.limit();
        let offset = params.offset();
        let has_more = offset + (items.len() as u64) < total_count;
        Self {
            items,
            total_count,
            has_more,
            limit,
            offset,
        }
    }
}

#[cfg(feature = "serde")]
#[allow(clippy::unnecessary_wraps)] // required by serde(default) which must return the field type
fn default_limit() -> Option<u64> {
    Some(20)
}

#[cfg(feature = "serde")]
#[allow(clippy::unnecessary_wraps)] // required by serde(default) which must return the field type
fn default_offset() -> Option<u64> {
    Some(0)
}

/// Query parameters for offset-based list endpoints.
///
/// `limit` must be between 1 and 100 (inclusive) and defaults to 20.
/// `offset` defaults to 0.
///
/// When the `validator` feature is enabled (the default), calling
/// `.validate()` enforces these constraints before the values are used.
///
/// # Examples
///
/// ```
/// use api_bones::pagination::PaginationParams;
///
/// let p = PaginationParams::default();
/// assert_eq!(p.limit(), 20);
/// assert_eq!(p.offset(), 0);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema, utoipa::IntoParams))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "validator", derive(Validate))]
#[cfg_attr(feature = "proptest", derive(proptest_derive::Arbitrary))]
pub struct PaginationParams {
    /// Maximum number of items to return (1–100). Defaults to 20.
    #[cfg_attr(feature = "serde", serde(default = "default_limit"))]
    #[cfg_attr(feature = "validator", validate(range(min = 1, max = 100)))]
    #[cfg_attr(
        feature = "proptest",
        proptest(strategy = "proptest::option::of(1u64..=100u64)")
    )]
    pub limit: Option<u64>,
    /// Number of items to skip. Defaults to 0.
    #[cfg_attr(feature = "serde", serde(default = "default_offset"))]
    pub offset: Option<u64>,
}

impl Default for PaginationParams {
    fn default() -> Self {
        Self {
            limit: Some(20),
            offset: Some(0),
        }
    }
}

impl PaginationParams {
    /// Resolved limit value (falls back to the default of 20).
    ///
    /// # Examples
    ///
    /// ```
    /// use api_bones::pagination::PaginationParams;
    ///
    /// let p = PaginationParams { limit: None, offset: None };
    /// assert_eq!(p.limit(), 20);
    ///
    /// let p = PaginationParams { limit: Some(50), offset: None };
    /// assert_eq!(p.limit(), 50);
    /// ```
    #[must_use]
    pub fn limit(&self) -> u64 {
        self.limit.unwrap_or(20)
    }

    /// Resolved offset value (falls back to the default of 0).
    ///
    /// # Examples
    ///
    /// ```
    /// use api_bones::pagination::PaginationParams;
    ///
    /// let p = PaginationParams { limit: None, offset: None };
    /// assert_eq!(p.offset(), 0);
    ///
    /// let p = PaginationParams { limit: None, offset: Some(100) };
    /// assert_eq!(p.offset(), 100);
    /// ```
    #[must_use]
    pub fn offset(&self) -> u64 {
        self.offset.unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// Cursor-based pagination
// ---------------------------------------------------------------------------

/// Cursor-based paginated response envelope (PLATFORM-003).
///
/// Requires `std` or `alloc`.
///
/// Cursor values are opaque tokens. Clients MUST NOT interpret their contents.
///
/// ```json
/// {"data": [...], "pagination": {"has_more": true, "next_cursor": "eyJpZCI6NDJ9"}}
/// ```
#[cfg(any(feature = "std", feature = "alloc"))]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "proptest", derive(proptest_derive::Arbitrary))]
pub struct CursorPaginatedResponse<T> {
    /// The page of results.
    pub data: Vec<T>,
    /// Cursor pagination metadata.
    pub pagination: CursorPagination,
}

#[cfg(any(feature = "std", feature = "alloc"))]
impl<T> CursorPaginatedResponse<T> {
    /// Create a new cursor-paginated response.
    ///
    /// # Examples
    ///
    /// ```
    /// use api_bones::pagination::{CursorPaginatedResponse, CursorPagination};
    ///
    /// let resp = CursorPaginatedResponse::new(
    ///     vec!["a", "b"],
    ///     CursorPagination::more("next_token"),
    /// );
    /// assert_eq!(resp.data.len(), 2);
    /// assert!(resp.pagination.has_more);
    /// ```
    #[must_use]
    pub fn new(data: Vec<T>, pagination: CursorPagination) -> Self {
        Self { data, pagination }
    }
}

/// Cursor-based pagination metadata (PLATFORM-003).
///
/// Requires `std` or `alloc`.
///
/// `next_cursor` is an opaque token. Clients MUST NOT interpret its contents.
#[cfg(any(feature = "std", feature = "alloc"))]
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "proptest", derive(proptest_derive::Arbitrary))]
pub struct CursorPagination {
    /// Whether more results exist beyond this page.
    pub has_more: bool,
    /// Opaque cursor for the next page. `None` when `has_more` is false.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub next_cursor: Option<String>,
}

#[cfg(any(feature = "std", feature = "alloc"))]
impl CursorPagination {
    /// Create cursor metadata indicating more results.
    ///
    /// # Examples
    ///
    /// ```
    /// use api_bones::pagination::CursorPagination;
    ///
    /// let c = CursorPagination::more("eyJpZCI6NDJ9");
    /// assert!(c.has_more);
    /// assert_eq!(c.next_cursor.as_deref(), Some("eyJpZCI6NDJ9"));
    /// ```
    #[must_use]
    pub fn more(cursor: impl Into<String>) -> Self {
        Self {
            has_more: true,
            next_cursor: Some(cursor.into()),
        }
    }

    /// Create cursor metadata indicating this is the last page.
    ///
    /// # Examples
    ///
    /// ```
    /// use api_bones::pagination::CursorPagination;
    ///
    /// let c = CursorPagination::last_page();
    /// assert!(!c.has_more);
    /// assert!(c.next_cursor.is_none());
    /// ```
    #[must_use]
    pub fn last_page() -> Self {
        Self {
            has_more: false,
            next_cursor: None,
        }
    }
}

#[cfg(all(feature = "serde", any(feature = "std", feature = "alloc")))]
#[allow(clippy::unnecessary_wraps)]
fn default_cursor_limit() -> Option<u64> {
    Some(20)
}

/// Query parameters for cursor-based list endpoints.
///
/// `limit` must be between 1 and 100 (inclusive) and defaults to 20.
/// `after` is an opaque cursor token; omit it (or pass `None`) for the first page.
///
/// Requires `std` or `alloc` (`after` field contains `String`).
#[cfg(any(feature = "std", feature = "alloc"))]
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema, utoipa::IntoParams))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "validator", derive(Validate))]
#[cfg_attr(feature = "proptest", derive(proptest_derive::Arbitrary))]
pub struct CursorPaginationParams {
    /// Maximum number of items to return (1–100). Defaults to 20.
    #[cfg_attr(feature = "serde", serde(default = "default_cursor_limit"))]
    #[cfg_attr(feature = "validator", validate(range(min = 1, max = 100)))]
    #[cfg_attr(
        feature = "proptest",
        proptest(strategy = "proptest::option::of(1u64..=100u64)")
    )]
    pub limit: Option<u64>,
    /// Opaque cursor for the next page. `None` requests the first page.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub after: Option<String>,
}

#[cfg(any(feature = "std", feature = "alloc"))]
impl Default for CursorPaginationParams {
    fn default() -> Self {
        Self {
            limit: Some(20),
            after: None,
        }
    }
}

#[cfg(any(feature = "std", feature = "alloc"))]
impl CursorPaginationParams {
    /// Resolved limit value (falls back to the default of 20).
    ///
    /// # Examples
    ///
    /// ```
    /// use api_bones::pagination::CursorPaginationParams;
    ///
    /// let p = CursorPaginationParams::default();
    /// assert_eq!(p.limit(), 20);
    ///
    /// let p = CursorPaginationParams { limit: Some(50), after: None };
    /// assert_eq!(p.limit(), 50);
    /// ```
    #[must_use]
    pub fn limit(&self) -> u64 {
        self.limit.unwrap_or(20)
    }

    /// The cursor token, if any.
    #[must_use]
    pub fn after(&self) -> Option<&str> {
        self.after.as_deref()
    }
}

// ---------------------------------------------------------------------------
// Keyset (seek) pagination
// ---------------------------------------------------------------------------

/// Query parameters for keyset (seek-based) pagination.
///
/// Keyset pagination is more efficient than offset pagination for large datasets
/// because the database anchors the query on an indexed column value rather than
/// skipping rows.
///
/// - `after` — fetch items whose sort key is **greater than** this value
/// - `before` — fetch items whose sort key is **less than** this value
/// - `limit` — maximum number of items to return (1–100, default 20)
///
/// Typically only one of `after` / `before` is supplied per request.
///
/// Requires `std` or `alloc`.
#[cfg(any(feature = "std", feature = "alloc"))]
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schemars", schemars(bound = "K: schemars::JsonSchema"))]
#[cfg_attr(feature = "validator", derive(Validate))]
pub struct KeysetPaginationParams<K> {
    /// Fetch items after (exclusive) this key value.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub after: Option<K>,
    /// Fetch items before (exclusive) this key value.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub before: Option<K>,
    /// Maximum number of items to return (1–100). Defaults to 20.
    #[cfg_attr(feature = "serde", serde(default = "default_keyset_limit"))]
    #[cfg_attr(feature = "validator", validate(range(min = 1, max = 100)))]
    pub limit: Option<u64>,
}

#[cfg(any(feature = "std", feature = "alloc"))]
impl<K> Default for KeysetPaginationParams<K> {
    fn default() -> Self {
        Self {
            after: None,
            before: None,
            limit: Some(20),
        }
    }
}

#[cfg(any(feature = "std", feature = "alloc"))]
impl<K> KeysetPaginationParams<K> {
    /// Resolved limit value (falls back to 20).
    ///
    /// # Examples
    ///
    /// ```
    /// use api_bones::pagination::KeysetPaginationParams;
    ///
    /// let p = KeysetPaginationParams::<String>::default();
    /// assert_eq!(p.limit(), 20);
    /// ```
    #[must_use]
    pub fn limit(&self) -> u64 {
        self.limit.unwrap_or(20)
    }
}

#[cfg(all(feature = "serde", any(feature = "std", feature = "alloc")))]
#[allow(clippy::unnecessary_wraps)]
fn default_keyset_limit() -> Option<u64> {
    Some(20)
}

/// A page of results from a keyset-paginated endpoint.
///
/// `has_next` / `has_prev` reflect whether further pages exist in each direction.
/// Cursors for navigation are opaque strings — typically the serialised key of
/// the first/last item in `items`.
///
/// Requires `std` or `alloc`.
#[cfg(any(feature = "std", feature = "alloc"))]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct KeysetPaginatedResponse<T> {
    /// The items on this page.
    pub items: Vec<T>,
    /// Whether a next page exists (there are items after the last item).
    pub has_next: bool,
    /// Whether a previous page exists (there are items before the first item).
    pub has_prev: bool,
    /// Opaque cursor pointing to the item just before the first item in `items`.
    ///
    /// Pass this as `before` to retrieve the previous page.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub prev_cursor: Option<String>,
    /// Opaque cursor pointing to the item just after the last item in `items`.
    ///
    /// Pass this as `after` to retrieve the next page.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub next_cursor: Option<String>,
}

#[cfg(any(feature = "std", feature = "alloc"))]
impl<T> KeysetPaginatedResponse<T> {
    /// Create a new keyset-paginated response.
    ///
    /// # Examples
    ///
    /// ```
    /// use api_bones::pagination::KeysetPaginatedResponse;
    ///
    /// let resp = KeysetPaginatedResponse::new(
    ///     vec![1i32, 2, 3],
    ///     true,
    ///     false,
    ///     None,
    ///     Some("cursor_after_3".to_string()),
    /// );
    /// assert!(resp.has_next);
    /// assert!(!resp.has_prev);
    /// ```
    #[must_use]
    pub fn new(
        items: Vec<T>,
        has_next: bool,
        has_prev: bool,
        prev_cursor: Option<String>,
        next_cursor: Option<String>,
    ) -> Self {
        Self {
            items,
            has_next,
            has_prev,
            prev_cursor,
            next_cursor,
        }
    }

    /// Convenience: first page with no previous cursor.
    ///
    /// # Examples
    ///
    /// ```
    /// use api_bones::pagination::KeysetPaginatedResponse;
    ///
    /// let resp = KeysetPaginatedResponse::first_page(
    ///     vec!["a", "b", "c"],
    ///     true,
    ///     Some("cursor_after_c".to_string()),
    /// );
    /// assert!(!resp.has_prev);
    /// assert!(resp.has_next);
    /// ```
    #[must_use]
    pub fn first_page(items: Vec<T>, has_next: bool, next_cursor: Option<String>) -> Self {
        Self::new(items, has_next, false, None, next_cursor)
    }
}

// ---------------------------------------------------------------------------
// Axum extractors — `axum` feature
// ---------------------------------------------------------------------------

#[cfg(feature = "axum")]
#[allow(clippy::result_large_err)]
mod axum_extractors {
    use super::{CursorPaginationParams, PaginationParams};
    use crate::error::ApiError;
    use axum::extract::{FromRequestParts, Query};
    use axum::http::request::Parts;
    #[cfg(feature = "validator")]
    use validator::Validate;

    impl<S: Send + Sync> FromRequestParts<S> for PaginationParams {
        type Rejection = ApiError;

        async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
            let Query(params) = Query::<Self>::from_request_parts(parts, state)
                .await
                .map_err(|e| ApiError::bad_request(e.to_string()))?;
            #[cfg(feature = "validator")]
            params
                .validate()
                .map_err(|e| ApiError::bad_request(e.to_string()))?;
            Ok(params)
        }
    }

    impl<S: Send + Sync> FromRequestParts<S> for CursorPaginationParams {
        type Rejection = ApiError;

        async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
            let Query(params) = Query::<Self>::from_request_parts(parts, state)
                .await
                .map_err(|e| ApiError::bad_request(e.to_string()))?;
            #[cfg(feature = "validator")]
            params
                .validate()
                .map_err(|e| ApiError::bad_request(e.to_string()))?;
            Ok(params)
        }
    }
}

// ---------------------------------------------------------------------------
// arbitrary::Arbitrary manual impls — constrained limit (1–100)
// ---------------------------------------------------------------------------

#[cfg(feature = "arbitrary")]
impl<'a> arbitrary::Arbitrary<'a> for PaginationParams {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        use arbitrary::Arbitrary;
        // limit is None or Some(1..=100)
        let limit = if bool::arbitrary(u)? {
            Some(u.int_in_range(1u64..=100)?)
        } else {
            None
        };
        Ok(Self {
            limit,
            offset: Arbitrary::arbitrary(u)?,
        })
    }
}

#[cfg(all(feature = "arbitrary", any(feature = "std", feature = "alloc")))]
impl<'a> arbitrary::Arbitrary<'a> for CursorPaginationParams {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        use arbitrary::Arbitrary;
        let limit = if bool::arbitrary(u)? {
            Some(u.int_in_range(1u64..=100)?)
        } else {
            None
        };
        Ok(Self {
            limit,
            after: Arbitrary::arbitrary(u)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // PaginatedResponse::new — has_more logic
    // -----------------------------------------------------------------------

    #[test]
    fn paginated_response_has_more_true() {
        let params = PaginationParams::default();
        let resp = PaginatedResponse::new(vec![1i32; 20], 25, &params);
        assert!(resp.has_more);
        assert_eq!(resp.total_count, 25);
        assert_eq!(resp.limit, 20);
        assert_eq!(resp.offset, 0);
    }

    #[test]
    fn paginated_response_has_more_false() {
        let params = PaginationParams::default();
        let resp = PaginatedResponse::new(vec![1i32; 5], 5, &params);
        assert!(!resp.has_more);
    }

    #[test]
    fn paginated_response_exact_last_page_boundary() {
        // offset=20, 5 items, total=25 → offset(20) + items(5) == total(25) → no more
        let params = PaginationParams {
            limit: Some(20),
            offset: Some(20),
        };
        let resp = PaginatedResponse::new(vec![1i32; 5], 25, &params);
        assert!(!resp.has_more);
    }

    #[test]
    fn paginated_response_second_page_has_more() {
        let params = PaginationParams {
            limit: Some(10),
            offset: Some(10),
        };
        let resp = PaginatedResponse::new(vec![1i32; 10], 50, &params);
        assert!(resp.has_more);
    }

    // -----------------------------------------------------------------------
    // PaginationParams defaults and accessors
    // -----------------------------------------------------------------------

    #[test]
    fn pagination_params_defaults() {
        let p = PaginationParams::default();
        assert_eq!(p.limit(), 20);
        assert_eq!(p.offset(), 0);
    }

    #[test]
    fn pagination_params_none_falls_back_to_defaults() {
        let p = PaginationParams {
            limit: None,
            offset: None,
        };
        assert_eq!(p.limit(), 20);
        assert_eq!(p.offset(), 0);
    }

    #[test]
    fn pagination_params_custom_values() {
        let p = PaginationParams {
            limit: Some(50),
            offset: Some(100),
        };
        assert_eq!(p.limit(), 50);
        assert_eq!(p.offset(), 100);
    }

    // -----------------------------------------------------------------------
    // validator feature — range constraints
    // -----------------------------------------------------------------------

    #[cfg(feature = "validator")]
    #[test]
    fn pagination_params_validate_min_limit() {
        use validator::Validate;
        let p = PaginationParams {
            limit: Some(0),
            offset: Some(0),
        };
        assert!(p.validate().is_err());
    }

    #[cfg(feature = "validator")]
    #[test]
    fn pagination_params_validate_max_limit() {
        use validator::Validate;
        let p = PaginationParams {
            limit: Some(101),
            offset: Some(0),
        };
        assert!(p.validate().is_err());
    }

    #[cfg(feature = "validator")]
    #[test]
    fn pagination_params_validate_boundary_values() {
        use validator::Validate;
        let min = PaginationParams {
            limit: Some(1),
            offset: Some(0),
        };
        assert!(min.validate().is_ok());
        let max = PaginationParams {
            limit: Some(100),
            offset: Some(0),
        };
        assert!(max.validate().is_ok());
    }

    #[cfg(feature = "validator")]
    #[test]
    fn pagination_params_validate_none_limit_uses_default() {
        use validator::Validate;
        // None is treated as default (20) — no field to validate → ok
        let p = PaginationParams {
            limit: None,
            offset: None,
        };
        assert!(p.validate().is_ok());
    }

    // -----------------------------------------------------------------------
    // Cursor-based types
    // -----------------------------------------------------------------------

    #[test]
    fn cursor_pagination_more() {
        let c = CursorPagination::more("abc123");
        assert!(c.has_more);
        assert_eq!(c.next_cursor.as_deref(), Some("abc123"));
    }

    #[test]
    fn cursor_pagination_last() {
        let c = CursorPagination::last_page();
        assert!(!c.has_more);
        assert!(c.next_cursor.is_none());
    }

    #[test]
    fn cursor_paginated_response_new() {
        let resp = CursorPaginatedResponse::new(vec!["a", "b"], CursorPagination::more("next"));
        assert_eq!(resp.data.len(), 2);
        assert!(resp.pagination.has_more);
    }

    // -----------------------------------------------------------------------
    // Serde round-trips
    // -----------------------------------------------------------------------

    #[cfg(feature = "serde")]
    #[test]
    fn paginated_response_serde_round_trip() {
        let params = PaginationParams {
            limit: Some(10),
            offset: Some(20),
        };
        let resp = PaginatedResponse::new(vec![1i32, 2, 3], 50, &params);
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["total_count"], 50);
        assert_eq!(json["has_more"], true);
        assert_eq!(json["limit"], 10);
        assert_eq!(json["offset"], 20);
        assert_eq!(json["items"], serde_json::json!([1, 2, 3]));

        let back: PaginatedResponse<i32> = serde_json::from_value(json).unwrap();
        assert_eq!(back, resp);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn snapshot_offset_paginated_response() {
        let params = PaginationParams {
            limit: Some(20),
            offset: Some(0),
        };
        let resp = PaginatedResponse::new(vec![1i32, 2, 3], 25, &params);
        let json = serde_json::to_value(&resp).unwrap();
        let expected = serde_json::json!({
            "items": [1, 2, 3],
            "total_count": 25,
            "has_more": true,
            "limit": 20,
            "offset": 0
        });
        assert_eq!(json, expected);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn pagination_params_serde_defaults() {
        let json = serde_json::json!({});
        let p: PaginationParams = serde_json::from_value(json).unwrap();
        assert_eq!(p.limit(), 20);
        assert_eq!(p.offset(), 0);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn pagination_params_serde_custom() {
        let json = serde_json::json!({"limit": 50, "offset": 100});
        let p: PaginationParams = serde_json::from_value(json).unwrap();
        assert_eq!(p.limit(), 50);
        assert_eq!(p.offset(), 100);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn cursor_response_serde_omits_null_cursor() {
        let resp = CursorPaginatedResponse::new(vec!["x"], CursorPagination::last_page());
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json["pagination"].get("next_cursor").is_none());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn cursor_response_serde_includes_cursor() {
        let resp = CursorPaginatedResponse::new(vec!["x"], CursorPagination::more("eyJpZCI6NDJ9"));
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["pagination"]["next_cursor"], "eyJpZCI6NDJ9");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn snapshot_cursor_paginated_response() {
        let resp =
            CursorPaginatedResponse::new(vec!["a", "b"], CursorPagination::more("eyJpZCI6NDJ9"));
        let json = serde_json::to_value(&resp).unwrap();
        let expected = serde_json::json!({
            "data": ["a", "b"],
            "pagination": {
                "has_more": true,
                "next_cursor": "eyJpZCI6NDJ9"
            }
        });
        assert_eq!(json, expected);
    }

    // -----------------------------------------------------------------------
    // CursorPaginationParams defaults and accessors
    // -----------------------------------------------------------------------

    #[test]
    fn cursor_pagination_params_defaults() {
        let p = CursorPaginationParams::default();
        assert_eq!(p.limit(), 20);
        assert!(p.after().is_none());
    }

    #[test]
    fn cursor_pagination_params_none_falls_back_to_defaults() {
        let p = CursorPaginationParams {
            limit: None,
            after: None,
        };
        assert_eq!(p.limit(), 20);
        assert!(p.after().is_none());
    }

    #[test]
    fn cursor_pagination_params_custom_values() {
        let p = CursorPaginationParams {
            limit: Some(50),
            after: Some("eyJpZCI6NDJ9".to_string()),
        };
        assert_eq!(p.limit(), 50);
        assert_eq!(p.after(), Some("eyJpZCI6NDJ9"));
    }

    // -----------------------------------------------------------------------
    // CursorPaginationParams — validator feature
    // -----------------------------------------------------------------------

    #[cfg(feature = "validator")]
    #[test]
    fn cursor_pagination_params_validate_min_limit() {
        use validator::Validate;
        let p = CursorPaginationParams {
            limit: Some(0),
            after: None,
        };
        assert!(p.validate().is_err());
    }

    #[cfg(feature = "validator")]
    #[test]
    fn cursor_pagination_params_validate_max_limit() {
        use validator::Validate;
        let p = CursorPaginationParams {
            limit: Some(101),
            after: None,
        };
        assert!(p.validate().is_err());
    }

    #[cfg(feature = "validator")]
    #[test]
    fn cursor_pagination_params_validate_boundary_values() {
        use validator::Validate;
        let min = CursorPaginationParams {
            limit: Some(1),
            after: None,
        };
        assert!(min.validate().is_ok());
        let max = CursorPaginationParams {
            limit: Some(100),
            after: None,
        };
        assert!(max.validate().is_ok());
    }

    // -----------------------------------------------------------------------
    // CursorPaginationParams — serde feature
    // -----------------------------------------------------------------------

    #[cfg(feature = "serde")]
    #[test]
    fn cursor_pagination_params_serde_defaults() {
        let json = serde_json::json!({});
        let p: CursorPaginationParams = serde_json::from_value(json).unwrap();
        assert_eq!(p.limit(), 20);
        assert!(p.after().is_none());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn cursor_pagination_params_serde_custom() {
        let json = serde_json::json!({"limit": 50, "after": "eyJpZCI6NDJ9"});
        let p: CursorPaginationParams = serde_json::from_value(json).unwrap();
        assert_eq!(p.limit(), 50);
        assert_eq!(p.after(), Some("eyJpZCI6NDJ9"));
    }

    #[cfg(feature = "schemars")]
    #[test]
    fn pagination_params_schema_is_valid() {
        let schema = schemars::schema_for!(PaginationParams);
        let json = serde_json::to_value(&schema).expect("schema serializable");
        assert!(json.is_object());
    }

    #[cfg(all(feature = "schemars", any(feature = "std", feature = "alloc")))]
    #[test]
    fn cursor_pagination_schema_is_valid() {
        let schema = schemars::schema_for!(CursorPagination);
        let json = serde_json::to_value(&schema).expect("schema serializable");
        assert!(json.is_object());
    }

    #[cfg(feature = "axum")]
    mod axum_extractor_tests {
        use super::super::{CursorPaginationParams, PaginationParams};
        use axum::extract::FromRequestParts;
        use axum::http::Request;

        async fn extract_offset(q: &str) -> Result<PaginationParams, u16> {
            let req = Request::builder().uri(format!("/?{q}")).body(()).unwrap();
            let (mut parts, ()) = req.into_parts();
            PaginationParams::from_request_parts(&mut parts, &())
                .await
                .map_err(|e| e.status)
        }

        async fn extract_cursor(q: &str) -> Result<CursorPaginationParams, u16> {
            let req = Request::builder().uri(format!("/?{q}")).body(()).unwrap();
            let (mut parts, ()) = req.into_parts();
            CursorPaginationParams::from_request_parts(&mut parts, &())
                .await
                .map_err(|e| e.status)
        }

        #[tokio::test]
        async fn default_params() {
            let p = extract_offset("").await.unwrap();
            assert_eq!(p.limit(), 20);
            assert_eq!(p.offset(), 0);
        }

        #[tokio::test]
        async fn custom_params() {
            let p = extract_offset("limit=50&offset=100").await.unwrap();
            assert_eq!(p.limit(), 50);
            assert_eq!(p.offset(), 100);
        }

        #[cfg(feature = "validator")]
        #[tokio::test]
        async fn limit_zero_rejected() {
            assert_eq!(extract_offset("limit=0").await.unwrap_err(), 400);
        }

        #[cfg(feature = "validator")]
        #[tokio::test]
        async fn limit_101_rejected() {
            assert_eq!(extract_offset("limit=101").await.unwrap_err(), 400);
        }

        #[tokio::test]
        async fn cursor_default() {
            let p = extract_cursor("").await.unwrap();
            assert_eq!(p.limit(), 20);
            assert!(p.after().is_none());
        }

        #[tokio::test]
        async fn cursor_custom() {
            let p = extract_cursor("limit=10&after=abc").await.unwrap();
            assert_eq!(p.limit(), 10);
            assert_eq!(p.after(), Some("abc"));
        }

        #[cfg(feature = "validator")]
        #[tokio::test]
        async fn cursor_limit_101_rejected() {
            assert_eq!(extract_cursor("limit=101").await.unwrap_err(), 400);
        }

        #[tokio::test]
        async fn offset_invalid_query_type_rejected() {
            // Non-numeric limit fails axum Query deserialization → 400 branch.
            assert_eq!(extract_offset("limit=abc").await.unwrap_err(), 400);
        }

        #[tokio::test]
        async fn cursor_invalid_query_type_rejected() {
            assert_eq!(extract_cursor("limit=abc").await.unwrap_err(), 400);
        }
    }

    #[cfg(all(feature = "schemars", any(feature = "std", feature = "alloc")))]
    #[test]
    fn paginated_response_schema_is_valid() {
        let schema = schemars::schema_for!(PaginatedResponse<String>);
        let json = serde_json::to_value(&schema).expect("schema serializable");
        assert!(json.is_object());
    }
}
