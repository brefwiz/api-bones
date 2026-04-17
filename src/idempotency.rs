//! `IdempotencyKey` newtype for safe retry of non-idempotent HTTP methods.
//!
//! An idempotency key is an opaque string (or UUID) that a client sends once
//! per logical operation. The server uses it to detect duplicate requests and
//! return the cached outcome instead of re-executing the operation.
//!
//! # Constraints
//!
//! - Length: 1–255 characters (inclusive).
//! - Characters: printable ASCII only (`0x20`–`0x7E`), i.e. no control
//!   characters or non-ASCII bytes.
//!
//! # Example
//!
//! ```rust
//! use api_bones::idempotency::IdempotencyKey;
//!
//! // From an arbitrary string
//! let key = IdempotencyKey::new("my-op-abc123").unwrap();
//! assert_eq!(key.as_str(), "my-op-abc123");
//!
//! // From a freshly generated UUID
//! let key = IdempotencyKey::from_uuid();
//! assert!(!key.as_str().is_empty());
//! ```

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{borrow::ToOwned, string::String};
use core::{fmt, ops::Deref};
#[cfg(feature = "serde")]
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

// ---------------------------------------------------------------------------
// IdempotencyKeyError
// ---------------------------------------------------------------------------

/// Errors that can occur when constructing an [`IdempotencyKey`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum IdempotencyKeyError {
    /// The input was empty.
    #[error("idempotency key must not be empty")]
    Empty,
    /// The input exceeds 255 characters.
    #[error("idempotency key must not exceed 255 characters")]
    TooLong,
    /// The input contains non-printable or non-ASCII characters.
    #[error("idempotency key may only contain printable ASCII characters (0x20–0x7E)")]
    InvalidChars,
}

// ---------------------------------------------------------------------------
// IdempotencyKey
// ---------------------------------------------------------------------------

/// A validated idempotency key for safe POST/PATCH retry semantics.
///
/// See the [module-level documentation](self) for the full invariant set.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct IdempotencyKey(String);

impl IdempotencyKey {
    /// Construct an `IdempotencyKey` from any string, returning an error if the
    /// value violates any constraint.
    ///
    /// # Errors
    ///
    /// Returns an [`IdempotencyKeyError`] variant that describes which
    /// constraint failed.
    ///
    /// ```rust
    /// use api_bones::idempotency::{IdempotencyKey, IdempotencyKeyError};
    ///
    /// assert!(IdempotencyKey::new("abc-123").is_ok());
    /// assert_eq!(IdempotencyKey::new(""), Err(IdempotencyKeyError::Empty));
    /// assert!(IdempotencyKey::new("a\x00b").is_err());
    /// ```
    pub fn new(s: impl AsRef<str>) -> Result<Self, IdempotencyKeyError> {
        let s = s.as_ref();
        if s.is_empty() {
            return Err(IdempotencyKeyError::Empty);
        }
        if s.len() > 255 {
            return Err(IdempotencyKeyError::TooLong);
        }
        if !s.bytes().all(|b| (0x20..=0x7E).contains(&b)) {
            return Err(IdempotencyKeyError::InvalidChars);
        }
        Ok(Self(s.to_owned()))
    }

    /// Generate a fresh `IdempotencyKey` backed by a UUID v4.
    ///
    /// The resulting key is the standard hyphenated UUID string, e.g.
    /// `"550e8400-e29b-41d4-a716-446655440000"`.
    ///
    /// ```rust
    /// use api_bones::idempotency::IdempotencyKey;
    ///
    /// let key = IdempotencyKey::from_uuid();
    /// assert_eq!(key.as_str().len(), 36);
    /// ```
    #[must_use]
    pub fn from_uuid() -> Self {
        // SAFETY: UUID hyphenated string is always 36 printable ASCII chars.
        Self(uuid::Uuid::new_v4().to_string())
    }

