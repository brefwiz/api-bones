//! Generic API response envelope types.
//!
//! [`ApiResponse<T>`] wraps any payload with consistent metadata so all API
//! endpoints share the same top-level shape.
//!
//! ```json
//! {
//!   "data": { "id": "abc", "name": "Foo" },
//!   "meta": {
//!     "request_id": "req-123",
//!     "timestamp": "2026-04-06T19:00:00Z",
//!     "version": "1.4.0"
//!   },
//!   "links": { "self": "/resources/abc" }
//! }
//! ```
//!
//! # Builder example
//!
//! ```rust
//! use shared_types::response::{ApiResponse, ResponseMeta};
//!
//! let response: ApiResponse<&str> = ApiResponse::builder("hello world")
//!     .meta(ResponseMeta::new().request_id("req-001").version("1.0"))
//!     .build();
//!
//! assert_eq!(response.data, "hello world");
//! ```

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::string::String;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::common::Timestamp;

// ---------------------------------------------------------------------------
// ResponseMeta
// ---------------------------------------------------------------------------

/// Metadata attached to every [`ApiResponse`].
///
/// All fields are optional to keep construction ergonomic. Consumers that do
/// not need a field simply omit it; it will be skipped in serialization.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ResponseMeta {
    /// Unique identifier for the originating HTTP request.
    ///
    /// Useful for correlating logs and distributed traces.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub request_id: Option<String>,

    /// Server-side timestamp when the response was generated (RFC 3339).
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    #[cfg_attr(feature = "utoipa", schema(value_type = Option<String>, format = DateTime))]
    pub timestamp: Option<Timestamp>,

    /// API or service version string.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub version: Option<String>,
}

impl ResponseMeta {
    /// Create an empty `ResponseMeta`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the `request_id` field (builder-style).
    #[must_use]
    pub fn request_id(mut self, id: impl Into<String>) -> Self {
        self.request_id = Some(id.into());
        self
    }

    /// Set the `timestamp` field (builder-style).
    #[must_use]
    pub fn timestamp(mut self, ts: Timestamp) -> Self {
        self.timestamp = Some(ts);
        self
    }

    /// Set the `version` field (builder-style).
    #[must_use]
    pub fn version(mut self, v: impl Into<String>) -> Self {
        self.version = Some(v.into());
        self
    }
}

// ---------------------------------------------------------------------------
// arbitrary / proptest impls for ResponseMeta
// ---------------------------------------------------------------------------

// ResponseMeta contains `Option<Timestamp>` where Timestamp is chrono::DateTime<Utc>
// (when the `chrono` feature is enabled).  Since chrono does not implement
// arbitrary::Arbitrary or proptest::arbitrary::Arbitrary, we provide hand-rolled
// impls for both feature combinations.

#[cfg(all(feature = "arbitrary", not(feature = "chrono")))]
impl<'a> arbitrary::Arbitrary<'a> for ResponseMeta {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        use arbitrary::Arbitrary;
        Ok(Self {
            request_id: Arbitrary::arbitrary(u)?,
            timestamp: Arbitrary::arbitrary(u)?,
            version: Arbitrary::arbitrary(u)?,
        })
    }
}

#[cfg(all(feature = "arbitrary", feature = "chrono"))]
impl<'a> arbitrary::Arbitrary<'a> for ResponseMeta {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        use arbitrary::Arbitrary;
        use chrono::TimeZone as _;
        let timestamp = if bool::arbitrary(u)? {
            // Clamp to a valid Unix timestamp range (year 1970–3000)
            let secs = u.int_in_range(0i64..=32_503_680_000i64)?;
            chrono::Utc.timestamp_opt(secs, 0).single()
        } else {
            None
        };
        Ok(Self {
            request_id: Arbitrary::arbitrary(u)?,
            timestamp,
            version: Arbitrary::arbitrary(u)?,
        })
    }
}

#[cfg(all(feature = "proptest", not(feature = "chrono")))]
impl proptest::arbitrary::Arbitrary for ResponseMeta {
    type Parameters = ();
    type Strategy = proptest::strategy::BoxedStrategy<Self>;

    fn arbitrary_with((): ()) -> Self::Strategy {
        use proptest::prelude::*;
        (
            proptest::option::of(any::<String>()),
            proptest::option::of(any::<String>()),
            proptest::option::of(any::<String>()),
        )
            .prop_map(|(request_id, timestamp, version)| Self {
                request_id,
                timestamp,
                version,
            })
            .boxed()
    }
}

#[cfg(all(feature = "proptest", feature = "chrono"))]
impl proptest::arbitrary::Arbitrary for ResponseMeta {
    type Parameters = ();
    type Strategy = proptest::strategy::BoxedStrategy<Self>;

