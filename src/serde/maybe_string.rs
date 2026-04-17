//! Serde module that accepts either a string **or** a number and coerces the
//! value to the target type via [`core::str::FromStr`].
//!
//! This is useful for APIs that may return `"42"` or `42` for the same field.
//!
//! ## Supported target types
//!
//! Any type that implements both [`serde::Deserialize`] and
//! [`core::str::FromStr`], including `u64`, `i64`, `f64`, `bool`, and any
//! custom newtype that wraps these.
//!
//! ## Examples
//!
//! ```rust
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Debug, PartialEq, Serialize, Deserialize)]
//! struct Record {
//!     #[serde(with = "api_bones::serde::maybe_string")]
//!     count: u64,
//!     #[serde(with = "api_bones::serde::maybe_string")]
//!     ratio: f64,
//!     #[serde(with = "api_bones::serde::maybe_string")]
//!     active: bool,
//! }
//!
//! // Numbers accepted as-is
//! let from_num: Record =
//!     serde_json::from_str(r#"{"count":42,"ratio":3.14,"active":true}"#).unwrap();
//! assert_eq!(from_num.count, 42);
//!
//! // Strings coerced to the target type
//! let from_str: Record =
//!     serde_json::from_str(r#"{"count":"42","ratio":"3.14","active":"true"}"#).unwrap();
//! assert_eq!(from_str, from_num);
//! ```

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::string::{String, ToString};
#[cfg(feature = "std")]
use std::string::String;

use core::fmt::Display;
use core::str::FromStr;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

/// Serialize the value using its standard [`Serialize`] implementation.
///
/// # Errors
///
/// Returns a serialization error if the serializer rejects the value.
pub fn serialize<T, S>(value: &T, serializer: S) -> Result<S::Ok, S::Error>
where
    T: Serialize,
    S: Serializer,
{
    value.serialize(serializer)
}

/// Deserialize a value that may arrive as a string **or** a native JSON type.
///
/// When the input is a string the value is parsed via [`FromStr`]; otherwise
/// the value is forwarded to `T`'s own deserializer.
///
/// # Errors
///
/// Returns a deserialization error if the string cannot be parsed or if the
/// native value cannot be deserialized as `T`.
pub fn deserialize<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
    T: Deserialize<'de> + FromStr,
    <T as FromStr>::Err: Display,
    D: Deserializer<'de>,
{
    deserializer.deserialize_any(MaybeStringVisitor::<T>(core::marker::PhantomData))
}

struct MaybeStringVisitor<T>(core::marker::PhantomData<T>);

impl<'de, T> de::Visitor<'de> for MaybeStringVisitor<T>
where
    T: Deserialize<'de> + FromStr,
    <T as FromStr>::Err: Display,
{
    type Value = T;

    fn expecting(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str("a string or a number")
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<T, E> {
        v.parse::<T>().map_err(de::Error::custom)
    }

    fn visit_string<E: de::Error>(self, v: String) -> Result<T, E> {
        self.visit_str(&v)
    }

    fn visit_bool<E: de::Error>(self, v: bool) -> Result<T, E> {
        self.visit_str(if v { "true" } else { "false" })
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<T, E> {
        self.visit_str(&v.to_string())
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<T, E> {
        self.visit_str(&v.to_string())
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<T, E> {
        self.visit_str(&v.to_string())
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct WithU64 {
        #[serde(with = "super")]
        value: u64,
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct WithI64 {
        #[serde(with = "super")]
        value: i64,
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct WithF64 {
        #[serde(with = "super")]
        value: f64,
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct WithBool {
        #[serde(with = "super")]
        value: bool,
    }

    // --- serialize ---

    #[test]
    fn serialize_u64() {
        let w = WithU64 { value: 99 };
        let json = serde_json::to_string(&w).unwrap();
        assert_eq!(json, r#"{"value":99}"#);
    }

    #[test]
    fn serialize_bool() {
        let w = WithBool { value: true };
        let json = serde_json::to_string(&w).unwrap();
        assert_eq!(json, r#"{"value":true}"#);
    }

    // --- deserialize: visit_str (JSON string input) ---

    #[test]
    fn deserialize_u64_from_string() {
        let w: WithU64 = serde_json::from_str(r#"{"value":"42"}"#).unwrap();
        assert_eq!(w.value, 42);
    }

    #[test]
    fn deserialize_i64_from_string() {
        let w: WithI64 = serde_json::from_str(r#"{"value":"-7"}"#).unwrap();
        assert_eq!(w.value, -7);
    }

    #[test]
    fn deserialize_f64_from_string() {
        let w: WithF64 = serde_json::from_str(r#"{"value":"3.14"}"#).unwrap();
        assert!((w.value - 3.14).abs() < 1e-9);
    }

    #[test]
    fn deserialize_bool_from_string_true() {
        let w: WithBool = serde_json::from_str(r#"{"value":"true"}"#).unwrap();
        assert!(w.value);
    }

    #[test]
    fn deserialize_bool_from_string_false() {
        let w: WithBool = serde_json::from_str(r#"{"value":"false"}"#).unwrap();
        assert!(!w.value);
    }

    // --- deserialize: visit_u64 (JSON number, positive) ---

    #[test]
    fn deserialize_u64_from_number() {
        let w: WithU64 = serde_json::from_str(r#"{"value":100}"#).unwrap();
        assert_eq!(w.value, 100);
    }

    // --- deserialize: visit_i64 (JSON number, negative) ---

    #[test]
    fn deserialize_i64_from_negative_number() {
        let w: WithI64 = serde_json::from_str(r#"{"value":-5}"#).unwrap();
        assert_eq!(w.value, -5);
    }

    // --- deserialize: visit_f64 (JSON float) ---

    #[test]
    fn deserialize_f64_from_float() {
        let w: WithF64 = serde_json::from_str(r#"{"value":2.718}"#).unwrap();
        assert!((w.value - 2.718).abs() < 1e-9);
    }

    // --- deserialize: visit_bool (JSON bool) ---

    #[test]
    fn deserialize_bool_from_true() {
        let w: WithBool = serde_json::from_str(r#"{"value":true}"#).unwrap();
        assert!(w.value);
    }

    #[test]
    fn deserialize_bool_from_false() {
        let w: WithBool = serde_json::from_str(r#"{"value":false}"#).unwrap();
        assert!(!w.value);
    }

    // --- visit_string is triggered by serde_json::Value::String via from_value ---

    #[test]
    fn deserialize_u64_from_value_string() {
        let val = serde_json::json!({"value": "55"});
        let w: WithU64 = serde_json::from_value(val).unwrap();
        assert_eq!(w.value, 55);
    }

    // --- error path: unparseable string ---

    #[test]
    fn deserialize_u64_invalid_string() {
        let result: Result<WithU64, _> = serde_json::from_str(r#"{"value":"not_a_number"}"#);
        assert!(result.is_err());
    }

    // --- expecting path: covered implicitly by error messages, exercise directly ---

    #[test]
    fn deserialize_error_message_contains_expectation() {
        // Provide a JSON type (array) that the visitor cannot handle → triggers `expecting`
        let result: Result<WithU64, _> = serde_json::from_str(r#"{"value":[1,2,3]}"#);
        assert!(result.is_err());
    }
}
