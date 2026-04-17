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
