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
}
