//! # api-bones
//!
//! Opinionated REST API types: errors (RFC 9457), pagination, health checks, and more.
//!
//! ## `no_std` support
//!
//! This crate is `#![no_std]` when the default `std` feature is disabled.
//!
//! | Features enabled         | Available types                                    |
//! |--------------------------|-----------------------------------------------------|
//! | *(none)*                 | Pure-`core` types: `ErrorCode`, `HealthStatus`, `PaginationParams`, `SortDirection` |
//! | `alloc`                  | All types that use `String`/`Vec`/`Arc`             |
//! | `std` *(default)*        | Full feature set including `HashMap`-backed types   |
//!
//! ```toml
//! # no_std + alloc (WASM, embedded with allocator)
//! api-bones = { version = "...", default-features = false, features = ["alloc"] }
//!
//! # pure no_std (core types only)
//! api-bones = { version = "...", default-features = false }
//! ```
//!
//! ## Core type: [`ApiError`]
//!
//! Every service serializes errors into [RFC 9457](https://www.rfc-editor.org/rfc/rfc9457)
//! Problem Details format:
//!
//! ```json
//! {
//!   "type": "urn:api-bones:error:resource-not-found",
//!   "title": "Resource Not Found",
//!   "status": 404,
//!   "detail": "Booking 123 not found"
//! }
//! ```
//!
//! ```rust
//! use api_bones::{ApiError, ErrorCode};
//!
//! fn find_booking(id: u64) -> Result<(), ApiError> {
//!     Err(ApiError::not_found(format!("booking {id} not found")))
//! }
//! ```
//!
//! ## Feature flags (selection)
//!
//! | Feature    | What it enables                                      |
//! |------------|------------------------------------------------------|
//! | `schemars` | [`schemars::JsonSchema`] derive on all public types  |
//! | `utoipa`   | [`utoipa::ToSchema`] derive on all public types      |
//!
//! Enable `schemars` in your `Cargo.toml`:
//!
//! ```toml
//! api-bones = { version = "1.6", features = ["schemars"] }
//! ```
//!
//! ## Add as dependency
//!
//! ```toml
//! [registries.brefwiz]
//! index = "sparse+https://git.brefwiz.com/api/packages/gsalingu/cargo/"
//!
//! [dependencies]
//! api-bones = { version = "1.6", registry = "brefwiz" }
//! ```

#![cfg_attr(not(feature = "std"), no_std)]

// When `std` is not available but `alloc` is, bring the alloc crate into scope.
// Under `std`, the `alloc` crate is re-exported by `std` so no explicit import
// is needed.
#[cfg(all(not(feature = "std"), feature = "alloc"))]
extern crate alloc;

// Modules that require heap allocation (String / Vec / Arc).
#[cfg(any(feature = "std", feature = "alloc"))]
pub mod audit;
#[cfg(any(feature = "std", feature = "alloc"))]
pub mod bulk;
#[cfg(any(feature = "std", feature = "alloc"))]
pub mod etag;
#[cfg(any(feature = "std", feature = "alloc"))]
pub mod links;
#[cfg(any(feature = "std", feature = "alloc"))]
pub mod models;
#[cfg(any(feature = "std", feature = "alloc"))]
pub mod response;
#[cfg(any(feature = "std", feature = "alloc"))]
pub mod slug;

// Modules available in all configurations; individual types inside are gated
// where they require `alloc` or `std`.
pub mod common;
pub mod error;
pub mod health;
pub mod pagination;
pub mod query;
pub mod ratelimit;

#[cfg(feature = "fake")]
mod fake_impls;

#[cfg(feature = "icalendar")]
pub mod calendar;

#[cfg(any(feature = "std", feature = "alloc"))]
pub use audit::AuditInfo;
#[cfg(any(feature = "std", feature = "alloc"))]
pub use bulk::{BulkItemResult, BulkRequest, BulkResponse};
#[cfg(feature = "uuid")]
pub use common::ResourceId;
#[cfg(feature = "uuid")]
pub use common::new_resource_id;
#[cfg(feature = "chrono")]
pub use common::parse_timestamp;
// Timestamp is chrono::DateTime when chrono is on (no alloc needed),
// or String when chrono is off (needs alloc or std).
#[cfg(any(feature = "chrono", feature = "std", feature = "alloc"))]
pub use common::Timestamp;
pub use error::ErrorCode;
#[cfg(any(feature = "std", feature = "alloc"))]
pub use error::ErrorTypeMode;
#[cfg(all(any(feature = "std", feature = "alloc"), feature = "serde"))]
pub use error::ProblemJson;
#[cfg(any(feature = "std", feature = "alloc"))]
pub use error::{ApiError, ValidationError};
#[cfg(feature = "std")]
pub use error::{error_type_mode, set_error_type_mode, urn_namespace};
#[cfg(any(feature = "std", feature = "alloc"))]
pub use etag::{ETag, IfMatch, IfNoneMatch};
pub use health::HealthStatus;
#[cfg(feature = "std")]
pub use health::ReadinessResponse;
#[cfg(any(feature = "std", feature = "alloc"))]
pub use health::{HealthCheck, LivenessResponse};
#[cfg(any(feature = "std", feature = "alloc"))]
pub use links::{Link, Links};
#[cfg(any(feature = "std", feature = "alloc"))]
pub use models::ErrorResponse;
pub use pagination::PaginationParams;
#[cfg(any(feature = "std", feature = "alloc"))]
pub use pagination::{
    CursorPaginatedResponse, CursorPagination, CursorPaginationParams, PaginatedResponse,
};
pub use query::SortDirection;
#[cfg(any(feature = "std", feature = "alloc"))]
pub use query::{FilterEntry, FilterParams, SearchParams, SortParams};
#[cfg(any(feature = "std", feature = "alloc"))]
pub use ratelimit::RateLimitInfo;
#[cfg(any(feature = "std", feature = "alloc"))]
pub use response::{ApiResponse, ApiResponseBuilder, ResponseMeta};
#[cfg(any(feature = "std", feature = "alloc"))]
pub use slug::{Slug, SlugError};
