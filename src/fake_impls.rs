//! [`fake::Dummy`] implementations for all public types (behind the `fake` feature flag).
//!
//! Enable the `fake` cargo feature to get realistic test-fixture generation for every
//! public type in this crate:
//!
//! ```toml
//! [dev-dependencies]
//! shared-types = { version = "*", features = ["fake"] }
//! fake = "2"
//! ```
//!
//! Then generate fixtures with:
//!
//! ```rust
//! # #[cfg(feature = "fake")] {
//! use fake::{Fake, Faker};
//! let error: shared_types::ApiError = Faker.fake();
//! # }
//! ```
//!
//! # Realistic generation guarantees
//!
//! - `ApiError::status` is always in `100..=599`.
//! - Timestamps (`AuditInfo`, `ResponseMeta`) are valid RFC 3339 `DateTime<Utc>` values
//!   when the `chrono` feature is enabled, or valid RFC 3339 strings otherwise.
//! - `PaginationParams::limit` and `CursorPaginationParams::limit` are `None` or
//!   `Some(1..=100)`, consistent with the domain constraints.
//! - `SearchParams::query` is a non-empty string of at most 500 bytes.

use fake::{Dummy, Fake, Faker};
use rand::Rng;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn gen_alphanum<R: Rng + ?Sized>(rng: &mut R, len: usize) -> String {
    use rand::distributions::{Alphanumeric, DistString};
    Alphanumeric.sample_string(rng, len)
}

fn gen_str<R: Rng + ?Sized>(rng: &mut R) -> String {
    let len = rng.gen_range(4usize..=24);
    gen_alphanum(rng, len)
}

fn gen_url<R: Rng + ?Sized>(rng: &mut R) -> String {
    format!("https://example.com/{}", gen_alphanum(rng, 8))
}

// ---------------------------------------------------------------------------
// error module
// ---------------------------------------------------------------------------

impl Dummy<Faker> for crate::error::ErrorCode {
    fn dummy_with_rng<R: Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        // 15 variants — generate exactly 0..15
        match rng.gen_range(0u8..15) {
            0 => Self::BadRequest,
            1 => Self::ValidationFailed,
            2 => Self::Unauthorized,
            3 => Self::InvalidCredentials,
            4 => Self::TokenExpired,
            5 => Self::TokenInvalid,
            6 => Self::Forbidden,
            7 => Self::InsufficientPermissions,
            8 => Self::ResourceNotFound,
            9 => Self::Conflict,
            10 => Self::ResourceAlreadyExists,
            11 => Self::UnprocessableEntity,
            12 => Self::RateLimited,
            13 => Self::InternalServerError,
            _ => Self::ServiceUnavailable,
        }
    }
}

impl Dummy<Faker> for crate::error::ErrorTypeMode {
    fn dummy_with_rng<R: Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        if rng.gen_bool(0.5) {
            Self::Url {
                base_url: gen_url(rng),
            }
        } else {
            Self::Urn {
                namespace: gen_alphanum(rng, 8),
            }
        }
    }
}

impl Dummy<Faker> for crate::error::ValidationError {
    fn dummy_with_rng<R: Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        Self {
            field: gen_str(rng),
            message: gen_str(rng),
            rule: if rng.gen_bool(0.5) {
                Some(gen_str(rng))
            } else {
                None
            },
        }
    }
}

impl Dummy<Faker> for crate::error::ApiError {
    fn dummy_with_rng<R: Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        // Realistic HTTP status: 100-599
        let status: u16 = rng.gen_range(100u16..=599);
        let code = Faker.fake_with_rng::<crate::error::ErrorCode, _>(rng);
        let title = code.title().to_owned();
        let detail = gen_str(rng);
        let errors_count = rng.gen_range(0usize..=3);
        let errors = (0..errors_count)
            .map(|_| Faker.fake_with_rng::<crate::error::ValidationError, _>(rng))
            .collect();
        #[cfg(feature = "uuid")]
        let request_id = if rng.gen_bool(0.5) {
            Some(uuid::Uuid::new_v4())
        } else {
            None
        };
        #[cfg(not(feature = "uuid"))]
        let request_id = None;
        Self {
            code,
            title,
            status,
            detail,
            request_id,
            errors,
        }
    }
}

