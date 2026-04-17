//! Flexible timestamp serde: deserializes from Unix epoch (integer or float)
//! **or** ISO 8601 / RFC 3339 string; always serializes as an RFC 3339 string.
//!
//! Requires the `chrono` feature.
//!
//! ## Wire formats accepted on deserialization
//!
//! | Input                        | Interpretation                      |
//! |------------------------------|-------------------------------------|
//! | `1_700_000_000` (i64)        | Unix epoch seconds                  |
//! | `1_700_000_000.5` (f64)      | Unix epoch seconds + sub-second     |
//! | `"2023-11-14T22:13:20Z"`     | RFC 3339 / ISO 8601 string          |
//!
//! ## Examples
//!
//! ```rust
//! use chrono::{DateTime, Utc};
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Debug, PartialEq, Serialize, Deserialize)]
//! struct Event {
//!     #[serde(with = "api_bones::serde::timestamp")]
//!     occurred_at: DateTime<Utc>,
//! }
//!
//! // Deserialize from epoch integer
//! let from_epoch: Event = serde_json::from_str(r#"{"occurred_at":0}"#).unwrap();
//! assert_eq!(from_epoch.occurred_at, DateTime::<Utc>::from_timestamp(0, 0).unwrap());
//!
//! // Deserialize from RFC 3339 string
//! let from_str: Event =
//!     serde_json::from_str(r#"{"occurred_at":"1970-01-01T00:00:00+00:00"}"#).unwrap();
//! assert_eq!(from_str.occurred_at, from_epoch.occurred_at);
//!
//! // Always serializes as RFC 3339
//! let json = serde_json::to_string(&from_epoch).unwrap();
//! assert_eq!(json, r#"{"occurred_at":"1970-01-01T00:00:00+00:00"}"#);
//! ```

use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserializer, Serializer, de};

/// Serialize a [`DateTime<Utc>`] as an RFC 3339 string.
///
/// # Errors
///
/// Returns a serialization error if the serializer rejects the string.
pub fn serialize<S>(value: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&value.to_rfc3339())
}

/// Deserialize a [`DateTime<Utc>`] from a Unix epoch integer, float, or RFC 3339 string.
///
/// # Errors
///
/// Returns a deserialization error if the input cannot be parsed as a valid
/// timestamp.
pub fn deserialize<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_any(TimestampVisitor)
}

struct TimestampVisitor;

impl de::Visitor<'_> for TimestampVisitor {
    type Value = DateTime<Utc>;

    fn expecting(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str("a Unix epoch integer, float, or RFC 3339 timestamp string")
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
        Utc.timestamp_opt(v, 0)
            .single()
            .ok_or_else(|| E::custom(format!("timestamp out of range: {v}")))
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
        let secs = i64::try_from(v).map_err(|_| E::custom("timestamp out of i64 range"))?;
        self.visit_i64(secs)
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
        let secs = v.floor() as i64;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let nanos = ((v - v.floor()) * 1_000_000_000.0).round() as u32;
        Utc.timestamp_opt(secs, nanos)
            .single()
            .ok_or_else(|| E::custom(format!("timestamp out of range: {v}")))
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
        v.parse::<DateTime<Utc>>().map_err(E::custom)
    }
}
