// SPDX-License-Identifier: MIT
use connectrpc::ConnectError;
use uuid::Uuid;

/// Parse a UUID string, returning [`ConnectError::invalid_argument`] on failure.
///
/// `field` names the request field being parsed and is included in the error
/// message to aid API callers in locating the problem (e.g. `"org_id"`).
///
/// # Example
///
/// ```rust
/// use api_bones::connect::parse_uuid;
///
/// let id = parse_uuid("550e8400-e29b-41d4-a716-446655440000", "org_id").unwrap();
/// assert_eq!(id.to_string(), "550e8400-e29b-41d4-a716-446655440000");
///
/// let err = parse_uuid("not-a-uuid", "org_id").unwrap_err();
/// // ConnectError message contains the field name
/// ```
pub fn parse_uuid(s: &str, field: &str) -> Result<Uuid, ConnectError> {
    Uuid::parse_str(s)
        .map_err(|_| ConnectError::invalid_argument(format!("invalid UUID in field `{field}`")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_uuid_parses() {
        let s = "550e8400-e29b-41d4-a716-446655440000";
        let id = parse_uuid(s, "id").unwrap();
        assert_eq!(id.to_string(), s);
    }

    #[test]
    fn nil_uuid_parses() {
        let id = parse_uuid("00000000-0000-0000-0000-000000000000", "id").unwrap();
        assert!(id.is_nil());
    }

    #[test]
    fn invalid_returns_error() {
        let err = parse_uuid("not-a-uuid", "org_id").unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains("org_id"));
    }

    #[test]
    fn empty_string_returns_error() {
        assert!(parse_uuid("", "id").is_err());
    }
}
