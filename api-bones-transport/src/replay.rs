// SPDX-License-Identifier: MIT
//! Sending a request, and sending it once more when it provably never ran.
//!
//! The replay holds when the failure classifies under [`never_ran`] (see
//! [`crate::classify`]: the verdict is taken where the failure arose), the
//! request body is a buffer (its size hint is exact, as connectrpc's unary
//! and server-streaming bodies are), and time is left before the caller's
//! deadline. The replay goes to a fresh connection, with the timeout header
//! rewritten to the time remaining. Its failure goes back to the caller
//! whatever it is: a request is replayed at most once.

use std::sync::Arc;
use std::time::Instant;

use connectrpc::ConnectError;
use connectrpc::client::{ClientBody, full_body};
use http::request::Parts;
use http::{Request, Response};
use http_body::Body as _;
use http_body_util::BodyExt as _;

use crate::body::ResponseBody;
use crate::classify::{NeverRan, TransportError, failed_to_connect, never_ran};
use crate::conn::Conn;
use crate::deadline::{self, Deadline};
use crate::peer;
use crate::pool::Pool;

/// A failed attempt, and the connection it ran on when it got that far.
struct Failure {
    error: TransportError,
    conn: Option<Arc<Conn>>,
}

async fn attempt(
    pool: &Pool,
    parts: Parts,
    body: ClientBody,
) -> Result<Response<ResponseBody>, Failure> {
    let conn = pool
        .acquire(&parts.uri)
        .await
        .map_err(|error| Failure { error, conn: None })?;
    conn.send(parts, body).await.map_err(|error| Failure {
        error,
        conn: Some(Arc::clone(&conn)),
    })
}

/// Sends `request` through the pool, replaying it once when the transport can
/// prove the peer never ran it.
pub(crate) async fn send(
    pool: Arc<Pool>,
    request: Request<ClientBody>,
) -> Result<Response<ResponseBody>, ConnectError> {
    let deadline = Deadline::of(request.headers(), Instant::now());
    let (parts, body) = request.into_parts();
    let peer = Peer::of(&parts);

    if body.size_hint().exact().is_none() {
        // A streaming body is consumed as it is sent and cannot be sent
        // again, so its request is never replayed.
        return attempt(&pool, parts, body).await.map_err(|failure| {
            failed(&peer, &failure.error, 1, Replay::StreamingBody);
            into_connect_error(failure.error)
        });
    }

    let bytes = body.collect().await?.to_bytes();
    let failure = match attempt(&pool, parts.clone(), full_body(bytes.clone())).await {
        Ok(response) => return Ok(response),
        Err(failure) => failure,
    };
    let Some(reason) = failure.error.never_ran() else {
        failed(&peer, &failure.error, 1, Replay::MayHaveRun);
        return Err(into_connect_error(failure.error));
    };
    let Some(parts) = replay_parts(parts, deadline) else {
        failed(&peer, &failure.error, 1, Replay::NoTimeLeft(reason));
        return Err(into_connect_error(failure.error));
    };
    failed(&peer, &failure.error, 1, Replay::Replaying(reason));
    if let Some(conn) = &failure.conn {
        // The replay must not meet the connection that just failed it.
        conn.close();
    }
    tracing::info!(
        target: "api_bones_transport",
        event = "retry",
        peer = %peer.0,
        never_ran = reason.as_str(),
    );
    attempt(&pool, parts, full_body(bytes))
        .await
        .map_err(|failure| {
            failed(&peer, &failure.error, 2, Replay::AttemptsSpent);
            into_connect_error(failure.error)
        })
}

/// The request head for the replay, with its timeout header stating the
/// time remaining, or `None` when none remains.
fn replay_parts(mut parts: Parts, deadline: Option<Deadline>) -> Option<Parts> {
    let Some(deadline) = deadline else {
        return Some(parts);
    };
    let remaining = deadline.remaining(Instant::now())?;
    deadline::rewrite(&mut parts.headers, remaining).then_some(parts)
}

