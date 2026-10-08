// SPDX-License-Identifier: MIT
//! Canonical wire-level Connect error code.
//!
//! Connect maps several codes onto the same HTTP status (for example both
//! `failed_precondition` and `invalid_argument` are HTTP 400), so callers that
//! need to classify a failure must read the code from the wire, not the
//! status. [`ConnectCode`] is that wire code, independent of the transport
//! crate's own error type.

use core::fmt;
use core::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A Connect protocol error code, identified by its snake_case wire string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ConnectCode {
    /// The operation was cancelled.
    Canceled,
    /// Unknown error.
    Unknown,
    /// The client supplied an invalid argument.
    InvalidArgument,
    /// The deadline expired before the operation completed.
    DeadlineExceeded,
    /// The requested entity was not found.
    NotFound,
    /// The entity already exists.
    AlreadyExists,
    /// The caller lacks permission.
    PermissionDenied,
    /// A resource (for example a quota) is exhausted.
    ResourceExhausted,
    /// The system is not in a state required for the operation.
    FailedPrecondition,
    /// The operation was aborted.
    Aborted,
    /// The operation was attempted past the valid range.
    OutOfRange,
    /// The operation is not implemented.
    Unimplemented,
    /// Internal error.
    Internal,
    /// The service is unavailable.
    Unavailable,
    /// Unrecoverable data loss or corruption.
    DataLoss,
    /// The request lacks valid authentication.
    Unauthenticated,
}

impl ConnectCode {
    /// Every code, in protocol order.
    const ALL: [Self; 16] = [
        Self::Canceled,
        Self::Unknown,
        Self::InvalidArgument,
        Self::DeadlineExceeded,
        Self::NotFound,
        Self::AlreadyExists,
        Self::PermissionDenied,
        Self::ResourceExhausted,
        Self::FailedPrecondition,
        Self::Aborted,
        Self::OutOfRange,
        Self::Unimplemented,
        Self::Internal,
        Self::Unavailable,
        Self::DataLoss,
        Self::Unauthenticated,
    ];

    /// The snake_case string this code carries on the wire.
    #[must_use]
    pub const fn as_wire(&self) -> &'static str {
        match self {
            Self::Canceled => "canceled",
            Self::Unknown => "unknown",
            Self::InvalidArgument => "invalid_argument",
            Self::DeadlineExceeded => "deadline_exceeded",
            Self::NotFound => "not_found",
            Self::AlreadyExists => "already_exists",
            Self::PermissionDenied => "permission_denied",
            Self::ResourceExhausted => "resource_exhausted",
            Self::FailedPrecondition => "failed_precondition",
            Self::Aborted => "aborted",
            Self::OutOfRange => "out_of_range",
            Self::Unimplemented => "unimplemented",
            Self::Internal => "internal",
            Self::Unavailable => "unavailable",
            Self::DataLoss => "data_loss",
            Self::Unauthenticated => "unauthenticated",
        }
    }

    /// Parse a wire string. Unrecognized strings yield `None`.
    #[must_use]
    pub fn from_wire(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.as_wire() == s)
    }

    /// Read the code from a Connect error JSON body
    /// (`{"code": "...", "message": "..."}`).
    ///
    /// Returns `None` when the body is not JSON, has no string `code`, or the
    /// code is not a recognized Connect code.
    #[must_use]
    pub fn from_error_body(body: &[u8]) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_slice(body).ok()?;
        Self::from_wire(value.get("code")?.as_str()?)
    }
}

impl fmt::Display for ConnectCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_wire())
    }
}

/// Error returned when a string is not a recognized Connect wire code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseConnectCodeError(String);

impl fmt::Display for ParseConnectCodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unrecognized connect code `{}`", self.0)
    }
}

impl std::error::Error for ParseConnectCodeError {}

impl FromStr for ConnectCode {
    type Err = ParseConnectCodeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_wire(s).ok_or_else(|| ParseConnectCodeError(s.to_owned()))
    }
}

impl Serialize for ConnectCode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_wire())
    }
}

impl<'de> Deserialize<'de> for ConnectCode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = <std::borrow::Cow<'de, str>>::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

impl From<connectrpc::ErrorCode> for ConnectCode {
    /// Codes the transport crate adds beyond the Connect protocol set map to
    /// [`ConnectCode::Unknown`].
    fn from(code: connectrpc::ErrorCode) -> Self {
        use connectrpc::ErrorCode as E;
        match code {
            E::Canceled => Self::Canceled,
            E::Unknown => Self::Unknown,
            E::InvalidArgument => Self::InvalidArgument,
            E::DeadlineExceeded => Self::DeadlineExceeded,
            E::NotFound => Self::NotFound,
            E::AlreadyExists => Self::AlreadyExists,
            E::PermissionDenied => Self::PermissionDenied,
            E::ResourceExhausted => Self::ResourceExhausted,
            E::FailedPrecondition => Self::FailedPrecondition,
            E::Aborted => Self::Aborted,
            E::OutOfRange => Self::OutOfRange,
            E::Unimplemented => Self::Unimplemented,
            E::Internal => Self::Internal,
            E::Unavailable => Self::Unavailable,
            E::DataLoss => Self::DataLoss,
            E::Unauthenticated => Self::Unauthenticated,
            _ => Self::Unknown,
        }
    }
}

