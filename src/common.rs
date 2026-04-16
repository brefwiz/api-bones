//! Common RFC-conformant primitive types used across all external APIs.
//!
//! ## Standards
//! - Timestamps: [RFC 3339](https://www.rfc-editor.org/rfc/rfc3339) (Internet Date/Time Format)
//! - Identifiers: [RFC 4122](https://www.rfc-editor.org/rfc/rfc4122) (UUID)

#[cfg(all(not(feature = "std"), feature = "alloc", not(feature = "chrono")))]
use alloc::string::String;

/// RFC 3339 timestamp alias for API responses.
///
/// Serializes as `"2026-03-09T15:00:00Z"` via `chrono`'s serde integration.
/// See [RFC 3339](https://www.rfc-editor.org/rfc/rfc3339).
#[cfg(feature = "chrono")]
pub type Timestamp = chrono::DateTime<chrono::Utc>;

/// RFC 3339 timestamp alias (string fallback when `chrono` feature is disabled).
///
/// Requires `std` or `alloc` when `chrono` is disabled.
#[cfg(all(not(feature = "chrono"), any(feature = "std", feature = "alloc")))]
pub type Timestamp = String;

/// RFC 4122 UUID v4 resource identifier.
///
/// See [RFC 4122](https://www.rfc-editor.org/rfc/rfc4122).
#[cfg(feature = "uuid")]
pub type ResourceId = uuid::Uuid;

/// Parse an RFC 3339 timestamp string.
///
/// See [RFC 3339](https://www.rfc-editor.org/rfc/rfc3339).
///
/// # Errors
///
/// Returns a `chrono::ParseError` if `s` is not a valid RFC 3339 timestamp.
#[cfg(feature = "chrono")]
pub fn parse_timestamp(s: &str) -> Result<Timestamp, chrono::ParseError> {
    s.parse()
}

/// Generate a new RFC 4122 v4 resource identifier.
///
/// See [RFC 4122 §4.4](https://www.rfc-editor.org/rfc/rfc4122#section-4.4).
#[cfg(feature = "uuid")]
#[must_use]
pub fn new_resource_id() -> ResourceId {
    uuid::Uuid::new_v4()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "uuid")]
    #[test]
    fn resource_id_is_v4() {
        let id = new_resource_id();
        assert_eq!(id.get_version_num(), 4);
    }

    #[cfg(feature = "chrono")]
    #[test]
    fn timestamp_parses_rfc3339() {
        // RFC 3339 format: YYYY-MM-DDTHH:MM:SSZ
        let ts = parse_timestamp("2026-03-09T15:00:00Z").unwrap();
        assert_eq!(ts.to_rfc3339(), "2026-03-09T15:00:00+00:00");
    }
}
