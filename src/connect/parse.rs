// SPDX-License-Identifier: MIT
// Parsing helpers for Connect RPC adapters.

use chrono::{DateTime, Utc};
use connectrpc::ConnectError;

/// Parse an RFC-3339 datetime string, returning `InvalidArgument` with field attribution on failure.
pub fn parse_rfc3339(s: &str, field: &str) -> Result<DateTime<Utc>, ConnectError> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|_| crate::connect::invalid_field(field))
}

#[cfg(test)]
mod tests {
    use super::*;
    use connectrpc::ErrorCode;

    #[test]
    fn parse_valid_rfc3339() {
        let dt = parse_rfc3339("2024-01-15T10:30:00Z", "start_at").unwrap();
        assert_eq!(dt.timestamp(), 1_705_314_600);
    }

    #[test]
    fn parse_invalid_returns_invalid_argument() {
        let err = parse_rfc3339("not-a-date", "start_at").unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn parse_with_offset() {
        let dt = parse_rfc3339("2024-01-15T10:30:00+02:00", "end_at").unwrap();
        assert_eq!(dt.timestamp(), 1_705_307_400);
    }
}