impl From<ConnectCode> for connectrpc::ErrorCode {
    fn from(code: ConnectCode) -> Self {
        use connectrpc::ErrorCode as E;
        match code {
            ConnectCode::Canceled => E::Canceled,
            ConnectCode::Unknown => E::Unknown,
            ConnectCode::InvalidArgument => E::InvalidArgument,
            ConnectCode::DeadlineExceeded => E::DeadlineExceeded,
            ConnectCode::NotFound => E::NotFound,
            ConnectCode::AlreadyExists => E::AlreadyExists,
            ConnectCode::PermissionDenied => E::PermissionDenied,
            ConnectCode::ResourceExhausted => E::ResourceExhausted,
            ConnectCode::FailedPrecondition => E::FailedPrecondition,
            ConnectCode::Aborted => E::Aborted,
            ConnectCode::OutOfRange => E::OutOfRange,
            ConnectCode::Unimplemented => E::Unimplemented,
            ConnectCode::Internal => E::Internal,
            ConnectCode::Unavailable => E::Unavailable,
            ConnectCode::DataLoss => E::DataLoss,
            ConnectCode::Unauthenticated => E::Unauthenticated,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_variant_round_trips_wire_string() {
        for code in ConnectCode::ALL {
            assert_eq!(ConnectCode::from_wire(code.as_wire()), Some(code));
            assert_eq!(code.to_string(), code.as_wire());
            assert_eq!(code.as_wire().parse::<ConnectCode>(), Ok(code));
        }
    }

    #[test]
    fn wire_strings_are_distinct_and_match_transport_crate() {
        for code in ConnectCode::ALL {
            let transport: connectrpc::ErrorCode = code.into();
            assert_eq!(transport.as_str(), code.as_wire());
            assert_eq!(ConnectCode::from(transport), code);
        }
        let mut wires: Vec<_> = ConnectCode::ALL.iter().map(ConnectCode::as_wire).collect();
        wires.sort_unstable();
        wires.dedup();
        assert_eq!(wires.len(), 16);
    }

    #[test]
    fn failed_precondition_is_distinct_from_invalid_argument() {
        assert_eq!(
            ConnectCode::from_wire("failed_precondition"),
            Some(ConnectCode::FailedPrecondition)
        );
        assert_ne!(
            ConnectCode::FailedPrecondition,
            ConnectCode::InvalidArgument
        );
    }

    #[test]
    fn unrecognized_wire_string_is_none_and_typed_error() {
        assert_eq!(ConnectCode::from_wire("bogus"), None);
        assert_eq!(ConnectCode::from_wire("FAILED_PRECONDITION"), None);
        assert_eq!(ConnectCode::from_wire(""), None);
        let err = "bogus".parse::<ConnectCode>().unwrap_err();
        assert_eq!(err.to_string(), "unrecognized connect code `bogus`");
    }

    #[test]
    fn serde_uses_wire_string() {
        let json = serde_json::to_string(&ConnectCode::FailedPrecondition).unwrap();
        assert_eq!(json, "\"failed_precondition\"");
        let back: ConnectCode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ConnectCode::FailedPrecondition);
        assert!(serde_json::from_str::<ConnectCode>("\"nope\"").is_err());
    }

    #[test]
    fn error_body_with_code() {
        let body = br#"{"code":"failed_precondition","message":"stale"}"#;
        assert_eq!(
            ConnectCode::from_error_body(body),
            Some(ConnectCode::FailedPrecondition)
        );
    }

    #[test]
    fn error_body_without_code() {
        assert_eq!(ConnectCode::from_error_body(br#"{"message":"x"}"#), None);
        assert_eq!(ConnectCode::from_error_body(br#"{"code":7}"#), None);
        assert_eq!(ConnectCode::from_error_body(b"[]"), None);
    }

    #[test]
    fn error_body_malformed() {
        assert_eq!(ConnectCode::from_error_body(b"not json"), None);
        assert_eq!(ConnectCode::from_error_body(b""), None);
    }

    #[test]
    fn error_body_unknown_code() {
        assert_eq!(
            ConnectCode::from_error_body(br#"{"code":"bogus","message":"x"}"#),
            None
        );
    }
}
