//! Opaque cursor encode/decode helpers (Stripe/GitHub style).
//!
//! Cursors hide internal sort keys behind URL-safe base64 encoding. An optional
//! HMAC-SHA256 signature (enabled with the `hmac` feature flag, which is gated
//! behind the `base64` dependency already present for the `auth` feature) can
//! protect cursors from tampering.
//!
//! # Basic usage (no signing)
//!
//! ```rust
//! use api_bones::cursor::Cursor;
//!
//! let encoded = Cursor::encode(b"user:42:created_at");
//! let decoded = Cursor::decode(&encoded).unwrap();
//! assert_eq!(decoded, b"user:42:created_at");
//! ```
//!
//! # Signed cursors
//!
//! ```rust
//! # #[cfg(feature = "hmac")]
//! # {
//! use api_bones::cursor::Cursor;
//!
//! let key = b"supersecret";
//! let signed = Cursor::encode_signed(b"user:42", key);
//! let payload = Cursor::decode_signed(&signed, key).unwrap();
//! assert_eq!(payload, b"user:42");
//! # }
//! ```

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{string::String, vec::Vec};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use core::fmt;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// CursorError
// ---------------------------------------------------------------------------

/// Error returned when a cursor cannot be decoded or its signature is invalid.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum CursorError {
    /// The base64 encoding is malformed.
    InvalidBase64,
    /// The HMAC signature did not match (tampered or wrong key).
    InvalidSignature,
    /// The cursor payload is too short to contain a signature.
    TooShort,
}