// ---------------------------------------------------------------------------
// etag module
// ---------------------------------------------------------------------------

impl Dummy<Faker> for crate::etag::ETag {
    fn dummy_with_rng<R: Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        Self {
            value: gen_alphanum(rng, 16),
            weak: rng.gen_bool(0.5),
        }
    }
}

impl Dummy<Faker> for crate::etag::IfMatch {
    fn dummy_with_rng<R: Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        if rng.gen_bool(0.5) {
            Self::Any
        } else {
            let n = rng.gen_range(1usize..=4);
            Self::Tags(
                (0..n)
                    .map(|_| Faker.fake_with_rng::<crate::etag::ETag, _>(rng))
                    .collect(),
            )
        }
    }
}

impl Dummy<Faker> for crate::etag::IfNoneMatch {
    fn dummy_with_rng<R: Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        if rng.gen_bool(0.5) {
            Self::Any
        } else {
            let n = rng.gen_range(1usize..=4);
            Self::Tags(
                (0..n)
                    .map(|_| Faker.fake_with_rng::<crate::etag::ETag, _>(rng))
                    .collect(),
            )
        }
    }
}

// ---------------------------------------------------------------------------
// health module
// ---------------------------------------------------------------------------

impl Dummy<Faker> for crate::health::HealthStatus {
    fn dummy_with_rng<R: Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        match rng.gen_range(0u8..3) {
            0 => Self::Pass,
            1 => Self::Fail,
            _ => Self::Warn,
        }
    }
}

impl Dummy<Faker> for crate::health::HealthCheck {
    fn dummy_with_rng<R: Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        Self {
            component_type: gen_str(rng),
            status: Faker.fake_with_rng(rng),
            output: if rng.gen_bool(0.5) {
                Some(gen_str(rng))
            } else {
                None
            },
            time: if rng.gen_bool(0.5) {
                Some(gen_str(rng))
            } else {
                None
            },
        }
    }
}

impl Dummy<Faker> for crate::health::LivenessResponse {
    fn dummy_with_rng<R: Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        Self {
            status: Faker.fake_with_rng(rng),
            version: format!(
                "{}.{}.{}",
                rng.gen_range(0u8..10),
                rng.gen_range(0u8..20),
                rng.gen_range(0u8..100)
            ),
            service_id: gen_alphanum(rng, 12),
        }
    }
}

impl Dummy<Faker> for crate::health::ReadinessResponse {
    fn dummy_with_rng<R: Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        use std::collections::HashMap;
        let mut checks = HashMap::new();
        let num_checks = rng.gen_range(0usize..=3);
        for _ in 0..num_checks {
            let key = format!("{}:connection", gen_str(rng));
            let n = rng.gen_range(1usize..=2);
            let checks_vec: Vec<crate::health::HealthCheck> =
                (0..n).map(|_| Faker.fake_with_rng(rng)).collect();
            checks.insert(key, checks_vec);
        }
        Self {
            status: Faker.fake_with_rng(rng),
            version: format!(
                "{}.{}.{}",
                rng.gen_range(0u8..10),
                rng.gen_range(0u8..20),
                rng.gen_range(0u8..100)
            ),
            service_id: gen_alphanum(rng, 12),
            checks,
        }
    }
}

// ---------------------------------------------------------------------------
// links module
// ---------------------------------------------------------------------------

impl Dummy<Faker> for crate::links::Link {
    fn dummy_with_rng<R: Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        let rels = ["self", "next", "prev", "first", "last", "related"];
        let rel = rels[rng.gen_range(0..rels.len())];
        let mut link = Self::new(rel, gen_url(rng));
        if rng.gen_bool(0.5) {
            let methods = ["GET", "POST", "PUT", "DELETE", "PATCH"];
            link = link.method(methods[rng.gen_range(0..methods.len())]);
        }
        link
    }
}

