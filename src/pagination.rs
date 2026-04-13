//! Pagination types for list endpoints.
//!
//! Supports both offset-based and cursor-based pagination patterns.
//! All types are framework-agnostic — consumers add their own framework
//! derives (e.g. `utoipa::ToSchema`, `utoipa::IntoParams`).

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Offset-based pagination
// ---------------------------------------------------------------------------

/// Offset-based paginated response envelope.
///
/// ```json
/// {"data": [...], "pagination": {"total": 142, "page": 2, "per_page": 20, "total_pages": 8}}
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct PaginatedResponse<T> {
    /// The page of results.
    pub data: Vec<T>,
    /// Pagination metadata.
    pub pagination: Pagination,
}

impl<T> PaginatedResponse<T> {
    /// Create a new paginated response.
    #[must_use]
    pub fn new(data: Vec<T>, pagination: Pagination) -> Self {
        Self { data, pagination }
    }
}

/// Offset-based pagination metadata.
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

// ---------------------------------------------------------------------------
// Pagination query parameters
// ---------------------------------------------------------------------------

fn default_page() -> i64 {
    1
}

fn default_per_page() -> i64 {
    20
}

/// Query parameters for offset-based pagination.
///
/// `per_page` defaults to 20 via serde default functions; consumers can
/// override by providing their own `#[serde(default = "…")]` on a wrapper type.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema, utoipa::IntoParams))]
pub struct PaginationParams {
    /// Page number (1-indexed). Defaults to 1.
    #[cfg_attr(feature = "serde", serde(default = "default_page"))]
    pub page: i64,
    /// Items per page. Defaults to 20.
    #[cfg_attr(feature = "serde", serde(default = "default_per_page"))]
    pub per_page: i64,
}

impl Default for PaginationParams {
    fn default() -> Self {
        Self {
            page: default_page(),
            per_page: default_per_page(),
        }
    }
}

impl PaginationParams {
    /// SQL-friendly offset: `(page - 1) * per_page`.
    #[must_use]
    pub fn offset(&self) -> i64 {
        (self.page - 1) * self.per_page
    }
}

/// Query parameters for paginated list endpoints.
///
/// Used as `Query<PaginationQuery>` in Axum handlers with `utoipa::IntoParams`.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::IntoParams, utoipa::ToSchema))]
pub struct PaginationQuery {
    /// Maximum number of items to return.
    pub limit: Option<i64>,
    /// Number of items to skip.
    pub offset: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn paginated_response_new() {
        let resp = PaginatedResponse::new(vec![1, 2, 3], Pagination::new(3, 1, 20));
        assert_eq!(resp.data.len(), 3);
        assert_eq!(resp.pagination.total, 3);
    }

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

    #[test]
    fn pagination_params_defaults() {
        let p = PaginationParams::default();
        assert_eq!(p.page, 1);
        assert_eq!(p.per_page, 20);
    }

    #[test]
    fn pagination_params_offset() {
        let p = PaginationParams {
            page: 3,
            per_page: 25,
        };
        assert_eq!(p.offset(), 50);
    }

    #[test]
    fn pagination_params_offset_first_page() {
        let p = PaginationParams::default();
        assert_eq!(p.offset(), 0);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn paginated_response_serde_round_trip() {
        let resp = PaginatedResponse::new(vec![1, 2, 3], Pagination::new(50, 2, 10));
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["pagination"]["total"], 50);
        assert_eq!(json["pagination"]["total_pages"], 5);
        assert_eq!(json["data"], serde_json::json!([1, 2, 3]));

        let back: PaginatedResponse<i32> = serde_json::from_value(json).unwrap();
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
    fn pagination_params_serde_defaults() {
        let json = serde_json::json!({});
        let p: PaginationParams = serde_json::from_value(json).unwrap();
        assert_eq!(p.page, 1);
        assert_eq!(p.per_page, 20);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn snapshot_offset_paginated_response() {
        let resp = PaginatedResponse::new(vec![1, 2, 3], Pagination::new(50, 2, 10));
        let json = serde_json::to_value(&resp).unwrap();
        let expected = serde_json::json!({
            "data": [1, 2, 3],
            "pagination": {
                "total": 50,
                "page": 2,
                "per_page": 10,
                "total_pages": 5
            }
        });
        assert_eq!(json, expected);
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

    #[cfg(feature = "serde")]
    #[test]
    fn pagination_params_serde_custom() {
        let json = serde_json::json!({"page": 5, "per_page": 50});
        let p: PaginationParams = serde_json::from_value(json).unwrap();
        assert_eq!(p.page, 5);
        assert_eq!(p.per_page, 50);
    }
}
