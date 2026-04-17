//! Bulk operation envelope types for batch API endpoints.
//!
//! [`BulkRequest<T>`] wraps a collection of items for a batch operation.
//! [`BulkResponse<T>`] holds per-item [`BulkItemResult<T>`] variants so callers
//! can inspect which items succeeded and which failed without unwrapping a
//! single top-level error.
//!
//! ```rust
//! use api_bones::bulk::{BulkRequest, BulkResponse, BulkItemResult};
//! use api_bones::ApiError;
//!
//! let request: BulkRequest<i32> = BulkRequest { items: vec![1, 2, 3] };
//! assert_eq!(request.items.len(), 3);
//!
//! let results: Vec<BulkItemResult<String>> = vec![
//!     BulkItemResult::Success { data: "ok".to_string() },
//!     BulkItemResult::Failure { index: 1, error: ApiError::not_found("item 2 not found") },
//! ];
//! let response: BulkResponse<String> = BulkResponse { results };
//! assert_eq!(response.succeeded_count(), 1);
//! assert_eq!(response.failed_count(), 1);
//! assert!(response.has_failures());
//! ```

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::error::ApiError;

// ---------------------------------------------------------------------------
// BulkRequest
// ---------------------------------------------------------------------------

/// A batch of items to be processed in a single API call.
///
/// # Examples
///
/// ```rust
/// use api_bones::bulk::BulkRequest;
///
/// let request: BulkRequest<i32> = BulkRequest { items: vec![1, 2, 3] };
/// assert_eq!(request.items.len(), 3);
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "proptest", derive(proptest_derive::Arbitrary))]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct BulkRequest<T> {
    /// The items to be processed.
    pub items: Vec<T>,
}

// ---------------------------------------------------------------------------
// BulkItemResult
// ---------------------------------------------------------------------------

/// The outcome of processing a single item in a [`BulkRequest`].
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(all(feature = "std", feature = "serde"), derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(all(feature = "std", feature = "serde"), serde(tag = "status", rename_all = "snake_case"))]
pub enum BulkItemResult<T> {
    /// The item was processed successfully.
    Success {
        /// The resulting data.
        data: T,
    },
    /// The item failed to process.
    Failure {
        /// Zero-based index of the item in the original [`BulkRequest::items`] slice.
        index: usize,
        /// The error describing why processing failed.
        error: ApiError,
    },
}

impl<T> BulkItemResult<T> {
    /// Returns `true` if this result is a [`BulkItemResult::Success`].
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::bulk::BulkItemResult;
    ///
    /// let result: BulkItemResult<i32> = BulkItemResult::Success { data: 42 };
    /// assert!(result.is_success());
    /// ```
    #[must_use]
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success { .. })
    }

    /// Returns `true` if this result is a [`BulkItemResult::Failure`].
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::bulk::BulkItemResult;
    /// use api_bones::ApiError;
    ///
    /// let result: BulkItemResult<i32> = BulkItemResult::Failure {
    ///     index: 0,
    ///     error: ApiError::not_found("missing"),
    /// };
    /// assert!(result.is_failure());
    /// ```
    #[must_use]
    pub fn is_failure(&self) -> bool {
        matches!(self, Self::Failure { .. })
    }
}

// ---------------------------------------------------------------------------
// BulkResponse
// ---------------------------------------------------------------------------

/// The response to a [`BulkRequest`], containing per-item results.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(all(feature = "std", feature = "serde"), derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct BulkResponse<T> {
    /// Per-item outcomes, in the same order as [`BulkRequest::items`].
    pub results: Vec<BulkItemResult<T>>,
}

impl<T> BulkResponse<T> {
    /// Returns the number of successfully processed items.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::bulk::{BulkResponse, BulkItemResult};
    ///
    /// let response: BulkResponse<i32> = BulkResponse {
    ///     results: vec![
    ///         BulkItemResult::Success { data: 1 },
    ///         BulkItemResult::Success { data: 2 },
    ///     ],
    /// };
    /// assert_eq!(response.succeeded_count(), 2);
    /// ```
    #[must_use]
    pub fn succeeded_count(&self) -> usize {
        self.results.iter().filter(|r| r.is_success()).count()
    }

    /// Returns the number of items that failed to process.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::bulk::{BulkResponse, BulkItemResult};
    /// use api_bones::ApiError;
    ///
    /// let response: BulkResponse<i32> = BulkResponse {
    ///     results: vec![
    ///         BulkItemResult::Failure { index: 0, error: ApiError::not_found("gone") },
    ///     ],
    /// };
    /// assert_eq!(response.failed_count(), 1);
    /// ```
    #[must_use]
    pub fn failed_count(&self) -> usize {
        self.results.iter().filter(|r| r.is_failure()).count()
    }