impl Dummy<Faker> for crate::links::Links {
    fn dummy_with_rng<R: Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        let n = rng.gen_range(0usize..=4);
        let mut links = Self::new();
        for _ in 0..n {
            links = links.push(Faker.fake_with_rng(rng));
        }
        links
    }
}

// ---------------------------------------------------------------------------
// models module
// ---------------------------------------------------------------------------

impl Dummy<Faker> for crate::models::ErrorResponse {
    fn dummy_with_rng<R: Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        Self::new(gen_str(rng))
    }
}

// ---------------------------------------------------------------------------
// pagination module
// ---------------------------------------------------------------------------

impl<T: Dummy<Faker>> Dummy<Faker> for crate::pagination::PaginatedResponse<T> {
    fn dummy_with_rng<R: Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        let n = rng.gen_range(0usize..=5);
        let items: Vec<T> = (0..n).map(|_| Faker.fake_with_rng(rng)).collect();
        let extra: u64 = rng.gen_range(0u64..=20);
        let total_count = n as u64 + extra;
        Self {
            has_more: extra > 0,
            limit: rng.gen_range(1u64..=100),
            offset: rng.gen_range(0u64..=total_count.saturating_sub(n as u64)),
            total_count,
            items,
        }
    }
}

impl Dummy<Faker> for crate::pagination::PaginationParams {
    fn dummy_with_rng<R: Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        // Constraint: None or Some(1..=100)
        Self {
            limit: if rng.gen_bool(0.5) {
                Some(rng.gen_range(1u64..=100))
            } else {
                None
            },
            offset: if rng.gen_bool(0.5) {
                Some(rng.gen_range(0u64..=1000))
            } else {
                None
            },
        }
    }
}

impl Dummy<Faker> for crate::pagination::CursorPagination {
    fn dummy_with_rng<R: Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        let has_more = rng.gen_bool(0.5);
        Self {
            has_more,
            next_cursor: if has_more {
                Some(gen_alphanum(rng, 24))
            } else {
                None
            },
        }
    }
}

impl<T: Dummy<Faker>> Dummy<Faker> for crate::pagination::CursorPaginatedResponse<T> {
    fn dummy_with_rng<R: Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        let n = rng.gen_range(0usize..=5);
        let data: Vec<T> = (0..n).map(|_| Faker.fake_with_rng(rng)).collect();
        Self {
            data,
            pagination: Faker.fake_with_rng(rng),
        }
    }
}

impl Dummy<Faker> for crate::pagination::CursorPaginationParams {
    fn dummy_with_rng<R: Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        // Constraint: None or Some(1..=100)
        Self {
            limit: if rng.gen_bool(0.5) {
                Some(rng.gen_range(1u64..=100))
            } else {
                None
            },
            after: if rng.gen_bool(0.5) {
                Some(gen_alphanum(rng, 24))
            } else {
                None
            },
        }
    }
}

// ---------------------------------------------------------------------------
// query module
// ---------------------------------------------------------------------------

impl Dummy<Faker> for crate::query::SortDirection {
    fn dummy_with_rng<R: Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        if rng.gen_bool(0.5) {
            Self::Asc
        } else {
            Self::Desc
        }
    }
}

impl Dummy<Faker> for crate::query::SortParams {
    fn dummy_with_rng<R: Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        Self::new(gen_str(rng), Faker.fake_with_rng(rng))
    }
}

impl Dummy<Faker> for crate::query::FilterEntry {
    fn dummy_with_rng<R: Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        let operators = ["eq", "ne", "lt", "lte", "gt", "gte", "contains", "in"];
        Self::new(
            gen_str(rng),
            operators[rng.gen_range(0..operators.len())],
            gen_str(rng),
        )
    }
}

impl Dummy<Faker> for crate::query::FilterParams {
    fn dummy_with_rng<R: Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        let n = rng.gen_range(0usize..=4);
        Self::new((0..n).map(|_| Faker.fake_with_rng::<crate::query::FilterEntry, _>(rng)))
    }
}

