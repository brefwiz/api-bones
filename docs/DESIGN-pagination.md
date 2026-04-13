# Design: Pagination Types for shared-types

**Issue:** my-service#552  
**ADR:** PLATFORM-006  
**Author:** ArchMind  
**Date:** 2026-03-02  
**Status:** Draft — awaiting review before implementation

---

## Problem

All Brefwiz list endpoints currently return bare `Vec<T>`. There's no pagination, no total counts, no cursor support. As datasets grow this becomes untenable. We need a standard paginated envelope that:

1. Works for offset-based pagination (admin dashboards, tables)
2. Works for cursor-based pagination (infinite scroll, event streams)
3. Lives in `shared-types` so every service uses the same wire format
4. Stays framework-agnostic in the core, with an optional `axum` feature for extractor support

## Wire Format (target)

### Offset-based response
```json
{
  "data": [{"id": "...", "name": "..."}],
  "pagination": {
    "total": 142,
    "page": 2,
    "per_page": 20,
    "total_pages": 8
  }
}
```

### Cursor-based response
```json
{
  "data": [{"id": "...", "name": "..."}],
  "pagination": {
    "has_more": true,
    "next_cursor": "eyJpZCI6NDJ9"
  }
}
```

---

## New Feature Gates

Add to `Cargo.toml`:

```toml
[features]
default = ["serde", "uuid", "chrono"]
serde = ["dep:serde", "dep:serde_json", "dep:serde_with", "uuid?/serde"]
uuid = ["dep:uuid"]
chrono = ["dep:chrono"]
axum = ["dep:axum", "serde"]          # NEW — optional Axum integration

[dependencies]
# ... existing ...
axum = { version = "0.8", optional = true, default-features = false, features = ["query"] }
```

Services that use Axum (all of them today) enable `shared-types = { features = ["axum"] }`. The core types remain usable without Axum.

---

## Type Definitions

### File: `src/pagination.rs` (new module)

```rust
//! Paginated response envelopes and query parameter extractors.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Pagination metadata
// ---------------------------------------------------------------------------

/// Offset-based pagination metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct OffsetPagination {
    /// Total number of items across all pages.
    pub total: u64,
    /// Current page number (1-indexed).
    pub page: u32,
    /// Items per page.
    pub per_page: u32,
    /// Total number of pages.
    pub total_pages: u32,
}

/// Cursor-based pagination metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CursorPagination {
    /// Whether more items exist after this page.
    pub has_more: bool,
    /// Opaque cursor for the next page. `None` when `has_more` is false.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub next_cursor: Option<String>,
}

/// Pagination metadata — either offset-based or cursor-based.
///
/// Serializes *untagged* so the JSON is flat under `"pagination"`:
/// - Offset variant: `{"total": N, "page": N, "per_page": N, "total_pages": N}`
/// - Cursor variant: `{"has_more": bool, "next_cursor": "..."}`
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(untagged))]
pub enum PaginationMeta {
    Offset(OffsetPagination),
    Cursor(CursorPagination),
}

// ---------------------------------------------------------------------------
// Paginated response envelope
// ---------------------------------------------------------------------------

/// Generic paginated response envelope.
///
/// ```json
/// {"data": [...], "pagination": {"total": 142, "page": 2, ...}}
/// ```
///
/// `T` is the item type. The response serializes as a flat object with
/// `"data"` and `"pagination"` keys — no outer `"response"` wrapper.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct PaginatedResponse<T> {
    /// The page of items.
    pub data: Vec<T>,
    /// Pagination metadata (offset or cursor).
    pub pagination: PaginationMeta,
}

impl<T> PaginatedResponse<T> {
    /// Create an offset-paginated response.
    ///
    /// `total_pages` is computed automatically as `ceil(total / per_page)`.
    pub fn offset(data: Vec<T>, total: u64, page: u32, per_page: u32) -> Self {
        let total_pages = if per_page == 0 {
            0
        } else {
            // integer ceil division
            ((total + u64::from(per_page) - 1) / u64::from(per_page)) as u32
        };
        Self {
            data,
            pagination: PaginationMeta::Offset(OffsetPagination {
                total,
                page,
                per_page,
                total_pages,
            }),
        }
    }

    /// Create a cursor-paginated response.
    pub fn cursor(data: Vec<T>, has_more: bool, next_cursor: Option<String>) -> Self {
        Self {
            data,
            pagination: PaginationMeta::Cursor(CursorPagination {
                has_more,
                next_cursor,
            }),
        }
    }