/// The same wrapping as connectrpc's own `HttpClient`, so the transport error
/// stays in the `source()` chain for [`failed_to_connect`] and [`never_ran`].
fn into_connect_error(error: TransportError) -> ConnectError {
    ConnectError::unavailable_from_transport("HTTP request failed", error)
}

/// The peer a request goes to, as its events name it.
struct Peer(String);

impl Peer {
    fn of(parts: &Parts) -> Self {
        Self(peer::name(&parts.uri))
    }
}

/// What the transport does with a failed request.
#[derive(Clone, Copy)]
enum Replay {
    /// Replayed once: the request never ran.
    Replaying(NeverRan),
    /// Not replayed: the request never ran, but no time was left.
    NoTimeLeft(NeverRan),
    /// Not replayed: nothing proves the request did not run.
    MayHaveRun,
    /// Not replayed: the request body is a stream.
    StreamingBody,
    /// Not replayed: this was the replay.
    AttemptsSpent,
}

impl Replay {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Replaying(_) => "replaying",
            Self::NoTimeLeft(_) => "no_time_left",
            Self::MayHaveRun => "may_have_run",
            Self::StreamingBody => "streaming_body",
            Self::AttemptsSpent => "attempts_spent",
        }
    }
}

fn failed(peer: &Peer, error: &TransportError, attempt: u8, replay: Replay) {
    let classified = match replay {
        Replay::Replaying(reason) | Replay::NoTimeLeft(reason) => Some(reason),
        Replay::MayHaveRun | Replay::StreamingBody | Replay::AttemptsSpent => never_ran(error),
    };
    tracing::warn!(
        target: "api_bones_transport",
        event = "request_failed",
        peer = %peer.0,
        attempt,
        never_ran = classified.map_or("none", NeverRan::as_str),
        failed_to_connect = failed_to_connect(error),
        replay = replay.as_str(),
        error = ?error,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use std::time::Duration;

    fn parts(timeout_ms: Option<&'static str>) -> Parts {
        let mut builder = Request::builder().uri("http://peer:80/svc/Method");
        if let Some(value) = timeout_ms {
            builder = builder.header("connect-timeout-ms", value);
        }
        builder.body(()).expect("request").into_parts().0
    }

    #[test]
    fn a_request_without_a_deadline_replays_unchanged() {
        let replay = replay_parts(parts(None), None).expect("replayable");
        assert!(replay.headers.is_empty());
    }

    #[test]
    fn a_replay_states_less_time_than_the_first_send() {
        let head = parts(Some("60000"));
        let deadline = Deadline::of(&head.headers, Instant::now());
        std::thread::sleep(Duration::from_millis(5));
        let replay = replay_parts(head, deadline).expect("time is left");
        let stated: u64 = replay.headers["connect-timeout-ms"]
            .to_str()
            .expect("ascii")
            .parse()
            .expect("number");
        assert!(stated < 60_000);
    }

    #[test]
    fn no_replay_once_the_deadline_passed() {
        let head = parts(Some("1"));
        let deadline = Deadline::of(&head.headers, Instant::now());
        std::thread::sleep(Duration::from_millis(5));
        assert!(replay_parts(head, deadline).is_none());
    }

    #[test]
    fn an_io_failure_is_not_a_reason_to_replay() {
        let io = std::io::Error::other("connection reset");
        let wrapped = ConnectError::unavailable_from_transport("HTTP request failed", io);
        assert!(never_ran(&wrapped).is_none());
    }

    #[test]
    fn a_full_body_has_an_exact_size_and_a_stream_does_not() {
        assert!(
            full_body(Bytes::from_static(b"x"))
                .size_hint()
                .exact()
                .is_some()
        );
        let stream = http_body_util::StreamBody::new(futures_util::stream::iter(Vec::<
            Result<http_body::Frame<Bytes>, ConnectError>,
        >::new()));
        assert!(stream.size_hint().exact().is_none());
    }
}
