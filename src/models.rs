//! Public API request and response types.
//!
//! These types mirror the platform `OpenAPI` schema and are the
//! canonical definitions shared between the server, SDKs, and consumers.
//! They carry no business logic — serialization only.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::string::String;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Error response
// ---------------------------------------------------------------------------

/// Standard error body returned by the API on failure.
///
/// # Examples
///
/// ```rust
/// use api_bones::models::ErrorResponse;
/// let err = ErrorResponse::new("oops");
/// assert_eq!(err.error, "oops");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "proptest", derive(proptest_derive::Arbitrary))]
pub struct ErrorResponse {
    /// Human-readable error description.
    pub error: String,
}

impl ErrorResponse {
    /// Create a new error response.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::models::ErrorResponse;
    /// let err = ErrorResponse::new("not found");
    /// assert_eq!(err.error, "not found");
    /// ```
    pub fn new(error: impl Into<String>) -> Self {
        Self {
            error: error.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_response_new() {
        let r = ErrorResponse::new("oops");
        assert_eq!(r.error, "oops");
    }
}