    fn arbitrary_with((): ()) -> Self::Strategy {
        use chrono::TimeZone as _;
        use proptest::prelude::*;
        (
            proptest::option::of(any::<String>()),
            proptest::option::of(0i64..=32_503_680_000i64),
            proptest::option::of(any::<String>()),
        )
            .prop_map(|(request_id, ts_secs, version)| Self {
                request_id,
                timestamp: ts_secs.and_then(|s| chrono::Utc.timestamp_opt(s, 0).single()),
                version,
            })
            .boxed()
    }
}

// ---------------------------------------------------------------------------
// Links
// ---------------------------------------------------------------------------

/// Hypermedia links included in an [`ApiResponse`].
///
/// Inspired by the [JSON:API `links` object](https://jsonapi.org/format/#document-links).
/// All fields are optional — include only the links that are meaningful for
/// the specific resource.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "proptest", derive(proptest_derive::Arbitrary))]
pub struct Links {
    /// The canonical URL for this resource.
    #[cfg_attr(
        feature = "serde",
        serde(rename = "self", default, skip_serializing_if = "Option::is_none")
    )]
    pub self_link: Option<String>,

    /// URL to the next page (for paginated responses).
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub next: Option<String>,

    /// URL to the previous page (for paginated responses).
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub prev: Option<String>,
}

impl Links {
    /// Create an empty `Links`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the `self` link (builder-style).
    #[must_use]
    pub fn self_link(mut self, url: impl Into<String>) -> Self {
        self.self_link = Some(url.into());
        self
    }

    /// Set the `next` link (builder-style).
    #[must_use]
    pub fn next(mut self, url: impl Into<String>) -> Self {
        self.next = Some(url.into());
        self
    }

    /// Set the `prev` link (builder-style).
    #[must_use]
    pub fn prev(mut self, url: impl Into<String>) -> Self {
        self.prev = Some(url.into());
        self
    }
}

// ---------------------------------------------------------------------------
// ApiResponse
// ---------------------------------------------------------------------------

/// Generic API response envelope.
///
/// Wraps any payload `T` with consistent metadata so all endpoints share the
/// same top-level JSON shape.  Use [`ApiResponse::builder`] for ergonomic
/// construction.
///
/// # Composing with `PaginatedResponse`
///
/// ```rust
/// use shared_types::pagination::{PaginatedResponse, PaginationParams};
/// use shared_types::response::ApiResponse;
///
/// let params = PaginationParams::default();
/// let page = PaginatedResponse::new(vec![1i32, 2, 3], 10, &params);
/// let response = ApiResponse::builder(page).build();
/// assert_eq!(response.data.total_count, 10);
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "proptest", derive(proptest_derive::Arbitrary))]
pub struct ApiResponse<T> {
    /// The primary payload.
    pub data: T,

    /// Request-level metadata.
    pub meta: ResponseMeta,

    /// Optional hypermedia links.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub links: Option<Links>,
}

// ---------------------------------------------------------------------------
// ApiResponseBuilder
// ---------------------------------------------------------------------------

/// Builder for [`ApiResponse`].
///
/// Obtain one via [`ApiResponse::builder`].
pub struct ApiResponseBuilder<T> {
    data: T,
    meta: ResponseMeta,
    links: Option<Links>,
}

impl<T> ApiResponseBuilder<T> {
    /// Set the `meta` field.
    #[must_use]
    pub fn meta(mut self, meta: ResponseMeta) -> Self {
        self.meta = meta;
        self
    }

    /// Set the `links` field.
    #[must_use]
    pub fn links(mut self, links: Links) -> Self {
        self.links = Some(links);
        self
    }

    /// Consume the builder and produce an [`ApiResponse`].
    #[must_use]
    pub fn build(self) -> ApiResponse<T> {
        ApiResponse {
            data: self.data,
            meta: self.meta,
            links: self.links,
        }
    }
}

