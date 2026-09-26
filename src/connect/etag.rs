// SPDX-License-Identifier: MIT
// ETag / If-Match helpers for Connect RPC adapters.

use chrono::{DateTime, Utc};
use connectrpc::{ConnectError, ErrorCode, RequestContext};

use crate::etag::ETag;

/// Derive a weak [`ETag`] from an `updated_at` timestamp.
///
/// The value is the Unix timestamp in milliseconds, hex-encoded — the same
/// derivation `service_kit::etag::etag_from_updated_at` uses on the HTTP
/// side, so a record's `ETag` is identical whichever transport served it.
#[must_use]
pub fn etag_from_updated_at(updated_at: DateTime<Utc>) -> ETag {
    let millis = updated_at.timestamp_millis();
    ETag::weak(format!("{millis:x}"))
}

/// Enforce the `If-Match` precondition on a Connect request context.
///
/// - Header absent → [`ErrorCode::FailedPrecondition`] — the caller must
///   supply a precondition before retrying.
/// - `"*"` → `Ok(())` (unconditional write).
/// - Header malformed (not a valid `ETag` or `ETag` list) →
///   [`ErrorCode::InvalidArgument`].
/// - Header well-formed but no listed tag weakly matches `current_etag` →
///   [`ErrorCode::Aborted`] — the platform's code for a failed
///   compare-and-swap (gRPC/Google API design guidance reserves
///   `FAILED_PRECONDITION` for a state the caller can't just retry past, and
///   `ABORTED` for a concurrency conflict the caller retries at a higher
///   level).
///
/// Comparison is weak per RFC 9110 §8.8.3.2: both client-supplied and
/// server-derived `ETag`s may be weak (timestamp-based), so a strong request
/// tag with the same value as a weak current one still matches.
///
/// # Errors
///
/// Returns a [`ConnectError`] with `failed_precondition`, `invalid_argument`,
/// or `aborted` as described above.
pub fn check_if_match(ctx: &RequestContext, current_etag: &ETag) -> Result<(), ConnectError> {
    let Some(raw) = ctx.header("if-match").and_then(|v| v.to_str().ok()) else {
        return Err(ConnectError::new(
            ErrorCode::FailedPrecondition,
            "If-Match header required",
        ));
    };

    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ConnectError::new(
            ErrorCode::FailedPrecondition,
            "If-Match header required",
        ));
    }

    let matched = if trimmed == "*" {
        true
    } else {
        let tags = ETag::parse_list(trimmed).map_err(|e| {
            ConnectError::new(
                ErrorCode::InvalidArgument,
                format!("If-Match header is malformed: {e}"),
            )
        })?;
        tags.iter().any(|t| t.matches_weak(current_etag))
    };

    if matched {
        Ok(())
    } else {
        Err(ConnectError::new(
            ErrorCode::Aborted,
            "ETag does not match; the resource has been modified",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone as _;
    use connectrpc::ErrorCode;
    use http::HeaderMap;

    fn ctx_with_if_match(value: &str) -> RequestContext {
        let mut headers = HeaderMap::new();
        headers.insert("if-match", value.parse().unwrap());
        RequestContext::new(headers)
    }

    fn ctx_empty() -> RequestContext {
        RequestContext::new(HeaderMap::new())
    }

    #[test]
    fn etag_is_weak_and_hex_encoded() {
        let ts = Utc.timestamp_millis_opt(1_700_000_000_000).unwrap();
        let tag = etag_from_updated_at(ts);
        assert!(tag.weak);
        assert_eq!(tag.to_string(), format!("W/\"{:x}\"", 1_700_000_000_000i64));
    }

    #[test]
    fn check_if_match_absent_header() {
        let current = etag_from_updated_at(Utc.timestamp_millis_opt(1_700_000_000_000).unwrap());
        let err = check_if_match(&ctx_empty(), &current).unwrap_err();
        assert_eq!(err.code, ErrorCode::FailedPrecondition);
    }

    #[test]
    fn check_if_match_empty_header() {
        let current = etag_from_updated_at(Utc.timestamp_millis_opt(1_700_000_000_000).unwrap());
        let err = check_if_match(&ctx_with_if_match(""), &current).unwrap_err();
        assert_eq!(err.code, ErrorCode::FailedPrecondition);
    }

    #[test]
    fn check_if_match_wildcard() {
        let current = etag_from_updated_at(Utc.timestamp_millis_opt(1_700_000_000_000).unwrap());
        check_if_match(&ctx_with_if_match("*"), &current).unwrap();
    }

    #[test]
    fn check_if_match_exact_match() {
        let current = etag_from_updated_at(Utc.timestamp_millis_opt(1_700_000_000_000).unwrap());
        check_if_match(&ctx_with_if_match(&current.to_string()), &current).unwrap();
    }

    #[test]
    fn check_if_match_matches_weakly_across_strength() {
        let current = etag_from_updated_at(Utc.timestamp_millis_opt(1_700_000_000_000).unwrap());
        // A strong-quoted client tag with the same value still matches a
        // weak (timestamp-derived) current ETag under weak comparison.
        let strong = format!("\"{}\"", current.value);
        check_if_match(&ctx_with_if_match(&strong), &current).unwrap();
    }

    #[test]
    fn check_if_match_mismatch() {
        let current = etag_from_updated_at(Utc.timestamp_millis_opt(1_700_000_000_000).unwrap());
        let other = etag_from_updated_at(Utc.timestamp_millis_opt(1_700_000_001_000).unwrap());
        let err = check_if_match(&ctx_with_if_match(&other.to_string()), &current).unwrap_err();
        assert_eq!(err.code, ErrorCode::Aborted);
    }

    #[test]
    fn check_if_match_malformed_header() {
        let current = etag_from_updated_at(Utc.timestamp_millis_opt(1_700_000_000_000).unwrap());
        let err = check_if_match(&ctx_with_if_match("not-a-valid-etag"), &current).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidArgument);
    }
}
