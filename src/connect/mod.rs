//! Proto adapter primitives for Connect RPC services (ADR-0096).
//!
//! Enable with `features = ["connect"]`:
//!
//! ```toml
//! api-bones = { version = "6", features = ["connect"] }
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! use api_bones::connect::{
//!     chrono_to_timestamp, chrono_opt_to_timestamp,
//!     parse_uuid,
//!     build_page, build_offset_page, OffsetPage,
//!     ConnectOptionExt as _,
//! };
//!
//! // Timestamp conversion
//! let ts = chrono_to_timestamp(record.created_at);
//!
//! // UUID parsing with field attribution in the error message
//! let org_id = parse_uuid(request.org_id, "org_id")?;
//!
//! // Pagination — OffsetPage has a private constructor; build_page is the only entry point
//! let page = build_page(req.page.limit, req.page.offset, items.len(), total);
//! let (total_count, has_more, limit, offset) = page.into_parts();
//!
//! // Option → not_found
//! let record = store.get(id).await.map_err(core_to_connect)?.or_not_found("not found")?;
//! ```
//!
//! # Enforcement (ADR-0096)
//!
//! Two primitives carry compile-time enforcement:
//!
//! - [`OffsetPage`] has a private constructor — only [`build_page`] /
//!   [`build_offset_page`] can produce one. Call sites expecting `OffsetPage`
//!   cannot be satisfied by hand-rolled construction.
//!
//! - [`ConnectOptionExt`] is sealed — reimplementing the trait in service code
//!   produces a different trait that will not satisfy imports of this path.
//!
//! A CI grep gate (`connect-bones-check`) additionally bans raw
//! `Uuid::parse_str(` and `Timestamp::from_unix(` calls in adapter modules.

mod ext;
mod page;
mod timestamp;
mod uuid;

pub use ext::ConnectOptionExt;
pub use page::{DEFAULT_LIMIT, MAX_LIMIT, OffsetPage, build_offset_page, build_page};
pub use timestamp::{chrono_opt_to_timestamp, chrono_to_timestamp};
pub use uuid::parse_uuid;
