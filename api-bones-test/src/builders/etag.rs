use api_bones::etag::ETag;
use chrono::{DateTime, Utc};

/// Convenience constructors for fake [`ETag`] values.
///
/// # Quick start
///
/// ```rust
/// use chrono::Utc;
/// use api_bones_test::builders::FakeETag;
///
/// let tag = FakeETag::for_updated_at(Utc::now());
/// assert!(!tag.weak);
///
/// let weak = FakeETag::weak("abc");
/// assert!(weak.weak);
/// ```
pub struct FakeETag;

impl FakeETag {
    /// Build a strong `ETag` whose value is the RFC 3339 timestamp string.
    #[must_use]
    pub fn for_updated_at(dt: DateTime<Utc>) -> ETag {
        ETag::strong(dt.to_rfc3339())
    }

    /// Build a weak `ETag` with the given opaque value.
    #[must_use]
    pub fn weak(value: impl Into<String>) -> ETag {
        ETag::weak(value)
    }
}
