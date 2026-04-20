//! Shared [`HeaderId`] trait for newtype wrappers that are transported via a
//! dedicated HTTP header.
//!
//! All ID newtypes in this crate — [`crate::request_id::RequestId`],
//! [`crate::correlation_id::CorrelationId`],
//! [`crate::idempotency::IdempotencyKey`], and
//! [`crate::traceparent::TraceContext`] — follow the same pattern:
//!
//! - They carry a fixed, well-known header name.
//! - They expose their value as a string.
//!
//! `HeaderId` makes that pattern explicit and enables generic middleware or
//! helper utilities that work over any of these types.
//!
//! # Example
//!
//! ```rust
//! use api_bones::header_id::HeaderId;
//! use api_bones::request_id::RequestId;
//! use api_bones::correlation_id::CorrelationId;
//! use api_bones::idempotency::IdempotencyKey;
//! use api_bones::traceparent::TraceContext;
//!
//! fn header_name_of<T: HeaderId>() -> &'static str {
//!     T::HEADER_NAME
//! }
//!
//! assert_eq!(header_name_of::<RequestId>(), "X-Request-Id");
//! assert_eq!(header_name_of::<CorrelationId>(), "X-Correlation-Id");
//! assert_eq!(header_name_of::<IdempotencyKey>(), "Idempotency-Key");
//! assert_eq!(header_name_of::<TraceContext>(), "traceparent");
//! ```

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::borrow::Cow;
#[cfg(feature = "std")]
use std::borrow::Cow;

// ---------------------------------------------------------------------------
// HeaderId trait
// ---------------------------------------------------------------------------

/// A type that is transported as a single HTTP header value.
///
/// Implementors provide:
/// - [`HEADER_NAME`](HeaderId::HEADER_NAME) — the canonical header name
///   exactly as it appears in an HTTP request or response, e.g.
///   `"X-Request-Id"`.
/// - [`as_str`](HeaderId::as_str) — the string representation of the value,
///   borrowed when the value is already stored as a `String`, or owned when
///   derived on the fly (e.g. for UUID-backed or structured types).
///
/// # Generic middleware
///
/// The trait enables writing helpers that work uniformly over any header-ID
/// type without duplicating the header-lookup logic:
///
/// ```rust
/// use api_bones::header_id::HeaderId;
///
/// fn log_header<T: HeaderId>(value: &T) {
///     println!("{}: {}", T::HEADER_NAME, value.as_str());
/// }
/// ```
///
/// In axum middleware you can use `T::HEADER_NAME` to look up the right header
/// for any `T: HeaderId + FromStr`:
///
/// ```rust,ignore
/// fn extract_header<T>(parts: &axum::http::request::Parts)
///     -> Result<T, api_bones::ApiError>
/// where
///     T: HeaderId + core::str::FromStr,
///     T::Err: core::fmt::Display,
/// {
///     let raw = parts
///         .headers
///         .get(T::HEADER_NAME)
///         .ok_or_else(|| api_bones::ApiError::bad_request(
///             format!("missing required header: {}", T::HEADER_NAME)
///         ))?
///         .to_str()
///         .map_err(|_| api_bones::ApiError::bad_request(
///             format!("header {} contains non-UTF-8 bytes", T::HEADER_NAME)
///         ))?;
///     raw.parse::<T>()
///         .map_err(|e| api_bones::ApiError::bad_request(
///             format!("invalid {}: {e}", T::HEADER_NAME)
///         ))
/// }
/// ```
#[cfg(any(feature = "std", feature = "alloc"))]
pub trait HeaderId {
    /// The canonical HTTP header name for this type.
    ///
    /// Examples: `"X-Request-Id"`, `"X-Correlation-Id"`,
    /// `"Idempotency-Key"`, `"traceparent"`.
    const HEADER_NAME: &'static str;

    /// Return the string representation of this header value.
    ///
    /// Returns a [`Cow::Borrowed`] slice when the value is already stored as a
    /// `String`, and a [`Cow::Owned`] string when the representation must be
    /// computed (e.g. for UUID-backed or structured types).
    fn as_str(&self) -> Cow<'_, str>;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(all(test, feature = "uuid"))]
mod tests {
    use super::*;
    use crate::correlation_id::CorrelationId;
    use crate::idempotency::IdempotencyKey;
    use crate::request_id::RequestId;
    use crate::traceparent::TraceContext;

    #[test]
    fn header_names() {
        assert_eq!(RequestId::HEADER_NAME, "X-Request-Id");
        assert_eq!(CorrelationId::HEADER_NAME, "X-Correlation-Id");
        assert_eq!(IdempotencyKey::HEADER_NAME, "Idempotency-Key");
        assert_eq!(TraceContext::HEADER_NAME, "traceparent");
    }

    // Use a generic helper to force dispatch through the HeaderId trait,
    // bypassing any inherent as_str() methods that would otherwise shadow it.
    fn trait_as_str<T: HeaderId>(v: &T) -> Cow<'_, str> {
        v.as_str()
    }

    #[test]
    fn as_str_request_id() {
        let id = RequestId::from_uuid(uuid::Uuid::nil());
        assert_eq!(trait_as_str(&id), "00000000-0000-0000-0000-000000000000");
    }

    #[test]
    fn as_str_correlation_id() {
        let id = CorrelationId::new("corr-abc").unwrap();
        assert_eq!(trait_as_str(&id), "corr-abc");
    }

    #[test]
    fn as_str_idempotency_key() {
        let key = IdempotencyKey::new("my-key").unwrap();
        assert_eq!(trait_as_str(&key), "my-key");
    }

    #[test]
    fn as_str_trace_context() {
        let tc: TraceContext = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
            .parse()
            .unwrap();
        assert_eq!(
            trait_as_str(&tc),
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
        );
    }

    #[test]
    fn header_name_instance_methods() {
        let key = IdempotencyKey::new("x").unwrap();
        assert_eq!(key.header_name(), "Idempotency-Key");
        let tc = TraceContext::new();
        assert_eq!(tc.header_name(), "traceparent");
    }

    #[test]
    fn generic_header_name_fn() {
        fn header_name_of<T: HeaderId>() -> &'static str {
            T::HEADER_NAME
        }
        assert_eq!(header_name_of::<RequestId>(), "X-Request-Id");
        assert_eq!(header_name_of::<CorrelationId>(), "X-Correlation-Id");
        assert_eq!(header_name_of::<IdempotencyKey>(), "Idempotency-Key");
        assert_eq!(header_name_of::<TraceContext>(), "traceparent");
    }
}