    /// Map the items in this response to a different type.
    pub fn map<U>(self, f: impl FnMut(T) -> U) -> PaginatedResponse<U> {
        PaginatedResponse {
            data: self.data.into_iter().map(f).collect(),
            pagination: self.pagination,
        }
    }
}

// ---------------------------------------------------------------------------
// Query parameter extractors
// ---------------------------------------------------------------------------

/// Default items per page.
const DEFAULT_PER_PAGE: u32 = 20;
/// Maximum items per page (prevents abuse).
const MAX_PER_PAGE: u32 = 100;
/// Default page number.
const DEFAULT_PAGE: u32 = 1;

/// Offset-based pagination query parameters.
///
/// Extracts `?page=2&per_page=20` from the query string.
/// Missing values get sensible defaults.
///
/// ```
/// use shared_types::OffsetParams;
///
/// let params = OffsetParams::default();
/// assert_eq!(params.page(), 1);
/// assert_eq!(params.per_page(), 20);
/// assert_eq!(params.offset(), 0);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct OffsetParams {
    /// Page number (1-indexed). Defaults to 1.
    #[cfg_attr(feature = "serde", serde(default = "default_page"))]
    page: u32,
    /// Items per page. Defaults to 20, clamped to [1, 100].
    #[cfg_attr(feature = "serde", serde(default = "default_per_page"))]
    per_page: u32,
}

#[cfg(feature = "serde")]
const fn default_page() -> u32 { DEFAULT_PAGE }
#[cfg(feature = "serde")]
const fn default_per_page() -> u32 { DEFAULT_PER_PAGE }

impl Default for OffsetParams {
    fn default() -> Self {
        Self {
            page: DEFAULT_PAGE,
            per_page: DEFAULT_PER_PAGE,
        }
    }
}

impl OffsetParams {
    /// Create with explicit page and per_page (clamped).
    #[must_use]
    pub fn new(page: u32, per_page: u32) -> Self {
        Self {
            page: page.max(1),
            per_page: per_page.clamp(1, MAX_PER_PAGE),
        }
    }

    /// Current page (1-indexed, minimum 1).
    #[must_use]
    pub fn page(&self) -> u32 {
        self.page.max(1)
    }

    /// Items per page (clamped to [1, 100]).
    #[must_use]
    pub fn per_page(&self) -> u32 {
        self.per_page.clamp(1, MAX_PER_PAGE)
    }

    /// SQL-style offset: `(page - 1) * per_page`.
    #[must_use]
    pub fn offset(&self) -> u64 {
        u64::from(self.page().saturating_sub(1)) * u64::from(self.per_page())
    }

    /// SQL-style limit (same as `per_page()`).
    #[must_use]
    pub fn limit(&self) -> u64 {
        u64::from(self.per_page())
    }
}

/// Cursor-based pagination query parameters.
///
/// Extracts `?cursor=eyJpZCI6NDJ9&limit=20` from the query string.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CursorParams {
    /// Opaque cursor from a previous response. `None` for the first page.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub cursor: Option<String>,
    /// Items to return. Defaults to 20, clamped to [1, 100].
    #[cfg_attr(feature = "serde", serde(default = "default_per_page"))]
    limit: u32,
}

impl Default for CursorParams {
    fn default() -> Self {
        Self {
            cursor: None,
            limit: DEFAULT_PER_PAGE,
        }
    }
}

impl CursorParams {
    /// Create with explicit cursor and limit.
    #[must_use]
    pub fn new(cursor: Option<String>, limit: u32) -> Self {
        Self {
            cursor,
            limit: limit.clamp(1, MAX_PER_PAGE),
        }
    }

    /// Items to return (clamped to [1, 100]).
    #[must_use]
    pub fn limit(&self) -> u32 {
        self.limit.clamp(1, MAX_PER_PAGE)
    }
}