impl Dummy<Faker> for crate::query::SearchParams {
    fn dummy_with_rng<R: Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        // Constraint: non-empty, at most 500 bytes
        let len = rng.gen_range(1usize..=48);
        let query = gen_alphanum(rng, len);
        let n = rng.gen_range(0usize..=3);
        let fields: Vec<String> = (0..n).map(|_| gen_str(rng)).collect();
        if fields.is_empty() {
            Self::new(query)
        } else {
            Self::with_fields(query, fields)
        }
    }
}

// ---------------------------------------------------------------------------
// ratelimit module
// ---------------------------------------------------------------------------

impl Dummy<Faker> for crate::ratelimit::RateLimitInfo {
    fn dummy_with_rng<R: Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        let limit: u64 = rng.gen_range(1u64..=10_000);
        let remaining: u64 = rng.gen_range(0u64..=limit);
        // reset: Unix timestamp in a reasonable range (year 2020-2040)
        let reset: u64 = rng.gen_range(1_577_836_800u64..=2_208_988_800u64);
        let mut info = Self::new(limit, remaining, reset);
        if remaining == 0 && rng.gen_bool(0.5) {
            info = info.retry_after(rng.gen_range(1u64..=3600));
        }
        info
    }
}

// ---------------------------------------------------------------------------
// audit module
// ---------------------------------------------------------------------------

#[cfg(not(feature = "chrono"))]
impl Dummy<Faker> for crate::audit::AuditInfo {
    fn dummy_with_rng<R: Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        // Without chrono, Timestamp = String; generate RFC 3339-like strings.
        fn rfc3339<R2: Rng + ?Sized>(rng: &mut R2) -> String {
            let year = rng.gen_range(2000u32..=2100);
            let month = rng.gen_range(1u32..=12);
            let day = rng.gen_range(1u32..=28);
            let hour = rng.gen_range(0u32..=23);
            let min = rng.gen_range(0u32..=59);
            let sec = rng.gen_range(0u32..=59);
            format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}Z")
        }
        Self {
            created_at: rfc3339(rng),
            updated_at: rfc3339(rng),
            created_by: if rng.gen_bool(0.5) {
                Some(gen_str(rng))
            } else {
                None
            },
            updated_by: if rng.gen_bool(0.5) {
                Some(gen_str(rng))
            } else {
                None
            },
        }
    }
}

#[cfg(feature = "chrono")]
impl Dummy<Faker> for crate::audit::AuditInfo {
    fn dummy_with_rng<R: Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        // year 2000-2100 as Unix seconds
        let created_secs: i64 = rng.gen_range(946_684_800i64..=4_102_444_800i64);
        let updated_secs: i64 = rng.gen_range(946_684_800i64..=4_102_444_800i64);
        let created_at =
            chrono::DateTime::from_timestamp(created_secs, 0).unwrap_or_else(chrono::Utc::now);
        let updated_at =
            chrono::DateTime::from_timestamp(updated_secs, 0).unwrap_or_else(chrono::Utc::now);
        Self {
            created_at,
            updated_at,
            created_by: if rng.gen_bool(0.5) {
                Some(gen_str(rng))
            } else {
                None
            },
            updated_by: if rng.gen_bool(0.5) {
                Some(gen_str(rng))
            } else {
                None
            },
        }
    }
}

// ---------------------------------------------------------------------------
// response module
// ---------------------------------------------------------------------------

#[cfg(not(feature = "chrono"))]
impl Dummy<Faker> for crate::response::ResponseMeta {
    fn dummy_with_rng<R: Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        let mut meta = Self::new();
        if rng.gen_bool(0.5) {
            meta = meta.request_id(gen_alphanum(rng, 36));
        }
        if rng.gen_bool(0.5) {
            let year = rng.gen_range(2000u32..=2100);
            meta = meta.timestamp(format!("{year:04}-01-01T00:00:00Z"));
        }
        if rng.gen_bool(0.5) {
            meta = meta.version(format!(
                "{}.{}.{}",
                rng.gen_range(0u8..10),
                rng.gen_range(0u8..20),
                rng.gen_range(0u8..100)
            ));
        }
        meta
    }
}