    /// Return the inner string slice.
    ///
    /// ```rust
    /// use api_bones::idempotency::IdempotencyKey;
    ///
    /// let key = IdempotencyKey::new("abc").unwrap();
    /// assert_eq!(key.as_str(), "abc");
    /// ```
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume the key and return the underlying `String`.
    ///
    /// ```rust
    /// use api_bones::idempotency::IdempotencyKey;
    ///
    /// let key = IdempotencyKey::new("abc").unwrap();
    /// assert_eq!(key.into_string(), "abc");
    /// ```
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

// ---------------------------------------------------------------------------
// Standard trait impls
// ---------------------------------------------------------------------------

impl Deref for IdempotencyKey {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for IdempotencyKey {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for IdempotencyKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for IdempotencyKey {
    type Error = IdempotencyKeyError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::new(s)
    }
}

impl TryFrom<&str> for IdempotencyKey {
    type Error = IdempotencyKeyError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Self::new(s)
    }
}

// ---------------------------------------------------------------------------
// Serde
// ---------------------------------------------------------------------------

#[cfg(feature = "serde")]
impl<'de> Deserialize<'de> for IdempotencyKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::new(&s).map_err(serde::de::Error::custom)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_key_is_accepted() {
        assert!(IdempotencyKey::new("abc-123").is_ok());
        assert!(IdempotencyKey::new("x").is_ok());
        assert!(IdempotencyKey::new("Hello World!").is_ok());
        // printable ASCII boundary chars
        assert!(IdempotencyKey::new(" ").is_ok()); // 0x20
        assert!(IdempotencyKey::new("~").is_ok()); // 0x7E
    }

    #[test]
    fn empty_is_rejected() {
        assert_eq!(IdempotencyKey::new(""), Err(IdempotencyKeyError::Empty));
    }

    #[test]
    fn too_long_is_rejected() {
        let s: String = "a".repeat(256);
        assert_eq!(
            IdempotencyKey::new(&s),
            Err(IdempotencyKeyError::TooLong)
        );
    }

    #[test]
    fn exactly_255_chars_is_accepted() {
        let s: String = "a".repeat(255);
        assert!(IdempotencyKey::new(&s).is_ok());
    }

    #[test]
    fn control_char_is_rejected() {
        assert_eq!(
            IdempotencyKey::new("ab\x00cd"),
            Err(IdempotencyKeyError::InvalidChars)
        );
        assert_eq!(
            IdempotencyKey::new("ab\ncd"),
            Err(IdempotencyKeyError::InvalidChars)
        );
    }

    #[test]
    fn non_ascii_is_rejected() {
        assert_eq!(
            IdempotencyKey::new("héllo"),
            Err(IdempotencyKeyError::InvalidChars)
        );
    }

    #[test]
    fn from_uuid_produces_valid_key() {
        let key = IdempotencyKey::from_uuid();
        // UUID v4 hyphenated = 36 chars, all printable ASCII
        assert_eq!(key.as_str().len(), 36);
        assert!(IdempotencyKey::new(key.as_str()).is_ok());
    }

    #[test]
    fn deref_to_str() {
        let key = IdempotencyKey::new("hello").unwrap();
        let s: &str = &key;
        assert_eq!(s, "hello");
    }

    #[test]
    fn display() {
        let key = IdempotencyKey::new("test-key").unwrap();
        assert_eq!(format!("{key}"), "test-key");
    }

    #[test]
    fn try_from_str() {
        assert!(IdempotencyKey::try_from("valid").is_ok());
        assert!(IdempotencyKey::try_from("").is_err());
    }

    #[test]
    fn try_from_string() {
        assert!(IdempotencyKey::try_from("valid".to_owned()).is_ok());
    }

    #[test]
    fn into_string() {
        let key = IdempotencyKey::new("abc").unwrap();
        assert_eq!(key.into_string(), "abc");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_roundtrip() {
        let key = IdempotencyKey::new("my-key-123").unwrap();
        let json = serde_json::to_string(&key).unwrap();
        assert_eq!(json, r#""my-key-123""#);
        let back: IdempotencyKey = serde_json::from_str(&json).unwrap();
        assert_eq!(back, key);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_deserialize_invalid_rejects() {
        let result: Result<IdempotencyKey, _> = serde_json::from_str(r#""""#);
        assert!(result.is_err());
    }
}
