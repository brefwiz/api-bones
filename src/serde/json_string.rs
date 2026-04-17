//! Serde module for fields that carry stringified JSON on the wire.
//!
//! Use `#[serde(with = "api_bones::serde::json_string")]` to transparently
//! serialize any `T: Serialize` as a JSON string and deserialize a JSON
//! string back into `T: DeserializeOwned`.
//!
//! ## Wire format
//!
//! ```json
//! { "payload": "{\"key\":\"value\"}" }
//! ```
//!
//! ## Examples
//!
//! ```rust
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Debug, PartialEq, Serialize, Deserialize)]
//! struct Inner {
//!     key: String,
//! }
//!
//! #[derive(Debug, PartialEq, Serialize, Deserialize)]
//! struct Outer {
//!     #[serde(with = "api_bones::serde::json_string")]
//!     payload: Inner,
//! }
//!
//! let outer = Outer { payload: Inner { key: "value".to_string() } };
//! let json = serde_json::to_string(&outer).unwrap();
//! assert_eq!(json, r#"{"payload":"{\"key\":\"value\"}"}"#);
//!
//! let back: Outer = serde_json::from_str(&json).unwrap();
//! assert_eq!(back, outer);
//! ```

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::string::String;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::DeserializeOwned};

/// Serialize `value` as a JSON string.
///
/// # Errors
///
/// Returns a serialization error if `value` cannot be serialized to JSON or
/// if the serializer rejects the resulting string.
pub fn serialize<T, S>(value: &T, serializer: S) -> Result<S::Ok, S::Error>
where
    T: Serialize,
    S: Serializer,
{
    let json = serde_json::to_string(value).map_err(serde::ser::Error::custom)?;
    serializer.serialize_str(&json)
}

/// Deserialize a JSON string into `T`.
///
/// # Errors
///
/// Returns a deserialization error if the input is not a string or if the
/// string is not valid JSON for `T`.
pub fn deserialize<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
    T: DeserializeOwned,
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    serde_json::from_str(&s).map_err(serde::de::Error::custom)
}
