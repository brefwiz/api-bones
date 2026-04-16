//! # shared-types
//!
//! Shared public API types for the Brefwiz service ecosystem.
//!
//! ## Core type: [`ApiError`]
//!
//! Every service serializes errors into [RFC 9457](https://www.rfc-editor.org/rfc/rfc9457)
//! Problem Details format:
//!
//! ```json
//! {
//!   "type": "urn:brefwiz:error:resource-not-found",
//!   "title": "Resource Not Found",
//!   "status": 404,
//!   "detail": "Booking 123 not found"
//! }
//! ```
//!
//! ```rust
//! use shared_types::{ApiError, ErrorCode};
//!
//! fn find_booking(id: u64) -> Result<(), ApiError> {
//!     Err(ApiError::not_found(format!("booking {id} not found")))
//! }
//! ```
//!
//! ## Add as dependency
//!
//! ```toml
//! [registries.gitea]
//! index = "sparse+https://brefwiz.mentalmorph.com/api/packages/Brefwiz/cargo/"
//!
//! [dependencies]
//! shared-types = { version = "0.2", registry = "gitea" }
//! ```

#[cfg(feature = "icalendar")]
pub mod calendar;

pub mod common;
pub mod error;
pub mod health;
pub mod links;
pub mod models;
pub mod pagination;
pub mod query;
pub mod ratelimit;
pub mod response;

#[cfg(feature = "uuid")]
pub use common::new_resource_id;
#[cfg(feature = "chrono")]
pub use common::parse_timestamp;
pub use common::{ResourceId, Timestamp};
pub use error::{
    ApiError, ErrorCode, ErrorTypeMode, ValidationError, error_type_mode, set_error_type_mode,
    urn_namespace,
};
pub use health::{HealthCheck, HealthStatus, LivenessResponse, ReadinessResponse};
pub use links::{Link, Links};
pub use models::ErrorResponse;
pub use pagination::{
    CursorPaginatedResponse, CursorPagination, PaginatedResponse, PaginationParams,
};
pub use query::{FilterEntry, FilterParams, SearchParams, SortDirection, SortParams};
pub use ratelimit::RateLimitInfo;
pub use response::{ApiResponse, ApiResponseBuilder, ResponseMeta};