impl fmt::Display for CursorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBase64 => f.write_str("cursor: invalid base64 encoding"),
            Self::InvalidSignature => f.write_str("cursor: invalid HMAC signature"),
            Self::TooShort => f.write_str("cursor: payload too short to contain signature"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for CursorError {}

// ---------------------------------------------------------------------------
// Cursor
// ---------------------------------------------------------------------------

/// URL-safe base64 cursor codec.
///
/// Cursors are opaque to API clients — they MUST NOT parse or construct cursor
/// values. Internally they encode an arbitrary byte payload. Optionally they
/// carry a 32-byte HMAC-SHA256 signature to prevent tampering.
pub struct Cursor;

impl Cursor {
    /// Encode a byte payload as a URL-safe, no-padding base64 cursor string.
    ///
    /// # Examples
    ///
    /// ```
    /// use api_bones::cursor::Cursor;
    ///
    /// let c = Cursor::encode(b"id=42");
    /// assert!(!c.is_empty());
    /// // URL-safe alphabet only
    /// assert!(c.chars().all(|ch| ch.is_alphanumeric() || ch == '-' || ch == '_'));
    /// ```
    #[must_use]
    pub fn encode(payload: &[u8]) -> String {
        URL_SAFE_NO_PAD.encode(payload)
    }

    /// Decode a cursor string back to its raw byte payload.
    ///
    /// # Errors
    ///
    /// Returns [`CursorError::InvalidBase64`] when the string is not valid
    /// URL-safe base64.
    ///
    /// # Examples
    ///
    /// ```
    /// use api_bones::cursor::Cursor;
    ///
    /// let encoded = Cursor::encode(b"hello");
    /// assert_eq!(Cursor::decode(&encoded).unwrap(), b"hello");
    /// ```
    pub fn decode(cursor: &str) -> Result<Vec<u8>, CursorError> {
        URL_SAFE_NO_PAD
            .decode(cursor)
            .map_err(|_| CursorError::InvalidBase64)
    }

    /// Encode a payload **with** a 32-byte HMAC-SHA256 signature appended.
    ///
    /// The layout is `base64url(payload || hmac(payload, key))`.
    ///
    /// Requires the `hmac` feature.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "hmac")]
    /// # {
    /// use api_bones::cursor::Cursor;
    ///
    /// let signed = Cursor::encode_signed(b"id=42", b"secret");
    /// assert!(!signed.is_empty());
    /// # }
    /// ```
    #[cfg(feature = "hmac")]
    #[must_use]
    pub fn encode_signed(payload: &[u8], key: &[u8]) -> String {
        use hmac::{Hmac, KeyInit, Mac};
        use sha2::Sha256;

        let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
        mac.update(payload);
        let sig = mac.finalize().into_bytes();

        let mut combined = Vec::with_capacity(payload.len() + 32);
        combined.extend_from_slice(payload);
        combined.extend_from_slice(&sig);
        URL_SAFE_NO_PAD.encode(&combined)
    }

    /// Decode and verify a signed cursor.
    ///
    /// Strips the 32-byte HMAC-SHA256 suffix, recomputes the MAC over the
    /// payload, and returns the payload if the signatures match in constant time.
    ///
    /// Requires the `hmac` feature.
    ///
    /// # Errors
    ///
    /// - [`CursorError::InvalidBase64`] — malformed base64
    /// - [`CursorError::TooShort`] — decoded bytes < 32
    /// - [`CursorError::InvalidSignature`] — MAC mismatch
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "hmac")]
    /// # {
    /// use api_bones::cursor::Cursor;
    ///
    /// let key = b"supersecret";
    /// let signed = Cursor::encode_signed(b"id=42", key);
    /// let payload = Cursor::decode_signed(&signed, key).unwrap();
    /// assert_eq!(payload, b"id=42");
    /// # }
    /// ```
    #[cfg(feature = "hmac")]
    pub fn decode_signed(cursor: &str, key: &[u8]) -> Result<Vec<u8>, CursorError> {
        use hmac::{Hmac, KeyInit, Mac};
        use sha2::Sha256;

        let combined = URL_SAFE_NO_PAD
            .decode(cursor)
            .map_err(|_| CursorError::InvalidBase64)?;
        if combined.len() < 32 {
            return Err(CursorError::TooShort);
        }
        let (payload, stored_sig) = combined.split_at(combined.len() - 32);
        let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
        mac.update(payload);
        mac.verify_slice(stored_sig)
            .map_err(|_| CursorError::InvalidSignature)?;
        Ok(payload.to_vec())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_round_trip() {
        let payload = b"user:42:2024-01-01";
        let encoded = Cursor::encode(payload);
        let decoded = Cursor::decode(&encoded).unwrap();
        assert_eq!(decoded, payload);
    }

    #[test]
    fn encode_uses_url_safe_alphabet() {
        // Run many payloads; none should contain '+' or '/'
        for i in 0u8..=255 {
            let encoded = Cursor::encode(&[i]);
            assert!(
                encoded
                    .chars()
                    .all(|ch| ch.is_alphanumeric() || ch == '-' || ch == '_'),
                "non-url-safe character in {encoded}"
            );
        }
    }

    #[test]
    fn decode_invalid_base64_error() {
        let result = Cursor::decode("!!!not-base64!!!");
        assert_eq!(result.unwrap_err(), CursorError::InvalidBase64);
    }

    #[test]
    fn encode_empty_payload() {
        let encoded = Cursor::encode(b"");
        assert_eq!(encoded, "");
        let decoded = Cursor::decode(&encoded).unwrap();
        assert_eq!(decoded, b"");
    }

    #[cfg(feature = "hmac")]
    #[test]
    fn signed_round_trip() {
        let key = b"test-key-very-secret";
        let payload = b"id=99&sort=asc";
        let signed = Cursor::encode_signed(payload, key);
        let out = Cursor::decode_signed(&signed, key).unwrap();
        assert_eq!(out, payload);
    }

    #[cfg(feature = "hmac")]
    #[test]
    fn signed_wrong_key_fails() {
        let signed = Cursor::encode_signed(b"id=1", b"right-key");
        let result = Cursor::decode_signed(&signed, b"wrong-key");
        assert_eq!(result.unwrap_err(), CursorError::InvalidSignature);
    }

    #[cfg(feature = "hmac")]
    #[test]
    fn signed_tampered_payload_fails() {
        let signed = Cursor::encode_signed(b"id=1", b"key");
        // Flip the first byte by decoding, mutating, re-encoding
        let mut raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(&signed)
            .unwrap();
        raw[0] ^= 0xFF;
        let tampered = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&raw);
        let result = Cursor::decode_signed(&tampered, b"key");
        assert_eq!(result.unwrap_err(), CursorError::InvalidSignature);
    }

    #[cfg(feature = "hmac")]
    #[test]
    fn signed_too_short_error() {
        // A cursor with < 32 bytes after decode
        let short = Cursor::encode(b"tiny");
        let result = Cursor::decode_signed(&short, b"key");
        assert_eq!(result.unwrap_err(), CursorError::TooShort);
    }

    // -----------------------------------------------------------------------
    // Coverage gap: CursorError Display for all variants
    // -----------------------------------------------------------------------

    #[test]
    fn cursor_error_display_all_variants() {
        assert!(!CursorError::InvalidBase64.to_string().is_empty());
        assert!(!CursorError::InvalidSignature.to_string().is_empty());
        assert!(!CursorError::TooShort.to_string().is_empty());
    }
}
