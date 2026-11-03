// SPDX-License-Identifier: MIT
//! The SDK page iterator for keyset-paginated List RPCs.
//!
//! A List RPC carries a `bones.v1.PageRequest` and answers with one repeated field
//! plus a `bones.v1.PageResponse`. [`ListCall`] turns it into a stream:
//!
//! - it follows `next_cursor` until it is empty;
//! - it fetches at most one page ahead ([`PageOptions::prefetch`]) and aborts
//!   that fetch when the stream is dropped;
//! - it retries a page fetch only when the RPC is declared free of side
//!   effects or idempotent, and only on a failure that is safe to repeat;
//! - a failure after items were delivered ends the stream with one
//!   [`PageError`] that says how much was delivered.
//!
//! The crate does not know any generated message type: a response type opts in
//! with [`PagedResponse`] and a request type with [`PagedRequest`].

mod cache;
mod call;
mod error;
mod prefetch;
mod span;

/// Public `ErrorInfo` reason strings documented in `bones/v1/pagination.proto`.
pub mod reason {
    /// Tampered cursor or wrong service, method, organization or principal scope.
    pub const CURSOR_INVALID: &str = "CURSOR_INVALID";
    /// Expired authentic cursor.
    pub const CURSOR_EXPIRED: &str = "CURSOR_EXPIRED";
    /// Authentic, correctly scoped cursor whose query hash mismatches the request.
    pub const CURSOR_QUERY_MISMATCH: &str = "CURSOR_QUERY_MISMATCH";
}

pub use cache::RevisionCache;
pub use call::{
    CacheMode, Fetch, ListCall, ListMetadata, ListResponse, MetadataFetch, Page, PageInfo,
    PageOptions, PagedRequest, PagedResponse, Total, TotalKind,
};
pub use error::{PageError, PageErrorKind};
