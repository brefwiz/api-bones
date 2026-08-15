// SPDX-License-Identifier: MIT
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
//!     parse_uuid, parse_rfc3339,
//!     build_page, build_offset_page, OffsetPage,
//!     ConnectOptionExt as _,
//!     invalid_field, encode_json,
//!     check_if_match, etag_from_updated_at,
//! };
//!
//! // Timestamp conversion
//! let ts = chrono_to_timestamp(record.created_at);
//!
//! // UUID parsing with field attribution in the error message
//! let org_id = parse_uuid(request.org_id, "org_id")?;
//!
//! // RFC-3339 datetime parsing
//! let starts_at = parse_rfc3339(request.starts_at, "starts_at")?;
//!
//! // Pagination — OffsetPage has a private constructor; build_page is the only entry point
//! let page = build_page(req.page.limit, req.page.offset, items.len(), total);
//! let (total_count, has_more, limit, offset) = page.into_parts();
//!
//! // Option → not_found
//! let record = store.get(id).await.map_err(core_to_connect)?.or_not_found("not found")?;
//!
//! // Build InvalidArgument for a named field
//! let err = invalid_field("quantity_rule");
//!
//! // Serialize a domain type to a JSON string for an opaque proto field
//! let json = encode_json(&quantity_rule, "quantity_rule")?;
//!
//! // ETag / If-Match — generate and enforce optimistic-concurrency headers
//! let etag = etag_from_updated_at(record.updated_at);
//! check_if_match(&ctx, &etag)?; // FailedPrecondition if header absent, Aborted on mismatch
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

mod domain_error;
mod etag;
mod ext;
mod page;
mod parse;
mod scoped;
mod timestamp;
pub mod transport;
mod uuid;

pub use domain_error::{DomainErrorKind, IntoDomainErrorKind, domain_to_connect};
pub use etag::{check_if_match, etag_from_updated_at};
pub use ext::ConnectOptionExt;
pub use page::{DEFAULT_LIMIT, MAX_LIMIT, OffsetPage, build_offset_page, build_page};
pub use parse::parse_rfc3339;
pub use scoped::{CallCredential, ScopeCall, ScopedClient, TokenInjector};
pub use timestamp::{chrono_opt_to_timestamp, chrono_to_timestamp};
pub use transport::{
    ConnectConfigExt, client_tls_config, connect_http_client, is_caller_rejection,
};
pub use uuid::parse_uuid;

/// Build an `InvalidArgument` `ConnectError` attributing the error to a named request field.
#[must_use]
pub fn invalid_field(field: &str) -> connectrpc::ConnectError {
    connectrpc::ConnectError::new(
        connectrpc::ErrorCode::InvalidArgument,
        format!("invalid `{field}`"),
    )
}

/// Serialize a value to a JSON string for use in opaque proto string fields.
/// Returns `Internal` if serialization fails (rare — only on non-serializable types).
pub fn encode_json<T: serde::Serialize>(
    v: &T,
    what: &str,
) -> Result<String, connectrpc::ConnectError> {
    serde_json::to_string(v).map_err(|e| {
        connectrpc::ConnectError::new(
            connectrpc::ErrorCode::Internal,
            format!("serialize {what}: {e}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use connectrpc::ErrorCode;

    #[test]
    fn invalid_field_code_and_message() {
        let err = invalid_field("org_id");
        assert_eq!(err.code, ErrorCode::InvalidArgument);
        let msg = err.message.as_deref().unwrap_or("");
        assert!(msg.contains("org_id"), "message: {msg}");
    }

    #[test]
    fn encode_json_simple_struct() {
        #[derive(serde::Serialize)]
        struct Foo {
            bar: u32,
        }
        let result = encode_json(&Foo { bar: 42 }, "foo").unwrap();
        assert_eq!(result, r#"{"bar":42}"#);
    }
}
