// SPDX-License-Identifier: MIT
//
//! Which Connect failures may be replayed.
//!
//! [`crate::retry`] answers *how long to wait* between attempts. It does not
//! answer *whether a call may be attempted again at all*, and that half has
//! until now been left to each Rust SDK. Every consumer that wanted a retry
//! therefore wrote its own code list, which is how the set drifts: a client
//! that adds a code here changes replay behaviour for every RPC it makes,
//! without the proto surface having said anything about it.
//!
//! The TypeScript profile (`api-bones-connect-ts/src/retry.ts`) already fixed
//! this policy once. This module is the Rust half of the same decision, so the
//! two adapters agree on what "retryable" means rather than converging by
//! accident.
//!
//! Scope: this is the *transport-shape* half — did the call reach the server.
//! The TypeScript profile additionally gates eligibility on each method's
//! proto-declared `idempotency`, read from a generated policy artifact that has
//! no Rust equivalent yet. Until that artifact exists, a Rust caller still owes
//! its own judgement about whether the specific RPC is safe to replay; these
//! predicates only tell it whether the failure looked like one that never ran.

use connectrpc::{ConnectError, ErrorCode};

/// Failures raised while writing the request onto the connection.
///
/// A keep-alive pool hands back a socket the peer has already closed and the
/// write fails before any byte of the request is delivered. Connect surfaces
/// that as `Internal` — the code it uses for "the transport itself broke" — so
/// a code check alone never sees it and the call fails outright even though
/// nothing ran on the server.
const CONNECTION_WRITE_FAILURE_SIGNATURES: &[&str] = &[
    "write epipe",
    "broken pipe",
    "econnreset",
    "connection reset",
    "socket hang up",
    "http2 stream closed",
    "received goaway",
];

/// Whether a code may be retried with no instruction from the server.
///
/// Only [`ErrorCode::Unavailable`]: it is the one code that reliably means this
/// connection did not carry the call.
///
/// [`ErrorCode::Aborted`] is deliberately absent. It signals a concurrency or
/// transaction conflict, and the correct response is to re-run the enclosing
/// transaction, not to replay one RPC inside it. Treating it as retryable is
/// also how a watch loop that reconnects on "retryable" turns a server that
/// keeps aborting into a spin.
///
/// [`ErrorCode::ResourceExhausted`] is absent too: retrying a quota rejection
/// on our own schedule is how a degraded service becomes a saturated one.
#[must_use]
pub fn is_unprompted_retryable(code: ErrorCode) -> bool {
    matches!(code, ErrorCode::Unavailable)
}

/// Whether a failure happened before the request reached the server.
///
/// Matched on the message because the underlying cause's code does not survive
/// being wrapped into a [`ConnectError`].
#[must_use]
pub fn is_connection_write_failure(error: &ConnectError) -> bool {
    if error.code != ErrorCode::Internal {
        return false;
    }
    let message = error.message.as_deref().unwrap_or_default().to_lowercase();
    CONNECTION_WRITE_FAILURE_SIGNATURES
        .iter()
        .any(|signature| message.contains(signature))
}

/// Whether a failure's *shape* permits a replay.
///
/// A socket that rejected the write demonstrably carried nothing, which is the
/// safest replay there is; `Unavailable` can reach the client after the server
/// accepted the call, which is why callers of non-idempotent RPCs must still
/// decide for themselves (see the module note on the missing policy artifact).
#[must_use]
pub fn is_replayable_transport_failure(error: &ConnectError) -> bool {
    is_unprompted_retryable(error.code) || is_connection_write_failure(error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_unavailable_retries_without_server_instruction() {
        assert!(is_unprompted_retryable(ErrorCode::Unavailable));
        for code in [
            ErrorCode::Aborted,
            ErrorCode::ResourceExhausted,
            ErrorCode::DeadlineExceeded,
            ErrorCode::Internal,
            ErrorCode::FailedPrecondition,
        ] {
            assert!(!is_unprompted_retryable(code), "{code:?} must not retry");
        }
    }

    #[test]
    fn a_rejected_write_is_replayable_but_a_semantic_internal_is_not() {
        assert!(is_connection_write_failure(&ConnectError::new(
            ErrorCode::Internal,
            "write EPIPE",
        )));
        assert!(is_connection_write_failure(&ConnectError::new(
            ErrorCode::Internal,
            "received GOAWAY from peer",
        )));
        // The server ran and broke: replaying it just breaks it again.
        assert!(!is_connection_write_failure(&ConnectError::new(
            ErrorCode::Internal,
            "invariant failed",
        )));
        // Only Internal carries these signatures; the same words under another
        // code describe a server-side outcome, not a write that never left.
        assert!(!is_connection_write_failure(&ConnectError::new(
            ErrorCode::Unknown,
            "broken pipe",
        )));
    }

    #[test]
    fn aborted_is_never_a_replayable_transport_failure() {
        // The regression this module exists to prevent: a concurrency conflict
        // read as a transport failure, replayed automatically, and — in a watch
        // loop — reconnected without pause.
        assert!(!is_replayable_transport_failure(&ConnectError::new(
            ErrorCode::Aborted,
            "registry changed during reconciliation",
        )));
        assert!(is_replayable_transport_failure(&ConnectError::new(
            ErrorCode::Unavailable,
            "connection refused",
        )));
    }
}
