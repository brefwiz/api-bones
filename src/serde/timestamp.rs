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
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
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

#[cfg(test)]
mod tests {
    use chrono::{DateTime, TimeZone, Utc};
    use serde::{Deserialize, Serialize};

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Event {
        #[serde(with = "super")]
        ts: DateTime<Utc>,
    }

    fn epoch() -> DateTime<Utc> {
        Utc.timestamp_opt(0, 0).unwrap()
    }

    // --- serialize ---

    #[test]
    fn serialize_as_rfc3339() {
        let e = Event { ts: epoch() };
        let json = serde_json::to_string(&e).unwrap();
        assert_eq!(json, r#"{"ts":"1970-01-01T00:00:00+00:00"}"#);
    }

    #[test]
    fn serialize_non_epoch() {
        let ts = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let e = Event { ts };
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("2023-"));
    }

    // --- deserialize: visit_i64 (negative or positive integer) ---

    #[test]
    fn deserialize_from_i64_zero() {
        let e: Event = serde_json::from_str(r#"{"ts":0}"#).unwrap();
        assert_eq!(e.ts, epoch());
    }

    #[test]
    fn deserialize_from_i64_positive() {
        let e: Event = serde_json::from_str(r#"{"ts":1700000000}"#).unwrap();
        assert_eq!(e.ts, Utc.timestamp_opt(1_700_000_000, 0).unwrap());
    }

    #[test]
    fn deserialize_from_i64_negative() {
        let e: Event = serde_json::from_str(r#"{"ts":-1}"#).unwrap();
        assert_eq!(e.ts, Utc.timestamp_opt(-1, 0).unwrap());
    }

    // --- deserialize: visit_u64 (large positive integer) ---

    #[test]
    fn deserialize_from_u64() {
        // serde_json sends positive integers that fit u64 as u64
        let e: Event =
            serde_json::from_value(serde_json::json!({"ts": 1_700_000_000_u64})).unwrap();
        assert_eq!(e.ts, Utc.timestamp_opt(1_700_000_000, 0).unwrap());
    }

    // --- deserialize: visit_f64 (fractional epoch) ---

    #[test]
    fn deserialize_from_f64_with_fraction() {
        let e: Event = serde_json::from_str(r#"{"ts":1700000000.5}"#).unwrap();
        let expected = Utc.timestamp_opt(1_700_000_000, 500_000_000).unwrap();
        assert_eq!(e.ts, expected);
    }

    #[test]
    fn deserialize_from_f64_whole() {
        let e: Event = serde_json::from_str(r#"{"ts":0.0}"#).unwrap();
        assert_eq!(e.ts, epoch());
    }

    // --- deserialize: visit_str (RFC 3339 string) ---

    #[test]
    fn deserialize_from_rfc3339_utc() {
        let e: Event = serde_json::from_str(r#"{"ts":"1970-01-01T00:00:00+00:00"}"#).unwrap();
        assert_eq!(e.ts, epoch());
    }

    #[test]
    fn deserialize_from_rfc3339_z_suffix() {
        let e: Event = serde_json::from_str(r#"{"ts":"1970-01-01T00:00:00Z"}"#).unwrap();
        assert_eq!(e.ts, epoch());
    }

    #[test]
    fn deserialize_from_rfc3339_non_epoch() {
        let e: Event = serde_json::from_str(r#"{"ts":"2023-11-14T22:13:20+00:00"}"#).unwrap();
        assert_eq!(e.ts, Utc.timestamp_opt(1_700_000_000, 0).unwrap());
    }

    // --- error paths ---

    #[test]
    fn deserialize_invalid_string() {
        let result: Result<Event, _> = serde_json::from_str(r#"{"ts":"not-a-date"}"#);
        assert!(result.is_err());
    }

    #[test]
    fn deserialize_u64_out_of_i64_range() {
        // u64::MAX cannot fit in i64 → visit_u64 error branch
        let val = serde_json::json!({"ts": u64::MAX});
        let result: Result<Event, _> = serde_json::from_value(val);
        assert!(result.is_err());
    }

    // --- roundtrip ---

    #[test]
    fn roundtrip() {
        let original = Event {
            ts: Utc.timestamp_opt(1_234_567_890, 0).unwrap(),
        };
        let json = serde_json::to_string(&original).unwrap();
        let back: Event = serde_json::from_str(&json).unwrap();
        assert_eq!(back, original);
    }

    // --- expecting: exercise error message path ---

    #[test]
    fn deserialize_invalid_type_triggers_expecting() {
        // Passing a boolean to a timestamp field exercises the `expecting` path in the error
        let result: Result<Event, _> = serde_json::from_str(r#"{"ts":true}"#);
        assert!(result.is_err());
    }

    // --- error paths: visit_i64 out-of-range ---

    #[test]
    fn deserialize_i64_out_of_range() {
        // i64::MAX is far beyond any valid Unix timestamp that chrono accepts,
        // exercising the `ok_or_else` branch in visit_i64.
        let val = serde_json::json!({"ts": i64::MAX});
        let result: Result<Event, _> = serde_json::from_value(val);
        assert!(result.is_err());
    }

    // --- error paths: visit_f64 out-of-range ---

    #[test]
    fn deserialize_f64_out_of_range() {
        // A very large float maps to seconds beyond what chrono can represent.
        let val = serde_json::json!({"ts": 1.0e18_f64});
        let result: Result<Event, _> = serde_json::from_value(val);
        assert!(result.is_err());
    }
}