/// Unified pagination query parameters.
///
/// If `cursor` is present, cursor-based mode is used.
/// Otherwise, falls back to offset-based with `page`/`per_page`.
///
/// This allows a single query extractor for endpoints that support both modes.
///
/// ```
/// // Offset: GET /api/v1/users?page=2&per_page=10
/// // Cursor: GET /api/v1/events?cursor=eyJpZCI6NDJ9&limit=50
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct PaginationParams {
    #[cfg_attr(feature = "serde", serde(default, skip_serializing_if = "Option::is_none"))]
    pub cursor: Option<String>,
    #[cfg_attr(feature = "serde", serde(default = "default_page"))]
    pub page: u32,
    #[cfg_attr(feature = "serde", serde(default = "default_per_page"))]
    pub per_page: u32,
    /// Alias for `per_page` in cursor mode.
    #[cfg_attr(feature = "serde", serde(default, skip_serializing_if = "Option::is_none"))]
    pub limit: Option<u32>,
}

impl Default for PaginationParams {
    fn default() -> Self {
        Self {
            cursor: None,
            page: DEFAULT_PAGE,
            per_page: DEFAULT_PER_PAGE,
            limit: None,
        }
    }
}

impl PaginationParams {
    /// Returns `true` if cursor-based pagination was requested.
    #[must_use]
    pub fn is_cursor(&self) -> bool {
        self.cursor.is_some()
    }

    /// Convert to offset params (ignoring cursor).
    #[must_use]
    pub fn as_offset(&self) -> OffsetParams {
        OffsetParams::new(self.page, self.per_page)
    }

    /// Convert to cursor params.
    #[must_use]
    pub fn as_cursor(&self) -> CursorParams {
        CursorParams::new(
            self.cursor.clone(),
            self.limit.unwrap_or(self.per_page),
        )
    }
}
```

### Axum Integration (`src/pagination.rs`, gated)

```rust
// ---------------------------------------------------------------------------
// Axum extractor support (feature = "axum")
// ---------------------------------------------------------------------------

#[cfg(feature = "axum")]
mod axum_support {
    use super::*;

    // axum::extract::Query<T> already implements FromRequestParts
    // for any T: DeserializeOwned. Since OffsetParams, CursorParams,
    // and PaginationParams all derive Deserialize, they work out of
    // the box as:
    //
    //   async fn list_users(
    //       Query(params): Query<OffsetParams>,
    //   ) -> impl IntoResponse { ... }
    //
    // No custom FromRequestParts impl needed.

    // IntoResponse for PaginatedResponse<T> so handlers can return it directly.

    impl<T: serde::Serialize> axum::response::IntoResponse for PaginatedResponse<T> {
        fn into_response(self) -> axum::response::Response {
            axum::Json(self).into_response()
        }
    }
}
```

### Module Registration (`src/lib.rs`)

```rust
pub mod pagination;

pub use pagination::{
    CursorPagination, CursorParams, OffsetPagination, OffsetParams,
    PaginatedResponse, PaginationMeta, PaginationParams,
};
```

---

## Design Decisions

### 1. Untagged enum for `PaginationMeta`

`#[serde(untagged)]` gives us clean JSON without a `"type"` discriminator. The two variants have disjoint field sets (`total`/`page` vs `has_more`/`next_cursor`), so deserialization is unambiguous.

### 2. Separate `OffsetParams` / `CursorParams` + unified `PaginationParams`

Most endpoints will use one pagination style exclusively:
- **CRUD list endpoints** → `OffsetParams` (tables with page numbers)
- **Event/audit streams** → `CursorParams` (forward-only traversal)

The unified `PaginationParams` exists for rare endpoints that support both, and for future-proofing. Endpoints pick the extractor that fits.

### 3. Framework-agnostic core, optional `axum` feature

The params types derive `Deserialize`, so `axum::extract::Query<OffsetParams>` works automatically — no custom `FromRequestParts` needed. The `axum` feature only adds `IntoResponse` for `PaginatedResponse<T>`, enabling handlers to return it directly.

### 4. Clamped values with accessors

Raw fields + accessor methods that clamp (`per_page` to [1, 100], `page` minimum 1). This prevents accidental `per_page=999999` abuse at the type level, not the handler level.

### 5. `map()` on `PaginatedResponse`

Allows converting domain types to response DTOs without reconstructing the pagination metadata:
```rust
let db_result: PaginatedResponse<UserRow> = repo.list_users(params).await?;
let response = db_result.map(UserResponse::from);
```

### 6. Constants are private, not configurable

`DEFAULT_PER_PAGE` (20) and `MAX_PER_PAGE` (100) are hardcoded. If a service needs different limits, it can clamp after extraction. Keeping these as shared defaults enforces consistency across the platform.

---

## Trait Implementations Required

