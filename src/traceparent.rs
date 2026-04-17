//! W3C Trace Context types: `TraceId`, `SpanId`, and `TraceContext`.
//!
//! This module implements the
//! [W3C Trace Context Level 1](https://www.w3.org/TR/trace-context/) spec,
//! covering the `traceparent` header format:
//!
//! ```text
//! traceparent: 00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01
//!              ^^ ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ ^^^^^^^^^^^^^^^^ ^^
//!              version  trace-id (32 hex)          span-id (16 hex) flags
//! ```
//!
//! # Example
//!
//! ```rust
//! use api_bones::traceparent::{TraceContext, SamplingFlags};
//!
//! let tc: TraceContext = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
//!     .parse()
//!     .unwrap();
//!
//! assert!(tc.flags.is_sampled());
//! assert_eq!(tc.to_string(),
//!     "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01");
//! ```

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::string::{String, ToString};
use core::{fmt, str::FromStr};
#[cfg(feature = "serde")]
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Error returned when parsing a `traceparent` header fails.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TraceContextError {
    /// The overall format is wrong (wrong number of fields, wrong lengths, etc.).
    #[error("invalid traceparent format")]
    InvalidFormat,
    /// The version byte is not supported (only `00` is currently valid).
    #[error("unsupported traceparent version: must be \"00\"")]
    UnsupportedVersion,
    /// The trace-id field is all zeros, which the spec forbids.
    #[error("trace-id must not be all zeros")]
    ZeroTraceId,
    /// The span-id field is all zeros, which the spec forbids.
    #[error("span-id must not be all zeros")]
    ZeroSpanId,
}

// ---------------------------------------------------------------------------
// TraceId
// ---------------------------------------------------------------------------

/// A 128-bit W3C trace identifier, encoded as 32 lowercase hex characters.
///
/// The all-zeros value is invalid per the W3C spec and will never be produced
/// by [`TraceId::new`] or accepted by [`TraceId::from_str`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct TraceId([u8; 16]);

impl TraceId {
    /// Generate a new random `TraceId` (backed by UUID v4 bytes).
    ///
    /// ```rust
    /// use api_bones::traceparent::TraceId;
    ///
    /// let id = TraceId::new();
    /// assert!(!id.is_zero());
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self(*uuid::Uuid::new_v4().as_bytes())
    }

    /// Construct from raw bytes.
    ///
    /// Returns `None` if the bytes are all zero (invalid per W3C spec).
    #[must_use]
    pub fn from_bytes(bytes: [u8; 16]) -> Option<Self> {
        if bytes == [0u8; 16] {
            None
        } else {
            Some(Self(bytes))
        }
    }

    /// Return the raw bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    /// Returns `true` if all bytes are zero (invalid, but possible via unsafe
    /// construction paths).
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.0 == [0u8; 16]
    }

    /// Encode as a 32-character lowercase hex string.
    #[must_use]
    pub fn to_hex(&self) -> String {
        self.to_string()
    }
}

impl Default for TraceId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for TraceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in &self.0 {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

impl FromStr for TraceId {
    type Err = TraceContextError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 32 {
            return Err(TraceContextError::InvalidFormat);
        }
        let mut bytes = [0u8; 16];
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16)
                .map_err(|_| TraceContextError::InvalidFormat)?;
        }
        if bytes == [0u8; 16] {
            return Err(TraceContextError::ZeroTraceId);
        }
        Ok(Self(bytes))
    }
}

#[cfg(feature = "serde")]
impl Serialize for TraceId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

#[cfg(feature = "serde")]
impl<'de> Deserialize<'de> for TraceId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

// ---------------------------------------------------------------------------
// SpanId
// ---------------------------------------------------------------------------

/// A 64-bit W3C span identifier, encoded as 16 lowercase hex characters.
///
/// The all-zeros value is invalid per the W3C spec and will never be produced
/// by [`SpanId::new`] or accepted by [`SpanId::from_str`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SpanId([u8; 8]);

