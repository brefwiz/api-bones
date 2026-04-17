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

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    // Helper structs for each sub-module

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Standard {
        #[serde(with = "super::standard")]
        data: Vec<u8>,
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct StandardNoPad {
        #[serde(with = "super::standard_no_pad")]
        data: Vec<u8>,
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct UrlSafe {
        #[serde(with = "super::url_safe")]
        data: Vec<u8>,
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct UrlSafeNoPad {
        #[serde(with = "super::url_safe_no_pad")]
        data: Vec<u8>,
    }

    // --- standard ---

    #[test]
    fn standard_serialize() {
        let s = Standard {
            data: vec![0xde, 0xad, 0xbe, 0xef],
        };
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, r#"{"data":"3q2+7w=="}"#);
    }

    #[test]
    fn standard_deserialize() {
        let s: Standard = serde_json::from_str(r#"{"data":"3q2+7w=="}"#).unwrap();
        assert_eq!(s.data, vec![0xde, 0xad, 0xbe, 0xef]);
    }

    #[test]
    fn standard_roundtrip() {
        let original = Standard {
            data: vec![1, 2, 3, 4, 5],
        };
        let json = serde_json::to_string(&original).unwrap();
        let back: Standard = serde_json::from_str(&json).unwrap();
        assert_eq!(back, original);
    }

    #[test]
    fn standard_deserialize_invalid() {
        let result: Result<Standard, _> = serde_json::from_str(r#"{"data":"not!!base64"}"#);
        assert!(result.is_err());
    }

    // --- standard_no_pad ---

    #[test]
    fn standard_no_pad_serialize() {
        let s = StandardNoPad {
            data: vec![0xde, 0xad, 0xbe, 0xef],
        };
        let json = serde_json::to_string(&s).unwrap();
        // No trailing '=' padding
        assert_eq!(json, r#"{"data":"3q2+7w"}"#);
    }

    #[test]
    fn standard_no_pad_deserialize() {
        let s: StandardNoPad = serde_json::from_str(r#"{"data":"3q2+7w"}"#).unwrap();
        assert_eq!(s.data, vec![0xde, 0xad, 0xbe, 0xef]);
    }

    #[test]
    fn standard_no_pad_roundtrip() {
        let original = StandardNoPad {
            data: vec![10, 20, 30],
        };
        let json = serde_json::to_string(&original).unwrap();
        let back: StandardNoPad = serde_json::from_str(&json).unwrap();
        assert_eq!(back, original);
    }

    #[test]
    fn standard_no_pad_deserialize_invalid() {
        let result: Result<StandardNoPad, _> = serde_json::from_str(r#"{"data":"!!!!"}"#);
        assert!(result.is_err());
    }

    // --- url_safe ---

    #[test]
    fn url_safe_serialize() {
        // bytes that produce '+' and '/' in standard become '-' and '_' in url-safe
        let s = UrlSafe {
            data: vec![0xfb, 0xff, 0xfe],
        };
        let json = serde_json::to_string(&s).unwrap();
        // 0xfb 0xff 0xfe => standard "+//+" → url-safe "-__+"  with padding
        assert!(json.contains("data"));
        // Ensure no '+' or '/' characters in the encoded value
        let encoded = json
            .trim_matches(|c| c == '{' || c == '}')
            .split(':')
            .nth(1)
            .unwrap_or("");
        assert!(!encoded.contains('+'));
        assert!(!encoded.contains('/'));
    }

    #[test]
    fn url_safe_deserialize() {
        let s: UrlSafe = serde_json::from_str(r#"{"data":"-_8="}"#).unwrap();
        assert_eq!(s.data, vec![0xfb, 0xff]);
    }

    #[test]
    fn url_safe_roundtrip() {
        let original = UrlSafe {
            data: vec![0xfb, 0xff, 0xfe, 0x01],
        };
        let json = serde_json::to_string(&original).unwrap();
        let back: UrlSafe = serde_json::from_str(&json).unwrap();
        assert_eq!(back, original);
    }

    #[test]
    fn url_safe_deserialize_invalid() {
        let result: Result<UrlSafe, _> = serde_json::from_str(r#"{"data":"not!!valid"}"#);
        assert!(result.is_err());
    }

    // --- url_safe_no_pad ---

    #[test]
    fn url_safe_no_pad_serialize() {
        let s = UrlSafeNoPad {
            data: vec![0xfb, 0xff],
        };
        let json = serde_json::to_string(&s).unwrap();
        // No padding, url-safe alphabet
        assert_eq!(json, r#"{"data":"-_8"}"#);
    }

    #[test]
    fn url_safe_no_pad_deserialize() {
        let s: UrlSafeNoPad = serde_json::from_str(r#"{"data":"-_8"}"#).unwrap();
        assert_eq!(s.data, vec![0xfb, 0xff]);
    }

    #[test]
    fn url_safe_no_pad_roundtrip() {
        let original = UrlSafeNoPad {
            data: vec![0xaa, 0xbb, 0xcc, 0xdd],
        };
        let json = serde_json::to_string(&original).unwrap();
        let back: UrlSafeNoPad = serde_json::from_str(&json).unwrap();
        assert_eq!(back, original);
    }

    #[test]
    fn url_safe_no_pad_deserialize_invalid() {
        let result: Result<UrlSafeNoPad, _> = serde_json::from_str(r#"{"data":"!!!!"}"#);
        assert!(result.is_err());
    }

    // --- empty bytes edge cases ---

    #[test]
    fn standard_empty_bytes_roundtrip() {
        let original = Standard { data: vec![] };
        let json = serde_json::to_string(&original).unwrap();
        let back: Standard = serde_json::from_str(&json).unwrap();
        assert_eq!(back, original);
    }

    #[test]
    fn url_safe_no_pad_empty_bytes_roundtrip() {
        let original = UrlSafeNoPad { data: vec![] };
        let json = serde_json::to_string(&original).unwrap();
        let back: UrlSafeNoPad = serde_json::from_str(&json).unwrap();
        assert_eq!(back, original);
    }
}
