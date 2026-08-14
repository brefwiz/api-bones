// SPDX-License-Identifier: MIT
use connectrpc::ConnectError;

/// Canonical error-kind shape for domain errors that need to cross the Connect RPC boundary.
///
/// Services implementing `IntoDomainErrorKind` on their own error types gain a
/// zero-boilerplate `domain_to_connect` mapper — no per-service re-implementation
/// of the `DomainError → ConnectError` translation table (ADR-0096).
///
/// # Variants
///
/// - `NotFound` — resource absent; maps to `ConnectError::not_found`.
/// - `Conflict(String)` — state conflict (e.g. duplicate key); maps to
///   `ConnectError::already_exists`.
/// - `Internal(String)` — unexpected error; logged via `tracing::error!` then
///   mapped to `ConnectError::internal` with a generic message (no internal
///   detail surfaced to callers).
#[derive(Debug)]
pub enum DomainErrorKind {
    NotFound,
    Conflict(String),
    Internal(String),
}

/// Implement this trait on your service's domain error type to unlock the
/// generic [`domain_to_connect`] mapper.
///
/// # Example
///
/// ```rust
/// use api_bones::connect::{DomainErrorKind, IntoDomainErrorKind};
///
/// enum MyError {
///     NotFound,
///     AlreadyExists(String),
///     Unexpected(String),
/// }
///
/// impl IntoDomainErrorKind for MyError {
///     fn kind(&self) -> DomainErrorKind {
///         match self {
///             Self::NotFound => DomainErrorKind::NotFound,
///             Self::AlreadyExists(msg) => DomainErrorKind::Conflict(msg.clone()),
///             Self::Unexpected(msg) => DomainErrorKind::Internal(msg.clone()),
///         }
///     }
/// }
/// ```
pub trait IntoDomainErrorKind {
    fn kind(&self) -> DomainErrorKind;
}

/// Map any domain error implementing [`IntoDomainErrorKind`] to a [`ConnectError`].
///
/// Variant mapping:
///
/// | `DomainErrorKind`  | `ConnectError`             |
/// |--------------------|----------------------------|
/// | `NotFound`         | `not_found("not found")`   |
/// | `Conflict(msg)`    | `already_exists(msg)`      |
/// | `Internal(detail)` | `internal("internal error")` — detail logged, not surfaced |
///
/// Internal errors are logged at `ERROR` level before the `ConnectError` is
/// returned. The detail string is never forwarded to callers.
///
/// # Example
///
/// ```rust
/// use api_bones::connect::{DomainErrorKind, IntoDomainErrorKind, domain_to_connect};
/// use connectrpc::ConnectError;
///
/// struct NotFoundErr;
/// impl IntoDomainErrorKind for NotFoundErr {
///     fn kind(&self) -> DomainErrorKind { DomainErrorKind::NotFound }
/// }
///
/// let err: ConnectError = domain_to_connect(&NotFoundErr);
/// ```
pub fn domain_to_connect<E: IntoDomainErrorKind>(err: &E) -> ConnectError {
    match err.kind() {
        DomainErrorKind::NotFound => ConnectError::not_found("not found"),
        DomainErrorKind::Conflict(msg) => ConnectError::already_exists(msg),
        DomainErrorKind::Internal(detail) => {
            tracing::error!(error = %detail, "internal domain error");
            ConnectError::internal("internal error")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestErr(DomainErrorKind);

    impl IntoDomainErrorKind for TestErr {
        fn kind(&self) -> DomainErrorKind {
            match &self.0 {
                DomainErrorKind::NotFound => DomainErrorKind::NotFound,
                DomainErrorKind::Conflict(s) => DomainErrorKind::Conflict(s.clone()),
                DomainErrorKind::Internal(s) => DomainErrorKind::Internal(s.clone()),
            }
        }
    }

    fn debug_str(e: &ConnectError) -> String {
        format!("{e:?}")
    }

    #[test]
    fn not_found_maps_to_not_found() {
        let err = domain_to_connect(&TestErr(DomainErrorKind::NotFound));
        let s = debug_str(&err);
        assert!(
            s.contains("not_found") || s.contains("NotFound"),
            "expected not_found code, got: {s}"
        );
    }

    #[test]
    fn conflict_maps_to_already_exists() {
        let err = domain_to_connect(&TestErr(DomainErrorKind::Conflict("dup key".into())));
        let s = debug_str(&err);
        assert!(
            s.contains("already_exists") || s.contains("AlreadyExists"),
            "expected already_exists code, got: {s}"
        );
    }

    #[test]
    fn internal_maps_to_internal_without_detail() {
        let err = domain_to_connect(&TestErr(DomainErrorKind::Internal("secret detail".into())));
        let s = debug_str(&err);
        assert!(
            s.contains("internal") || s.contains("Internal"),
            "expected internal code, got: {s}"
        );
        assert!(
            !s.contains("secret detail"),
            "internal detail must not be surfaced to caller"
        );
    }
}
