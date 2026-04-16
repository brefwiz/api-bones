//! Pagination types for list endpoints.
//!
//! Supports both offset-based and cursor-based pagination patterns.
//! All types are framework-agnostic — consumers add their own framework
//! derives (e.g. `utoipa::ToSchema`, `utoipa::IntoParams`).

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
#[cfg(feature = "validator")]
use validator::Validate;

// ---------------------------------------------------------------------------
// Offset-based pagination (flat, limit/offset contract)
// ---------------------------------------------------------------------------

/// Offset-based paginated response envelope with a flat shape.
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
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
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

impl<T> PaginatedResponse<T> {
    /// Build a paginated response from items, total count, and the query params.
    ///
    /// `has_more` is set to `true` when `offset + items.len() < total_count`.
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

#[allow(clippy::unnecessary_wraps)] // required by serde(default) which must return the field type
fn default_limit() -> Option<u64> {
    Some(20)
}

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
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema, utoipa::IntoParams))]
#[cfg_attr(feature = "validator", derive(Validate))]
pub struct PaginationParams {
    /// Maximum number of items to return (1–100). Defaults to 20.
    #[cfg_attr(feature = "serde", serde(default = "default_limit"))]
    #[cfg_attr(feature = "validator", validate(range(min = 1, max = 100)))]
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
    #[must_use]
    pub fn limit(&self) -> u64 {
        self.limit.unwrap_or(20)
    }

    /// Resolved offset value (falls back to the default of 0).
    #[must_use]
    pub fn offset(&self) -> u64 {
        self.offset.unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// Page-based pagination (legacy / page+per_page contract)
// ---------------------------------------------------------------------------

/// Page-based pagination metadata.
///
/// Used together with [`PagedResponse`] for endpoints that prefer a
/// `page` / `per_page` query contract over `limit` / `offset`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct Pagination {
    /// Total number of items across all pages.
    pub total: i64,
    /// Current page number (1-indexed).
    pub page: i64,
    /// Items per page.
    pub per_page: i64,
    /// Total number of pages.
    pub total_pages: i64,
}

impl Pagination {
    /// Compute pagination metadata from total count and params.
    #[must_use]
    pub fn new(total: i64, page: i64, per_page: i64) -> Self {
        let total_pages = if per_page > 0 {
            (total + per_page - 1) / per_page
        } else {
            0
        };
        Self {
            total,
            page,
            per_page,
            total_pages,
        }
    }
}

/// Page-based paginated response envelope.
///
/// ```json
/// {"data": [...], "pagination": {"total": 142, "page": 2, "per_page": 20, "total_pages": 8}}
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct PagedResponse<T> {
    /// The page of results.
    pub data: Vec<T>,
    /// Pagination metadata.
    pub pagination: Pagination,
}

impl<T> PagedResponse<T> {
    /// Create a new page-based paginated response.
    #[must_use]
    pub fn new(data: Vec<T>, pagination: Pagination) -> Self {
        Self { data, pagination }
    }
}

/// Query parameters for page-based list endpoints.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema, utoipa::IntoParams))]
pub struct PageParams {
    /// Page number (1-indexed). Defaults to 1.
    #[cfg_attr(feature = "serde", serde(default = "default_page"))]
    pub page: i64,
    /// Items per page. Defaults to 20.
    #[cfg_attr(feature = "serde", serde(default = "default_per_page"))]
    pub per_page: i64,
}

fn default_page() -> i64 {
    1
}

fn default_per_page() -> i64 {
    20
}

impl Default for PageParams {
    fn default() -> Self {
        Self {
            page: default_page(),
            per_page: default_per_page(),
        }
    }
}

impl PageParams {
    /// SQL-friendly offset: `(page - 1) * per_page`.
    #[must_use]
    pub fn offset(&self) -> i64 {
        (self.page - 1) * self.per_page
    }
}

// ---------------------------------------------------------------------------
// Cursor-based pagination
// ---------------------------------------------------------------------------

/// Cursor-based paginated response envelope (PLATFORM-003).
///
/// Cursor values are opaque tokens. Clients MUST NOT interpret their contents.
///
/// ```json
/// {"data": [...], "pagination": {"has_more": true, "next_cursor": "eyJpZCI6NDJ9"}}
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct CursorPaginatedResponse<T> {
    /// The page of results.
    pub data: Vec<T>,
    /// Cursor pagination metadata.
    pub pagination: CursorPagination,
}

impl<T> CursorPaginatedResponse<T> {
    /// Create a new cursor-paginated response.
    #[must_use]
    pub fn new(data: Vec<T>, pagination: CursorPagination) -> Self {
        Self { data, pagination }
    }
}

/// Cursor-based pagination metadata (PLATFORM-003).
///
/// `next_cursor` is an opaque token. Clients MUST NOT interpret its contents.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
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

impl CursorPagination {
    /// Create cursor metadata indicating more results.
    #[must_use]
    pub fn more(cursor: impl Into<String>) -> Self {
        Self {
            has_more: true,
            next_cursor: Some(cursor.into()),
        }
    }

    /// Create cursor metadata indicating this is the last page.
    #[must_use]
    pub fn last_page() -> Self {
        Self {
            has_more: false,
            next_cursor: None,
        }
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
    // Page-based types (Pagination / PagedResponse / PageParams)
    // -----------------------------------------------------------------------

    #[test]
    fn pagination_new_computes_total_pages() {
        let p = Pagination::new(142, 2, 20);
        assert_eq!(p.total_pages, 8);
        assert_eq!(p.total, 142);
        assert_eq!(p.page, 2);
        assert_eq!(p.per_page, 20);
    }

    #[test]
    fn pagination_exact_division() {
        let p = Pagination::new(100, 1, 20);
        assert_eq!(p.total_pages, 5);
    }

    #[test]
    fn pagination_zero_per_page() {
        let p = Pagination::new(100, 1, 0);
        assert_eq!(p.total_pages, 0);
    }

    #[test]
    fn pagination_zero_total() {
        let p = Pagination::new(0, 1, 20);
        assert_eq!(p.total_pages, 0);
    }

    #[test]
    fn paged_response_new() {
        let resp = PagedResponse::new(vec![1, 2, 3], Pagination::new(3, 1, 20));
        assert_eq!(resp.data.len(), 3);
        assert_eq!(resp.pagination.total, 3);
    }

    #[test]
    fn page_params_defaults() {
        let p = PageParams::default();
        assert_eq!(p.page, 1);
        assert_eq!(p.per_page, 20);
    }

    #[test]
    fn page_params_offset() {
        let p = PageParams {
            page: 3,
            per_page: 25,
        };
        assert_eq!(p.offset(), 50);
    }

    #[test]
    fn page_params_offset_first_page() {
        let p = PageParams::default();
        assert_eq!(p.offset(), 0);
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
    fn paged_response_serde_round_trip() {
        let resp = PagedResponse::new(vec![1i32, 2, 3], Pagination::new(50, 2, 10));
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["pagination"]["total"], 50);
        assert_eq!(json["pagination"]["total_pages"], 5);
        assert_eq!(json["data"], serde_json::json!([1, 2, 3]));

        let back: PagedResponse<i32> = serde_json::from_value(json).unwrap();
        assert_eq!(back, resp);
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
}
