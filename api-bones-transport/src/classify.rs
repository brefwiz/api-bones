// SPDX-License-Identifier: MIT
//! Which failures prove the peer never ran the request, and which were a
//! failure to connect.
//!
//! The verdict comes from where in the request's life the failure arose, not
//! from the text or chain of an error:
//!
//! - a failure of the connection's gate means no stream was opened, so the
//!   peer saw nothing: any failure waiting for readiness, and a failure to
//!   open the stream that is a connection failure (a request h2 refuses to
//!   encode is not classified);
//! - a failure of the request's own stream reply is never-ran only when the
//!   peer said so for that stream: a remote GOAWAY with `NO_ERROR` (h2 gives a
//!   stream that error only when its id is above the frame's last-stream-id)
//!   or a remote `RST_STREAM` with `REFUSED_STREAM`;
//! - everything else (an error-coded GOAWAY, an I/O error, a keepalive
//!   timeout, an idle close, any error after the response head) may have run.

use std::error::Error;
use std::fmt;
use std::io;

use api_bones::Reach;
use connectrpc::ErrorCode;

/// Why a failed request provably never ran, so sending it again cannot run it
/// twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NeverRan {
    /// The connection refused the request before opening a stream for it: the
    /// connection had closed or received a GOAWAY.
    Unsent,
    /// The peer's GOAWAY carried `NO_ERROR` and excluded the request's stream
    /// (its id is above the frame's last-stream-id).
    GoAway,
    /// The peer reset the request's stream with `REFUSED_STREAM`.
    RefusedStream,
}

impl NeverRan {
    /// The classification's name as it appears in the transport's events.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unsent => "unsent",
            Self::GoAway => "goaway_no_error",
            Self::RefusedStream => "refused_stream",
        }
    }
}

impl fmt::Display for NeverRan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where an h2 error arose in the request's life.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Origin {
    /// Waiting for the connection to accept a new stream. Any failure here
    /// (the connection failed, or its stream ids ran out) leaves the
    /// connection unusable and no stream exists yet.
    Ready,
    /// Opening the stream once the connection was ready.
    Gate,
    /// The reply future of the request's own, opened stream.
    OwnStream,
}

fn classify(origin: Origin, error: &h2::Error) -> Option<NeverRan> {
    match origin {
        Origin::Ready => Some(NeverRan::Unsent),
        // DESIGN: a user error (a request h2 refuses to encode) has no reason
        // and is not an I/O error; it is not a connection failure and a fresh
        // connection would refuse the same request, so it is not classified.
        Origin::Gate => (error.reason().is_some() || error.is_io()).then_some(NeverRan::Unsent),
        Origin::OwnStream => h2_never_ran(error),
    }
}

fn h2_never_ran(error: &h2::Error) -> Option<NeverRan> {
    if !error.is_remote() {
        return None;
    }
    match error.reason() {
        Some(h2::Reason::NO_ERROR) if error.is_go_away() => Some(NeverRan::GoAway),
        Some(h2::Reason::REFUSED_STREAM) if error.is_reset() => Some(NeverRan::RefusedStream),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Connect,
    Request,
}

/// A failed request or connection attempt, carrying the verdict taken where
/// the failure arose. It sits in the `source()` chain of the
/// [`connectrpc::ConnectError`] the caller gets.
#[derive(Debug)]
pub(crate) struct TransportError {
    kind: Kind,
    never_ran: Option<NeverRan>,
    source: Box<dyn Error + Send + Sync>,
}

impl TransportError {
    fn new(
        kind: Kind,
        never_ran: Option<NeverRan>,
        source: impl Into<Box<dyn Error + Send + Sync>>,
    ) -> Self {
        Self {
            kind,
            never_ran,
            source: source.into(),
        }
    }

    /// A failure to establish the connection.
    pub(crate) fn connect(source: impl Into<Box<dyn Error + Send + Sync>>) -> Self {
        Self::new(Kind::Connect, None, source)
    }

    /// A failure of the request, classified by `origin`.
    pub(crate) fn request(origin: Origin, error: h2::Error) -> Self {
        let never_ran = classify(origin, &error);
        Self::new(Kind::Request, never_ran, error)
    }

    /// The connection was already closed when the request reached its gate.
    pub(crate) fn closed() -> Self {
        Self::new(
            Kind::Request,
            Some(NeverRan::Unsent),
            io::Error::new(
                io::ErrorKind::NotConnected,
                "the connection closed before a stream was opened for the request",
            ),
        )
    }

    pub(crate) const fn never_ran(&self) -> Option<NeverRan> {
        self.never_ran
    }
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self.kind {
            Kind::Connect => "failed to connect",
            Kind::Request => "request failed",
        })
    }
}

