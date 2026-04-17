//! Serde helper modules for common API wire-format patterns.
//!
//! Each module is designed to be used with `#[serde(with = "...")]`.
//!
//! | Module          | Use case                                               |
//! |-----------------|--------------------------------------------------------|
//! | [`json_string`] | Value ↔ stringified JSON (`"{\"k\":\"v\"}"`)           |
//! | [`base64_bytes`]| `Vec<u8>` ↔ Base64 string (standard / URL-safe)        |
//! | [`timestamp`]   | `DateTime<Utc>` from epoch or ISO 8601, as RFC 3339    |
//! | [`maybe_string`]| String-or-number coerced to a target type              |

pub mod json_string;

#[cfg(feature = "base64")]
pub mod base64_bytes;

#[cfg(feature = "chrono")]
pub mod timestamp;

pub mod maybe_string;