| Type | Trait | Notes |
|---|---|---|
| `PaginatedResponse<T>` | `Serialize` (when `T: Serialize`) | `#[cfg_attr(feature = "serde", ...)]` |
| `PaginatedResponse<T>` | `Deserialize` (when `T: Deserialize`) | For SDK/client usage |
| `PaginatedResponse<T>` | `axum::response::IntoResponse` (when `T: Serialize`) | `#[cfg(feature = "axum")]` |
| `OffsetParams`, `CursorParams`, `PaginationParams` | `Serialize + Deserialize` | Query string extraction |
| `OffsetPagination`, `CursorPagination` | `Serialize + Deserialize` | Part of response |
| `PaginationMeta` | `Serialize + Deserialize` (untagged) | Enum dispatch |
| All types | `Debug, Clone, PartialEq, Eq` | Match existing conventions (`Eq` where possible) |

Note: `PaginatedResponse<T>` gets `PartialEq` only (not `Eq`) to match `ApiError` convention — `T` may not be `Eq`.

---

## Example Usage

### Handler returning offset-paginated users

```rust
use axum::extract::{Query, State};
use shared_types::{OffsetParams, PaginatedResponse};

async fn list_users(
    State(state): State<AppState>,
    Query(params): Query<OffsetParams>,
) -> Result<PaginatedResponse<UserResponse>, ApiErrorResponse> {
    let (users, total) = UserService::list_paginated(
        &state.db,
        params.offset(),
        params.limit(),
    ).await?;

    Ok(PaginatedResponse::offset(
        users.into_iter().map(UserResponse::from).collect(),
        total,
        params.page(),
        params.per_page(),
    ))
}
```

### Handler returning cursor-paginated events

```rust
use shared_types::{CursorParams, PaginatedResponse};

async fn list_events(
    State(state): State<AppState>,
    Query(params): Query<CursorParams>,
) -> Result<PaginatedResponse<EventResponse>, ApiErrorResponse> {
    let (events, next_cursor) = EventService::list_after_cursor(
        &state.db,
        params.cursor.as_deref(),
        params.limit(),
    ).await?;

    let has_more = next_cursor.is_some();
    Ok(PaginatedResponse::cursor(events, has_more, next_cursor))
}
```

### Using `map()` for DTO conversion

```rust
let page: PaginatedResponse<OrgRow> = org_repo.list(params).await?;
let response: PaginatedResponse<OrganizationResponse> = page.map(Into::into);
```

---

## Migration Path for my-service

Current list endpoints return `Json<Vec<T>>`. Migration per endpoint:

1. Add `Query(params): Query<OffsetParams>` parameter
2. Change service layer to accept `offset`/`limit` and return `(Vec<T>, u64)` (items + total count)
3. Return `PaginatedResponse::offset(...)` instead of `Json(vec)`
4. Update OpenAPI annotations (`body = PaginatedResponse<T>`)

This is backward-**incompatible** (response shape changes from `[...]` to `{"data": [...], "pagination": {...}}`). Coordinate with frontend/SDK consumers. Consider versioning or a migration period if needed.

---

## Test Plan

1. **Serde round-trip** for offset and cursor variants
2. **Wire format** assertions (exact JSON shape, field names)
3. **OffsetParams clamping**: `per_page=0` → 1, `per_page=999` → 100, `page=0` → 1
4. **OffsetParams::offset()**: page 1 → 0, page 3 with per_page 20 → 40
5. **Untagged deserialization**: offset JSON → `PaginationMeta::Offset`, cursor JSON → `PaginationMeta::Cursor`
6. **`map()`**: verify pagination metadata preserved after mapping
7. **PaginatedResponse::offset()** `total_pages` calculation: edge cases (0 items, 1 item, exact multiple)
8. **Feature gate compilation**: build with `--no-default-features`, with `serde` only, with `axum`

---

## Open Questions

1. **Should `IntoResponse` set any pagination headers?** (e.g., `X-Total-Count`, `Link` headers for RFC 8288). Current design: no — keep it simple, metadata is in the body. Revisit if needed.
2. **Should `PaginatedResponse` implement `utoipa::ToSchema`?** The my-service uses utoipa for OpenAPI. This may need a `utoipa` feature gate or manual schema impl since generic types are tricky with utoipa's derive macro. Defer to implementation phase.
3. **Cursor encoding convention?** The spec doesn't prescribe cursor format — that's intentionally left to each service (base64-encoded JSON, opaque UUID, etc.). The type is just `String`.