#[cfg(feature = "chrono")]
impl Dummy<Faker> for crate::response::ResponseMeta {
    fn dummy_with_rng<R: Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        let mut meta = Self::new();
        if rng.gen_bool(0.5) {
            meta = meta.request_id(gen_alphanum(rng, 36));
        }
        if rng.gen_bool(0.5) {
            let secs: i64 = rng.gen_range(946_684_800i64..=4_102_444_800i64);
            if let Some(ts) = chrono::DateTime::from_timestamp(secs, 0) {
                meta = meta.timestamp(ts);
            }
        }
        if rng.gen_bool(0.5) {
            meta = meta.version(format!(
                "{}.{}.{}",
                rng.gen_range(0u8..10),
                rng.gen_range(0u8..20),
                rng.gen_range(0u8..100)
            ));
        }
        meta
    }
}

impl Dummy<Faker> for crate::response::Links {
    fn dummy_with_rng<R: Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        let mut links = Self::new();
        if rng.gen_bool(0.5) {
            links = links.self_link(gen_url(rng));
        }
        if rng.gen_bool(0.5) {
            links = links.next(gen_url(rng));
        }
        if rng.gen_bool(0.5) {
            links = links.prev(gen_url(rng));
        }
        links
    }
}

impl<T: Dummy<Faker>> Dummy<Faker> for crate::response::ApiResponse<T> {
    fn dummy_with_rng<R: Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        let data: T = Faker.fake_with_rng(rng);
        let meta = Faker.fake_with_rng::<crate::response::ResponseMeta, _>(rng);
        let links = if rng.gen_bool(0.5) {
            Some(Faker.fake_with_rng::<crate::response::Links, _>(rng))
        } else {
            None
        };
        let mut builder = Self::builder(data).meta(meta);
        if let Some(l) = links {
            builder = builder.links(l);
        }
        builder.build()
    }
}

// ---------------------------------------------------------------------------
// bulk module
// ---------------------------------------------------------------------------

impl<T: Dummy<Faker>> Dummy<Faker> for crate::bulk::BulkRequest<T> {
    fn dummy_with_rng<R: Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        let n = rng.gen_range(1usize..=8);
        let items: Vec<T> = (0..n).map(|_| Faker.fake_with_rng(rng)).collect();
        Self { items }
    }
}

impl<T: Dummy<Faker>> Dummy<Faker> for crate::bulk::BulkItemResult<T> {
    fn dummy_with_rng<R: Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        if rng.gen_bool(0.5) {
            Self::Success {
                data: Faker.fake_with_rng(rng),
            }
        } else {
            Self::Failure {
                index: rng.gen_range(0usize..=100),
                error: Faker.fake_with_rng(rng),
            }
        }
    }
}

impl<T: Dummy<Faker>> Dummy<Faker> for crate::bulk::BulkResponse<T> {
    fn dummy_with_rng<R: Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        let n = rng.gen_range(1usize..=8);
        let results: Vec<crate::bulk::BulkItemResult<T>> =
            (0..n).map(|_| Faker.fake_with_rng(rng)).collect();
        Self { results }
    }
}

// ---------------------------------------------------------------------------
// slug module
// ---------------------------------------------------------------------------

impl Dummy<Faker> for crate::slug::Slug {
    fn dummy_with_rng<R: Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
        let segments = rng.gen_range(1usize..=4);
        let parts: Vec<String> = (0..segments)
            .map(|_| {
                let len = rng.gen_range(2usize..=12);
                (0..len)
                    .map(|_| {
                        let idx = rng.gen_range(0..CHARS.len());
                        CHARS[idx] as char
                    })
                    .collect()
            })
            .collect();
        let s = parts.join("-");
        // Guaranteed valid: only [a-z0-9], joined by single hyphens, non-empty
        crate::slug::Slug::new(s).expect("fake Slug must be valid")
    }
}