    /// Returns `true` if at least one item failed.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::bulk::{BulkResponse, BulkItemResult};
    /// use api_bones::ApiError;
    ///
    /// let response: BulkResponse<i32> = BulkResponse {
    ///     results: vec![
    ///         BulkItemResult::Success { data: 1 },
    ///         BulkItemResult::Failure { index: 1, error: ApiError::not_found("nope") },
    ///     ],
    /// };
    /// assert!(response.has_failures());
    /// ```
    #[must_use]
    pub fn has_failures(&self) -> bool {
        self.results.iter().any(BulkItemResult::is_failure)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{ApiError, ErrorCode};

    fn make_error() -> ApiError {
        ApiError::not_found("item not found")
    }

    // -----------------------------------------------------------------------
    // BulkRequest
    // -----------------------------------------------------------------------

    #[test]
    fn bulk_request_construction() {
        let req: BulkRequest<i32> = BulkRequest {
            items: vec![1, 2, 3],
        };
        assert_eq!(req.items, vec![1, 2, 3]);
    }

    #[test]
    fn bulk_request_empty() {
        let req: BulkRequest<String> = BulkRequest { items: vec![] };
        assert!(req.items.is_empty());
    }

    // -----------------------------------------------------------------------
    // BulkItemResult
    // -----------------------------------------------------------------------

    #[test]
    fn bulk_item_result_success_is_success() {
        let r: BulkItemResult<i32> = BulkItemResult::Success { data: 42 };
        assert!(r.is_success());
        assert!(!r.is_failure());
    }

    #[test]
    fn bulk_item_result_failure_is_failure() {
        let r: BulkItemResult<i32> = BulkItemResult::Failure {
            index: 0,
            error: make_error(),
        };
        assert!(r.is_failure());
        assert!(!r.is_success());
    }

    // -----------------------------------------------------------------------
    // BulkResponse summary methods
    // -----------------------------------------------------------------------

    #[test]
    fn bulk_response_all_success() {
        let response: BulkResponse<i32> = BulkResponse {
            results: vec![
                BulkItemResult::Success { data: 1 },
                BulkItemResult::Success { data: 2 },
            ],
        };
        assert_eq!(response.succeeded_count(), 2);
        assert_eq!(response.failed_count(), 0);
        assert!(!response.has_failures());
    }

    #[test]
    fn bulk_response_all_failure() {
        let response: BulkResponse<i32> = BulkResponse {
            results: vec![
                BulkItemResult::Failure {
                    index: 0,
                    error: make_error(),
                },
                BulkItemResult::Failure {
                    index: 1,
                    error: make_error(),
                },
            ],
        };
        assert_eq!(response.succeeded_count(), 0);
        assert_eq!(response.failed_count(), 2);
        assert!(response.has_failures());
    }

    #[test]
    fn bulk_response_mixed() {
        let response: BulkResponse<String> = BulkResponse {
            results: vec![
                BulkItemResult::Success {
                    data: "ok".to_string(),
                },
                BulkItemResult::Failure {
                    index: 1,
                    error: make_error(),
                },
                BulkItemResult::Success {
                    data: "also ok".to_string(),
                },
            ],
        };
        assert_eq!(response.succeeded_count(), 2);
        assert_eq!(response.failed_count(), 1);
        assert!(response.has_failures());
    }

    #[test]
    fn bulk_response_empty() {
        let response: BulkResponse<i32> = BulkResponse { results: vec![] };
        assert_eq!(response.succeeded_count(), 0);
        assert_eq!(response.failed_count(), 0);
        assert!(!response.has_failures());
    }

    // -----------------------------------------------------------------------
    // Serde round-trips
    // -----------------------------------------------------------------------

    #[cfg(feature = "serde")]
    #[test]
    fn bulk_request_serde_round_trip() {
        let req: BulkRequest<i32> = BulkRequest {
            items: vec![10, 20, 30],
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["items"], serde_json::json!([10, 20, 30]));
        let back: BulkRequest<i32> = serde_json::from_value(json).unwrap();
        assert_eq!(back, req);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn bulk_item_result_success_serde_round_trip() {
        let r: BulkItemResult<String> = BulkItemResult::Success {
            data: "hello".to_string(),
        };
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["status"], "success");
        assert_eq!(json["data"], "hello");
        let back: BulkItemResult<String> = serde_json::from_value(json).unwrap();
        assert_eq!(back, r);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn bulk_item_result_failure_serde_round_trip() {
        let r: BulkItemResult<i32> = BulkItemResult::Failure {
            index: 3,
            error: make_error(),
        };
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["status"], "failure");
        assert_eq!(json["index"], 3);
        let back: BulkItemResult<i32> = serde_json::from_value(json).unwrap();
        assert_eq!(back, r);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn bulk_response_serde_round_trip_mixed() {
        let response: BulkResponse<String> = BulkResponse {
            results: vec![
                BulkItemResult::Success {
                    data: "ok".to_string(),
                },
                BulkItemResult::Failure {
                    index: 1,
                    error: make_error(),
                },
            ],
        };
        let json = serde_json::to_value(&response).unwrap();
        let back: BulkResponse<String> = serde_json::from_value(json).unwrap();
        assert_eq!(back, response);
    }

    // -----------------------------------------------------------------------
    // ErrorCode composition check
    // -----------------------------------------------------------------------

    #[test]
    fn bulk_item_result_failure_uses_api_error() {
        let error = ApiError::new(ErrorCode::ValidationFailed, "bad input");
        let r: BulkItemResult<()> = BulkItemResult::Failure { index: 0, error };
        if let BulkItemResult::Failure { error, .. } = &r {
            assert_eq!(error.code, ErrorCode::ValidationFailed);
        } else {
            panic!("expected Failure");
        }
    }
}