impl SpanId {
    /// Generate a new random `SpanId`.
    ///
    /// ```rust
    /// use api_bones::traceparent::SpanId;
    ///
    /// let id = SpanId::new();
    /// assert!(!id.is_zero());
    /// ```
    #[must_use]
    pub fn new() -> Self {
        // Use the first 8 bytes of a UUID v4 for randomness.
        let uuid = uuid::Uuid::new_v4();
        let b = uuid.as_bytes();
        let mut arr = [0u8; 8];
        arr.copy_from_slice(&b[..8]);
        // Extremely unlikely to be all-zero, but ensure it.
        if arr == [0u8; 8] {
            arr[0] = 1;
        }
        Self(arr)
    }

    /// Construct from raw bytes.
    ///
    /// Returns `None` if the bytes are all zero (invalid per W3C spec).
    #[must_use]
    pub fn from_bytes(bytes: [u8; 8]) -> Option<Self> {
        if bytes == [0u8; 8] {
            None
        } else {
            Some(Self(bytes))
        }
    }

    /// Return the raw bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 8] {
        &self.0
    }

    /// Returns `true` if all bytes are zero.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.0 == [0u8; 8]
    }

    /// Encode as a 16-character lowercase hex string.
    #[must_use]
    pub fn to_hex(&self) -> String {
        self.to_string()
    }
}

impl Default for SpanId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for SpanId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in &self.0 {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

impl FromStr for SpanId {
    type Err = TraceContextError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 16 {
            return Err(TraceContextError::InvalidFormat);
        }
        let mut bytes = [0u8; 8];
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16)
                .map_err(|_| TraceContextError::InvalidFormat)?;
        }
        if bytes == [0u8; 8] {
            return Err(TraceContextError::ZeroSpanId);
        }
        Ok(Self(bytes))
    }
}

#[cfg(feature = "serde")]
impl Serialize for SpanId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

#[cfg(feature = "serde")]
impl<'de> Deserialize<'de> for SpanId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

// ---------------------------------------------------------------------------
// SamplingFlags
// ---------------------------------------------------------------------------

/// W3C Trace Context sampling flags byte.
///
/// Currently only the `sampled` flag (bit 0) is defined by the spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SamplingFlags(u8);

impl SamplingFlags {
    /// The `sampled` flag — bit 0 of the flags byte.
    pub const SAMPLED: u8 = 0x01;

    /// Create flags from a raw byte.
    #[must_use]
    pub const fn from_byte(b: u8) -> Self {
        Self(b)
    }

    /// Create flags with the `sampled` flag set.
    #[must_use]
    pub const fn sampled() -> Self {
        Self(Self::SAMPLED)
    }

    /// Create flags with no bits set (not sampled).
    #[must_use]
    pub const fn not_sampled() -> Self {
        Self(0x00)
    }

    /// Returns `true` when the `sampled` flag is set.
    #[must_use]
    pub const fn is_sampled(&self) -> bool {
        self.0 & Self::SAMPLED != 0
    }

    /// Return the raw flags byte.
    #[must_use]
    pub const fn as_byte(&self) -> u8 {
        self.0
    }
}

impl fmt::Display for SamplingFlags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02x}", self.0)
    }
}

// ---------------------------------------------------------------------------
// TraceContext
// ---------------------------------------------------------------------------

/// A parsed W3C `traceparent` header value.
///
/// Holds a [`TraceId`], a [`SpanId`], and [`SamplingFlags`]. Only spec version
/// `00` is accepted; future versions with extra fields will be rejected.
///
/// # Parsing
///
/// ```rust
/// use api_bones::traceparent::{TraceContext, SamplingFlags};
///
/// let tc: TraceContext =
///     "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
///         .parse()
///         .unwrap();
///
/// assert!(tc.flags.is_sampled());
/// ```
///
/// # Serialization
///
/// `Display` produces the canonical `traceparent` string which can be used
/// directly as an HTTP header value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct TraceContext {
    /// 128-bit trace identifier.
    pub trace_id: TraceId,
    /// 64-bit parent span identifier.
    pub span_id: SpanId,
    /// Sampling and other flags.
    pub flags: SamplingFlags,
}