impl Error for TransportError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&*self.source)
    }
}

/// Whether `error` proves the peer never ran the request, and why.
///
/// Answers from the verdict the transport recorded where the failure arose
/// (see the module documentation), found by walking the `source()` chain:
/// `Some` only for the three cases RFC 9113 section 8.7 defines as not
/// processed. A bare h2 or I/O error that did not come through the transport
/// answers `None`, as does an error-coded GOAWAY, any other reset, an I/O
/// error, a timeout or a failure to connect.
#[must_use]
pub fn never_ran(error: &(dyn Error + 'static)) -> Option<NeverRan> {
    let mut current: Option<&(dyn Error + 'static)> = Some(error);
    while let Some(link) = current {
        if let Some(transport) = link.downcast_ref::<TransportError>() {
            return transport.never_ran;
        }
        current = link.source();
    }
    None
}

/// Whether `error` is a failure to establish the connection (refused,
/// unreachable, TLS or SPIFFE peer verification failed, or the establishment
/// bound elapsed) rather than a failure after the request was on the wire.
///
/// True when the `source()` chain holds a connect-phase transport failure, or
/// a `ConnectError` with code `Unavailable` caused by a
/// `tokio::time::error::Elapsed` (the establishment bound). A true result is
/// also returned when the TLS or SPIFFE peer check fails, so it does not mean
/// the peer is merely still booting: callers must not treat it as grounds to
/// retry without bound or to downgrade security.
#[must_use]
pub fn failed_to_connect(error: &(dyn Error + 'static)) -> bool {
    let mut current: Option<&(dyn Error + 'static)> = Some(error);
    while let Some(link) = current {
        if link
            .downcast_ref::<TransportError>()
            .is_some_and(|transport| transport.kind == Kind::Connect)
        {
            return true;
        }
        if link
            .downcast_ref::<connectrpc::ConnectError>()
            .is_some_and(|e| e.code == ErrorCode::Unavailable && has_elapsed_source(link))
        {
            return true;
        }
        current = link.source();
    }
    false
}

/// How far a failed call got, for the replay rule (see [`Reach`]).
///
/// The first transport failure in the chain decides: [`Reach::Unreached`] when
/// it is a connect-phase failure or carries a never-ran verdict, because only
/// the transport records those where the failure arose; an error shape any
/// caller can build (such as an `Unavailable` caused by a timeout) proves
/// nothing. [`Reach::Unanswered`] for any other transport failure.
/// Without one, [`Reach::Answered`] when the chain holds a server-coded
/// `ConnectError`: one whose code is not one a client synthesizes for itself
/// (`Unavailable`, `DeadlineExceeded`, `Canceled`). Everything else is
/// [`Reach::Unanswered`], because only a proof may make a call replayable.
#[must_use]
pub fn reach_of(error: &(dyn Error + 'static)) -> Reach {
    let mut server_coded = false;
    let mut current: Option<&(dyn Error + 'static)> = Some(error);
    while let Some(link) = current {
        if let Some(transport) = link.downcast_ref::<TransportError>() {
            return if transport.kind == Kind::Connect || transport.never_ran.is_some() {
                Reach::Unreached
            } else {
                Reach::Unanswered
            };
        }
        if let Some(e) = link.downcast_ref::<connectrpc::ConnectError>() {
            server_coded |= !matches!(
                e.code,
                ErrorCode::Unavailable | ErrorCode::DeadlineExceeded | ErrorCode::Canceled
            );
        }
        current = link.source();
    }
    if server_coded {
        Reach::Answered
    } else {
        Reach::Unanswered
    }
}

fn has_elapsed_source(error: &(dyn Error + 'static)) -> bool {
    let mut current = error.source();
    while let Some(link) = current {
        if link.is::<tokio::time::error::Elapsed>() {
            return true;
        }
        current = link.source();
    }
    false
}

#[cfg(test)]
mod tests {
    //! h2 errors are not publicly constructible (their constructors are
    //! crate-private), so the remote cases of [`never_ran`] are proven against
    //! a real HTTP/2 server in `tests/transport.rs`. What can be constructed
    //! here is the origin rules and the negative space: errors that look close
    //! but must never classify as never-ran.

    use super::*;
    use std::time::Duration;

    fn elapsed() -> tokio::time::error::Elapsed {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("runtime")
            .block_on(async {
                tokio::time::timeout(Duration::from_millis(1), std::future::pending::<()>())
                    .await
                    .expect_err("pending never completes")
            })
    }

    #[test]
    fn an_io_error_never_classifies_as_never_ran() {
        let io = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "stream closed");
        assert_eq!(never_ran(&io), None);
        let wrapped = connectrpc::ConnectError::unavailable_from_transport("request failed", io);
        assert_eq!(never_ran(&wrapped), None);
    }

    #[test]
    fn an_h2_reason_converted_into_an_error_is_local_and_never_classifies() {
        // `From<Reason> for h2::Error` builds a library-initiated error: the
        // peer said nothing, so nothing proves the request did not run.
        let refused: h2::Error = h2::Reason::REFUSED_STREAM.into();
        assert!(!refused.is_remote());
        assert_eq!(never_ran(&refused), None);
        let wrapped =
            connectrpc::ConnectError::unavailable_from_transport("request failed", refused);
        assert_eq!(never_ran(&wrapped), None);
        let no_error: h2::Error = h2::Reason::NO_ERROR.into();
        assert_eq!(never_ran(&no_error), None);
    }

    #[test]
    fn a_text_that_names_goaway_never_classifies() {
        let text = connectrpc::ConnectError::unavailable("h2 send failed: received goaway");
        assert_eq!(never_ran(&text), None);
    }

    #[test]
    fn classification_names_are_stable() {
        assert_eq!(NeverRan::Unsent.to_string(), "unsent");
        assert_eq!(NeverRan::GoAway.to_string(), "goaway_no_error");
        assert_eq!(NeverRan::RefusedStream.to_string(), "refused_stream");
    }

    #[test]
    fn failed_to_connect_true_for_establishment_bound_chain() {
        let err =
            connectrpc::ConnectError::unavailable("establishment timed out").with_source(elapsed());
        assert!(failed_to_connect(&err));
        let wrong_code = connectrpc::ConnectError::internal("x").with_source(elapsed());
        assert!(!failed_to_connect(&wrong_code));
    }

    #[test]
    fn failed_to_connect_false_for_non_connect_errors() {
        assert!(!failed_to_connect(&connectrpc::ConnectError::unavailable(
            "x"
        )));
        assert!(!failed_to_connect(&std::io::Error::other("boom")));
    }

    #[test]
    fn an_error_from_the_gate_means_no_stream_was_opened() {
        let refused: h2::Error = h2::Reason::REFUSED_STREAM.into();
        let error = TransportError::request(Origin::Gate, refused);
        assert_eq!(error.never_ran(), Some(NeverRan::Unsent));
        let wrapped = connectrpc::ConnectError::unavailable_from_transport("failed", error);
        assert_eq!(never_ran(&wrapped), Some(NeverRan::Unsent));
        assert_eq!(TransportError::closed().never_ran(), Some(NeverRan::Unsent));
    }

    #[test]
    fn any_error_waiting_for_readiness_means_no_stream_was_opened() {
        // A readiness error needs no reason and no I/O cause (a stream-id
        // overflow has neither); the origin alone decides. h2 cannot build
        // such an error here, so the reason-carrying one stands for the rule.
        let error: h2::Error = h2::Reason::NO_ERROR.into();
        assert_eq!(
            TransportError::request(Origin::Ready, error).never_ran(),
            Some(NeverRan::Unsent)
        );
    }

    #[test]
    fn a_goaway_reaching_an_opened_stream_is_never_replayable_unless_it_is_remote() {
        // A GOAWAY-shaped error that the peer did not send (library-initiated)
        // proves nothing on the request's own stream, whatever its reason.
        for reason in [h2::Reason::NO_ERROR, h2::Reason::REFUSED_STREAM] {
            let local: h2::Error = reason.into();
            let error = TransportError::request(Origin::OwnStream, local);
            assert_eq!(error.never_ran(), None, "{reason:?}");
        }
    }

    #[test]
    fn a_connect_failure_is_flagged_and_never_replayable() {
        let error = TransportError::connect(io::Error::other("refused"));
        assert_eq!(error.never_ran(), None);
        let wrapped = connectrpc::ConnectError::unavailable_from_transport("failed", error);
        assert!(failed_to_connect(&wrapped));
        assert_eq!(never_ran(&wrapped), None);
    }

    #[test]
    fn a_dial_failure_is_unreached() {
        let error = TransportError::connect(io::Error::other("refused"));
        let wrapped = connectrpc::ConnectError::unavailable_from_transport("failed", error);
        assert_eq!(reach_of(&wrapped), Reach::Unreached);
    }

    #[test]
    fn a_timeout_shape_anyone_can_build_is_unanswered() {
        let forged = connectrpc::ConnectError::unavailable("call timed out").with_source(elapsed());
        assert_eq!(reach_of(&forged), Reach::Unanswered);
    }

    #[test]
    fn a_request_failure_with_a_timeout_source_is_unanswered() {
        let error = TransportError::new(Kind::Request, None, elapsed());
        let wrapped = connectrpc::ConnectError::unavailable("failed").with_source(error);
        assert_eq!(reach_of(&wrapped), Reach::Unanswered);
    }

    #[test]
    fn a_proven_never_ran_is_unreached() {
        let wrapped = connectrpc::ConnectError::unavailable_from_transport(
            "failed",
            TransportError::closed(),
        );
        assert_eq!(reach_of(&wrapped), Reach::Unreached);
    }

    #[test]
    fn a_failure_after_the_request_was_sent_is_unanswered() {
        let local: h2::Error = h2::Reason::NO_ERROR.into();
        let error = TransportError::request(Origin::OwnStream, local);
        let wrapped = connectrpc::ConnectError::unavailable_from_transport("failed", error);
        assert_eq!(reach_of(&wrapped), Reach::Unanswered);
        assert_eq!(reach_of(&io::Error::other("reset")), Reach::Unanswered);
    }

    #[test]
    fn a_client_synthesized_code_is_unanswered() {
        for error in [
            connectrpc::ConnectError::unavailable("x"),
            connectrpc::ConnectError::deadline_exceeded("x"),
            connectrpc::ConnectError::canceled("x"),
        ] {
            assert_eq!(reach_of(&error), Reach::Unanswered, "{error:?}");
        }
    }

    #[test]
    fn a_server_coded_error_is_answered() {
        for error in [
            connectrpc::ConnectError::internal("boom"),
            connectrpc::ConnectError::not_found("gone"),
            connectrpc::ConnectError::invalid_argument("bad"),
        ] {
            assert_eq!(reach_of(&error), Reach::Answered, "{error:?}");
        }
    }

    #[test]
    fn a_transport_error_is_unanswered_even_under_a_server_code() {
        let local: h2::Error = h2::Reason::NO_ERROR.into();
        let bare = TransportError::request(Origin::OwnStream, local);
        assert_eq!(reach_of(&bare), Reach::Unanswered);
        let local: h2::Error = h2::Reason::NO_ERROR.into();
        let coded = connectrpc::ConnectError::internal("x")
            .with_source(TransportError::request(Origin::OwnStream, local));
        assert_eq!(reach_of(&coded), Reach::Unanswered);
    }

    #[test]
    fn a_server_code_anywhere_in_the_chain_is_answered() {
        let chain = connectrpc::ConnectError::internal("x")
            .with_source(connectrpc::ConnectError::unavailable("y"));
        assert_eq!(reach_of(&chain), Reach::Answered);
    }
}