impl<T> ApiResponse<T> {
    /// Begin building an [`ApiResponse`] with the given `data` payload.
    #[must_use]
    pub fn builder(data: T) -> ApiResponseBuilder<T> {
        ApiResponseBuilder {
            data,
            meta: ResponseMeta::default(),
            links: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // ResponseMeta construction
    // -----------------------------------------------------------------------

    #[test]
    fn response_meta_new_is_empty() {
        let m = ResponseMeta::new();
        assert!(m.request_id.is_none());
        assert!(m.timestamp.is_none());
        assert!(m.version.is_none());
    }

    #[test]
    fn response_meta_builder_chain() {
        let m = ResponseMeta::new().request_id("req-001").version("1.4.0");
        assert_eq!(m.request_id.as_deref(), Some("req-001"));
        assert_eq!(m.version.as_deref(), Some("1.4.0"));
    }

    #[cfg(feature = "chrono")]
    #[test]
    fn response_meta_timestamp_builder() {
        use chrono::Utc;
        let ts = Utc::now();
        let m = ResponseMeta::new().timestamp(ts);
        assert!(m.timestamp.is_some());
    }

    // -----------------------------------------------------------------------
    // Links construction
    // -----------------------------------------------------------------------

    #[test]
    fn links_new_is_empty() {
        let l = Links::new();
        assert!(l.self_link.is_none());
        assert!(l.next.is_none());
        assert!(l.prev.is_none());
    }

    #[test]
    fn links_builder_chain() {
        let l = Links::new()
            .self_link("/resources/1")
            .next("/resources?page=2")
            .prev("/resources?page=0");
        assert_eq!(l.self_link.as_deref(), Some("/resources/1"));
        assert_eq!(l.next.as_deref(), Some("/resources?page=2"));
        assert_eq!(l.prev.as_deref(), Some("/resources?page=0"));
    }

    // -----------------------------------------------------------------------
    // ApiResponse construction
    // -----------------------------------------------------------------------

    #[test]
    fn api_response_builder_minimal() {
        let r: ApiResponse<i32> = ApiResponse::builder(42).build();
        assert_eq!(r.data, 42);
        assert!(r.links.is_none());
        assert!(r.meta.request_id.is_none());
    }

    #[test]
    fn api_response_builder_with_meta_and_links() {
        let meta = ResponseMeta::new().request_id("r1").version("2.0");
        let links = Links::new().self_link("/items/1");
        let r: ApiResponse<&str> = ApiResponse::builder("payload")
            .meta(meta)
            .links(links)
            .build();
        assert_eq!(r.data, "payload");
        assert_eq!(r.meta.request_id.as_deref(), Some("r1"));
        assert_eq!(r.meta.version.as_deref(), Some("2.0"));
        assert_eq!(
            r.links.as_ref().unwrap().self_link.as_deref(),
            Some("/items/1")
        );
    }

    #[test]
    fn api_response_composes_with_paginated_response() {
        use crate::pagination::{PaginatedResponse, PaginationParams};
        let params = PaginationParams::default();
        let page = PaginatedResponse::new(vec![1i32, 2, 3], 10, &params);
        let r = ApiResponse::builder(page).build();
        assert_eq!(r.data.total_count, 10);
    }

    // -----------------------------------------------------------------------
    // Serde round-trips
    // -----------------------------------------------------------------------

    #[cfg(feature = "serde")]
    #[test]
    fn api_response_serde_round_trip_minimal() {
        let r: ApiResponse<i32> = ApiResponse::builder(99).build();
        let json = serde_json::to_value(&r).unwrap();
        // links omitted when None
        assert!(json.get("links").is_none());
        assert_eq!(json["data"], 99);
        let back: ApiResponse<i32> = serde_json::from_value(json).unwrap();
        assert_eq!(back, r);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn api_response_serde_round_trip_full() {
        let meta = ResponseMeta::new().request_id("abc").version("1.0");
        let links = Links::new().self_link("/x");
        let r: ApiResponse<String> = ApiResponse::builder("hello".to_string())
            .meta(meta)
            .links(links)
            .build();
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["data"], "hello");
        assert_eq!(json["meta"]["request_id"], "abc");
        assert_eq!(json["links"]["self"], "/x");
        let back: ApiResponse<String> = serde_json::from_value(json).unwrap();
        assert_eq!(back, r);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn response_meta_omits_none_fields() {
        let m = ResponseMeta::new().request_id("id1");
        let json = serde_json::to_value(&m).unwrap();
        assert!(json.get("timestamp").is_none());
        assert!(json.get("version").is_none());
        assert_eq!(json["request_id"], "id1");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn links_omits_none_fields() {
        let l = Links::new().self_link("/a");
        let json = serde_json::to_value(&l).unwrap();
        assert!(json.get("next").is_none());
        assert!(json.get("prev").is_none());
        assert_eq!(json["self"], "/a");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn snapshot_api_response_full() {
        let meta = ResponseMeta::new().request_id("req-xyz").version("1.4.0");
        let links = Links::new().self_link("/items/1").next("/items?after=1");
        let r: ApiResponse<serde_json::Value> = ApiResponse::builder(serde_json::json!({"id": 1}))
            .meta(meta)
            .links(links)
            .build();
        let json = serde_json::to_value(&r).unwrap();
        let expected = serde_json::json!({
            "data": {"id": 1},
            "meta": {"request_id": "req-xyz", "version": "1.4.0"},
            "links": {"self": "/items/1", "next": "/items?after=1"}
        });
        assert_eq!(json, expected);
    }
}