impl TraceContext {
    /// Create a new `TraceContext` with fresh random IDs and the `sampled` flag
    /// set.
    ///
    /// ```rust
    /// use api_bones::traceparent::TraceContext;
    ///
    /// let tc = TraceContext::new();
    /// assert!(tc.flags.is_sampled());
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            trace_id: TraceId::new(),
            span_id: SpanId::new(),
            flags: SamplingFlags::sampled(),
        }
    }

    /// Create a new child span — same `trace_id`, new `span_id`.
    ///
    /// ```rust
    /// use api_bones::traceparent::TraceContext;
    ///
    /// let parent = TraceContext::new();
    /// let child = parent.child_span();
    /// assert_eq!(child.trace_id, parent.trace_id);
    /// assert_ne!(child.span_id, parent.span_id);
    /// ```
    #[must_use]
    pub fn child_span(&self) -> Self {
        Self {
            trace_id: self.trace_id,
            span_id: SpanId::new(),
            flags: self.flags,
        }
    }

    /// Produce the canonical `traceparent` header value string.
    ///
    /// Equivalent to `self.to_string()`.
    #[must_use]
    pub fn header_value(&self) -> String {
        self.to_string()
    }
}

impl Default for TraceContext {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for TraceContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "00-{}-{}-{}", self.trace_id, self.span_id, self.flags)
    }
}

impl FromStr for TraceContext {
    type Err = TraceContextError;

    /// Parse a `traceparent` header value.
    ///
    /// Only version `00` is accepted. Extra fields beyond the four standard
    /// ones are rejected per spec (future-version compatibility is the
    /// caller's responsibility).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: [&str; 4] = {
            let mut iter = s.splitn(5, '-');
            let version = iter.next().ok_or(TraceContextError::InvalidFormat)?;
            let trace_id = iter.next().ok_or(TraceContextError::InvalidFormat)?;
            let span_id = iter.next().ok_or(TraceContextError::InvalidFormat)?;
            let flags = iter.next().ok_or(TraceContextError::InvalidFormat)?;
            // For version 00 there must be no further fields.
            if version == "00" && iter.next().is_some() {
                return Err(TraceContextError::InvalidFormat);
            }
            [version, trace_id, span_id, flags]
        };

        if parts[0] != "00" {
            return Err(TraceContextError::UnsupportedVersion);
        }

        let trace_id: TraceId = parts[1].parse()?;
        let span_id: SpanId = parts[2].parse()?;

        if parts[3].len() != 2 {
            return Err(TraceContextError::InvalidFormat);
        }
        let flags_byte =
            u8::from_str_radix(parts[3], 16).map_err(|_| TraceContextError::InvalidFormat)?;
        let flags = SamplingFlags::from_byte(flags_byte);

        Ok(Self {
            trace_id,
            span_id,
            flags,
        })
    }
}

#[cfg(feature = "serde")]
impl<'de> Deserialize<'de> for TraceContext {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";

    // --- TraceId ---

    #[test]
    fn trace_id_new_not_zero() {
        assert!(!TraceId::new().is_zero());
    }

