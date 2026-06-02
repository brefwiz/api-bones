// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
// ETag / If-Match helpers for Connect RPC adapters.

use chrono::{DateTime, Utc};
use connectrpc::{ConnectError, ErrorCode, RequestContext};

/// Generate a weak `ETag` from an `updated_at` timestamp.
/// Format: `W/"<unix_ms>"` — mirrors `service_kit::etag::etag_from_updated_at`.
#[must_use]
pub fn etag_from_updated_at(updated_at: DateTime<Utc>) -> String {
    format!("W/\"{}\"", updated_at.timestamp_millis())
}

/// Enforce the `If-Match` precondition on a Connect request context.
/// - Header absent or empty → `FailedPrecondition` ("If-Match header required")
/// - `"*"` → Ok(()) (unconditional write)
/// - value matches `current_etag` → Ok(())
/// - mismatch → Aborted ("`ETag` mismatch")
pub fn check_if_match(ctx: &RequestContext, current_etag: &str) -> Result<(), ConnectError> {
    let header = ctx
        .header("if-match")
        .and_then(|v| v.to_str().ok())
        .map(str::trim);
    match header {
        None | Some("") => Err(ConnectError::new(
            ErrorCode::FailedPrecondition,
            "If-Match header required",
        )),
        Some("*") => Ok(()),
        Some(v) if v == current_etag => Ok(()),
        Some(_) => Err(ConnectError::new(ErrorCode::Aborted, "ETag mismatch")),
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
    fn etag_format() {
        let ts = Utc.timestamp_millis_opt(1_700_000_000_000).unwrap();
        assert_eq!(etag_from_updated_at(ts), r#"W/"1700000000000""#);
    }

    #[test]
    fn check_if_match_absent_header() {
        let err = check_if_match(&ctx_empty(), r#"W/"123""#).unwrap_err();
        assert_eq!(err.code, ErrorCode::FailedPrecondition);
    }

    #[test]
    fn check_if_match_empty_header() {
        let err = check_if_match(&ctx_with_if_match(""), r#"W/"123""#).unwrap_err();
        assert_eq!(err.code, ErrorCode::FailedPrecondition);
    }

    #[test]
    fn check_if_match_wildcard() {
        check_if_match(&ctx_with_if_match("*"), r#"W/"123""#).unwrap();
    }

    #[test]
    fn check_if_match_exact_match() {
        check_if_match(&ctx_with_if_match(r#"W/"123""#), r#"W/"123""#).unwrap();
    }

    #[test]
    fn check_if_match_mismatch() {
        let err = check_if_match(&ctx_with_if_match(r#"W/"999""#), r#"W/"123""#).unwrap_err();
        assert_eq!(err.code, ErrorCode::Aborted);
    }
}
