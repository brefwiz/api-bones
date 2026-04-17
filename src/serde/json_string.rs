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

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Inner {
        key: String,
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Outer {
        #[serde(with = "super")]
        payload: Inner,
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct NumWrapper {
        #[serde(with = "super")]
        value: u32,
    }

    #[test]
    fn serialize_struct() {
        let outer = Outer {
            payload: Inner {
                key: "value".to_string(),
            },
        };
        let json = serde_json::to_string(&outer).unwrap();
        assert_eq!(json, r#"{"payload":"{\"key\":\"value\"}"}"#);
    }

    #[test]
    fn deserialize_struct() {
        let json = r#"{"payload":"{\"key\":\"value\"}"}"#;
        let outer: Outer = serde_json::from_str(json).unwrap();
        assert_eq!(
            outer.payload,
            Inner {
                key: "value".to_string()
            }
        );
    }

    #[test]
    fn roundtrip() {
        let original = Outer {
            payload: Inner {
                key: "hello world".to_string(),
            },
        };
        let json = serde_json::to_string(&original).unwrap();
        let back: Outer = serde_json::from_str(&json).unwrap();
        assert_eq!(back, original);
    }

    #[test]
    fn serialize_number() {
        let w = NumWrapper { value: 42 };
        let json = serde_json::to_string(&w).unwrap();
        assert_eq!(json, r#"{"value":"42"}"#);
    }

    #[test]
    fn deserialize_number() {
        let w: NumWrapper = serde_json::from_str(r#"{"value":"42"}"#).unwrap();
        assert_eq!(w.value, 42);
    }

    #[test]
    fn deserialize_invalid_json_string() {
        // The outer string is valid, but its content is not valid JSON for Inner
        let result: Result<Outer, _> = serde_json::from_str(r#"{"payload":"not json"}"#);
        assert!(result.is_err());
    }

    #[test]
    fn deserialize_non_string_field() {
        // The field must be a JSON string, not a raw object
        let result: Result<Outer, _> = serde_json::from_str(r#"{"payload":{"key":"value"}}"#);
        assert!(result.is_err());
    }
}