    #[test]
    fn trace_id_display_is_32_hex() {
        let id = TraceId::new();
        let s = id.to_string();
        assert_eq!(s.len(), 32);
        assert!(s.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn trace_id_parse_roundtrip() {
        let id = TraceId::new();
        let back: TraceId = id.to_string().parse().unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn trace_id_parse_rejects_all_zeros() {
        let err = "00000000000000000000000000000000"
            .parse::<TraceId>()
            .unwrap_err();
        assert_eq!(err, TraceContextError::ZeroTraceId);
    }

    #[test]
    fn trace_id_parse_rejects_wrong_length() {
        assert!("abc".parse::<TraceId>().is_err());
    }

    #[test]
    fn trace_id_from_bytes_rejects_zeros() {
        assert!(TraceId::from_bytes([0u8; 16]).is_none());
    }

    // --- SpanId ---

    #[test]
    fn span_id_new_not_zero() {
        assert!(!SpanId::new().is_zero());
    }

    #[test]
    fn span_id_display_is_16_hex() {
        let id = SpanId::new();
        let s = id.to_string();
        assert_eq!(s.len(), 16);
        assert!(s.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn span_id_parse_roundtrip() {
        let id = SpanId::new();
        let back: SpanId = id.to_string().parse().unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn span_id_parse_rejects_all_zeros() {
        let err = "0000000000000000".parse::<SpanId>().unwrap_err();
        assert_eq!(err, TraceContextError::ZeroSpanId);
    }

    // --- SamplingFlags ---

    #[test]
    fn sampling_flags_sampled() {
        let f = SamplingFlags::sampled();
        assert!(f.is_sampled());
        assert_eq!(f.to_string(), "01");
    }

    #[test]
    fn sampling_flags_not_sampled() {
        let f = SamplingFlags::not_sampled();
        assert!(!f.is_sampled());
        assert_eq!(f.to_string(), "00");
    }

    #[test]
    fn sampling_flags_from_byte() {
        assert!(SamplingFlags::from_byte(0x01).is_sampled());
        assert!(SamplingFlags::from_byte(0x03).is_sampled()); // other bits set too
        assert!(!SamplingFlags::from_byte(0x02).is_sampled());
    }

    // --- TraceContext ---

    #[test]
    fn parse_sample_traceparent() {
        let tc: TraceContext = SAMPLE.parse().unwrap();
        assert!(tc.flags.is_sampled());
        assert_eq!(tc.to_string(), SAMPLE);
    }

    #[test]
    fn trace_context_roundtrip() {
        let tc = TraceContext::new();
        let back: TraceContext = tc.to_string().parse().unwrap();
        assert_eq!(tc, back);
    }

    #[test]
    fn trace_context_child_span_same_trace() {
        let parent = TraceContext::new();
        let child = parent.child_span();
        assert_eq!(child.trace_id, parent.trace_id);
        assert_ne!(child.span_id, parent.span_id);
        assert_eq!(child.flags, parent.flags);
    }

    #[test]
    fn parse_not_sampled() {
        let s = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-00";
        let tc: TraceContext = s.parse().unwrap();
        assert!(!tc.flags.is_sampled());
    }

    #[test]
    fn parse_rejects_unsupported_version() {
        let err = "01-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
            .parse::<TraceContext>()
            .unwrap_err();
        assert_eq!(err, TraceContextError::UnsupportedVersion);
    }

    #[test]
    fn parse_rejects_too_few_fields() {
        assert!(
            "00-4bf92f3577b34da6a3ce929d0e0e4736"
                .parse::<TraceContext>()
                .is_err()
        );
    }

    #[test]
    fn parse_rejects_extra_fields_for_version_00() {
        let s = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01-extra";
        assert!(s.parse::<TraceContext>().is_err());
    }

    #[test]
    fn parse_rejects_zero_trace_id() {
        let s = "00-00000000000000000000000000000000-00f067aa0ba902b7-01";
        assert!(s.parse::<TraceContext>().is_err());
    }

    #[test]
    fn parse_rejects_zero_span_id() {
        let s = "00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01";
        assert!(s.parse::<TraceContext>().is_err());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn trace_context_serde_roundtrip() {
        let tc: TraceContext = SAMPLE.parse().unwrap();
        let json = serde_json::to_string(&tc).unwrap();
        let back: TraceContext = serde_json::from_str(&json).unwrap();
        assert_eq!(tc, back);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn trace_id_serde_roundtrip() {
        let id = TraceId::new();
        let json = serde_json::to_string(&id).unwrap();
        let back: TraceId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }
}
