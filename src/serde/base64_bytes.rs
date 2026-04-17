//! Serde module for `Vec<u8>` ↔ Base64 string.
//!
//! Provides three sub-modules covering the most common encoding combinations:
//!
//! | Sub-module              | Alphabet    | Padding |
//! |-------------------------|-------------|---------|
//! | `standard`              | `A-Za-z0-9+/` | yes   |
//! | `standard_no_pad`       | `A-Za-z0-9+/` | no    |
//! | `url_safe`              | `A-Za-z0-9-_` | yes   |
//! | `url_safe_no_pad`       | `A-Za-z0-9-_` | no    |
//!
//! ## Examples
//!
//! ```rust
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Debug, PartialEq, Serialize, Deserialize)]
//! struct Payload {
//!     #[serde(with = "api_bones::serde::base64_bytes::standard")]
//!     data: Vec<u8>,
//! }
//!
//! let p = Payload { data: vec![0xde, 0xad, 0xbe, 0xef] };
//! let json = serde_json::to_string(&p).unwrap();
//! assert_eq!(json, r#"{"data":"3q2+7w=="}"#);
//! let back: Payload = serde_json::from_str(&json).unwrap();
//! assert_eq!(back, p);
//! ```

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{string::String, vec::Vec};

use base64::{Engine, engine::GeneralPurpose};
use serde::{Deserialize, Deserializer, Serializer};

fn ser<S>(value: &[u8], serializer: S, engine: &GeneralPurpose) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let encoded = engine.encode(value);
    serializer.serialize_str(&encoded)
}

fn de<'de, D>(deserializer: D, engine: &GeneralPurpose) -> Result<Vec<u8>, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    engine
        .decode(s.as_bytes())
        .map_err(serde::de::Error::custom)
}

/// Standard Base64 alphabet (`A-Za-z0-9+/`) **with** `=` padding.
pub mod standard {
    use super::{de, ser};
    use base64::engine::general_purpose::STANDARD;
    use serde::{Deserializer, Serializer};

    #[cfg(all(not(feature = "std"), feature = "alloc"))]
    use alloc::vec::Vec;

    /// Serialize bytes as a standard Base64 string with padding.
    ///
    /// # Errors
    ///
    /// Returns a serialization error if the serializer rejects the string.
    pub fn serialize<S: Serializer>(value: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        ser(value, serializer, &STANDARD)
    }

    /// Deserialize a standard Base64 string (with padding) into bytes.
    ///
    /// # Errors
    ///
    /// Returns a deserialization error if the input is not a valid Base64 string.
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        de(deserializer, &STANDARD)
    }
}

/// Standard Base64 alphabet (`A-Za-z0-9+/`) **without** padding.
pub mod standard_no_pad {
    use super::{de, ser};
    use base64::engine::general_purpose::STANDARD_NO_PAD;
    use serde::{Deserializer, Serializer};

    #[cfg(all(not(feature = "std"), feature = "alloc"))]
    use alloc::vec::Vec;

    /// Serialize bytes as a standard Base64 string without padding.
    ///
    /// # Errors
    ///
    /// Returns a serialization error if the serializer rejects the string.
    pub fn serialize<S: Serializer>(value: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        ser(value, serializer, &STANDARD_NO_PAD)
    }

    /// Deserialize a standard Base64 string (without padding) into bytes.
    ///
    /// # Errors
    ///
    /// Returns a deserialization error if the input is not a valid Base64 string.
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        de(deserializer, &STANDARD_NO_PAD)
    }
}

/// URL-safe Base64 alphabet (`A-Za-z0-9-_`) **with** `=` padding.
pub mod url_safe {
    use super::{de, ser};
    use base64::engine::general_purpose::URL_SAFE;
    use serde::{Deserializer, Serializer};

    #[cfg(all(not(feature = "std"), feature = "alloc"))]
    use alloc::vec::Vec;

    /// Serialize bytes as a URL-safe Base64 string with padding.
    ///
    /// # Errors
    ///
    /// Returns a serialization error if the serializer rejects the string.
    pub fn serialize<S: Serializer>(value: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        ser(value, serializer, &URL_SAFE)
    }

    /// Deserialize a URL-safe Base64 string (with padding) into bytes.
    ///
    /// # Errors
    ///
    /// Returns a deserialization error if the input is not a valid Base64 string.
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        de(deserializer, &URL_SAFE)
    }
}

/// URL-safe Base64 alphabet (`A-Za-z0-9-_`) **without** padding.
pub mod url_safe_no_pad {
    use super::{de, ser};
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use serde::{Deserializer, Serializer};

    #[cfg(all(not(feature = "std"), feature = "alloc"))]
    use alloc::vec::Vec;

    /// Serialize bytes as a URL-safe Base64 string without padding.
    ///
    /// # Errors
    ///
    /// Returns a serialization error if the serializer rejects the string.
    pub fn serialize<S: Serializer>(value: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        ser(value, serializer, &URL_SAFE_NO_PAD)
    }

    /// Deserialize a URL-safe Base64 string (without padding) into bytes.
    ///
    /// # Errors
    ///
    /// Returns a deserialization error if the input is not a valid Base64 string.
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        de(deserializer, &URL_SAFE_NO_PAD)
    }
}
