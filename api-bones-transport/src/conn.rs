// SPDX-License-Identifier: MIT
//! One HTTP/2 connection driven through `h2::client` directly.
//!
//! Streams are opened behind a serialized gate: one [`SendRequest`] behind an
//! async mutex, and a request holds the mutex from waiting for readiness
//! through `send_request`. At most one stream is pending-open inside h2; every
//! other request waits in this crate's code, never handed to h2, so moving it
//! to a fresh connection is not a replay.
//!
//! A supervisor task per connection sends keepalive PINGs and closes the
//! connection after [`POOL_IDLE_TIMEOUT`] without a request.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use bytes::Bytes;
use connectrpc::client::ClientBody;
use h2::client::{Builder, ResponseFuture, SendRequest};
use h2::{Ping, PingPong, SendStream};
use http::header::{CONNECTION, HeaderMap, HeaderName, TRANSFER_ENCODING, UPGRADE};
use http::request::Parts;
use http::{Request, Response, Uri};
use http_body::Body as _;
use tokio::sync::{Mutex as AsyncMutex, Notify, oneshot};
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::body::{self, ResponseBody};
use crate::classify::{Origin, TransportError};
use crate::connector::{Connector, Io};
use crate::{KEEPALIVE_INTERVAL, KEEPALIVE_TIMEOUT, POOL_IDLE_TIMEOUT, driver, peer};

/// Connection-level flow-control window, the size hyper's client uses.
const CONNECTION_WINDOW: u32 = 5 * 1024 * 1024;
/// Per-stream flow-control window, the size hyper's client uses.
const STREAM_WINDOW: u32 = 2 * 1024 * 1024;
/// Bytes h2 buffers per stream before the body pump waits, the size hyper's
/// client uses.
const SEND_BUFFER: usize = 400 * 1024;
/// Streams h2 allows before the server's SETTINGS arrive, as hyper's client.
const INITIAL_SEND_STREAMS: usize = 100;

const KEEP_ALIVE: HeaderName = HeaderName::from_static("keep-alive");
const PROXY_CONNECTION: HeaderName = HeaderName::from_static("proxy-connection");

/// An open connection to one peer.
pub(crate) struct Conn {
    peer: String,
    opened: std::time::Instant,
    send: AsyncMutex<Option<SendRequest<Bytes>>>,
    closed: AtomicBool,
    activity: Mutex<Activity>,
    /// Wakes the idle watcher when the connection is flagged closed or its
    /// last request ends.
    changed: Notify,
}

struct Activity {
    in_flight: usize,
    idle_since: Option<Instant>,
}

/// Holds a connection's in-flight count up while a request or its response
/// body lives.
pub(crate) struct InFlight(Arc<Conn>);

impl Drop for InFlight {
    fn drop(&mut self) {
        self.0.end();
    }
}

impl Conn {
    fn new(send: SendRequest<Bytes>, peer: String) -> Arc<Self> {
        Arc::new(Self {
            peer,
            opened: std::time::Instant::now(),
            send: AsyncMutex::new(Some(send)),
            closed: AtomicBool::new(false),
            activity: Mutex::new(Activity {
                in_flight: 0,
                idle_since: Some(Instant::now()),
            }),
            changed: Notify::new(),
        })
    }

    /// Connects to the peer `uri` names on the driver runtime, so the
    /// connection outlives the runtime of the caller that opened it.
    pub(crate) async fn dial(
        connector: &Connector,
        uri: &Uri,
    ) -> Result<Arc<Self>, TransportError> {
        let (connector, uri) = (connector.clone(), uri.clone());
        let (hand_over, received) = oneshot::channel();
        drop(driver::handle().spawn(async move {
            let dialed = start(&connector, &uri).await;
            // The caller was cancelled while the dial ran: nobody will use
            // the connection, so it closes at once rather than idling out.
            if let Err(Ok(orphan)) = hand_over.send(dialed) {
                orphan.close();
            }
        }));
        received
            .await
            .map_err(|_dropped| TransportError::connect("the dial task ended without a result"))?
    }

    /// Whether the pool must not hand this connection to a new request.
    pub(crate) fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    /// Flags the connection so that no new request is routed to it. Requests
    /// in flight finish; the connection closes once they have.
    pub(crate) fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.changed.notify_one();
    }

    /// Sends `parts` and `body` on a new stream of this connection.
    pub(crate) async fn send(
        self: &Arc<Self>,
        mut parts: Parts,
        body: ClientBody,
    ) -> Result<Response<ResponseBody>, TransportError> {
        strip_connection_headers(&mut parts.headers);
        let end_of_stream = body.is_end_stream();
        let (reply, stream, flight) = self
            .open(Request::from_parts(parts, ()), end_of_stream)
            .await?;
        if !end_of_stream {
            drop(driver::handle().spawn(body::pump(body, stream)));
        }
        let response = reply
            .await
            .map_err(|error| TransportError::request(Origin::OwnStream, error))?;
        Ok(response.map(|received| ResponseBody::new(received, flight)))
    }

    /// Waits for the right to open a stream and opens it. An error is the
    /// gate's: no stream was opened.
    async fn open(
        self: &Arc<Self>,
        request: Request<()>,
        end_of_stream: bool,
    ) -> Result<(ResponseFuture, SendStream<Bytes>, InFlight), TransportError> {
        let mut gate = self.send.lock().await;
        let Some(sender) = gate.as_mut() else {
            return Err(TransportError::closed());
        };
        let flight = self.begin();
        // Any readiness error (the connection failed, or its stream ids ran
        // out) leaves this connection unusable, and no stream exists yet.
        if let Err(error) = std::future::poll_fn(|cx| sender.poll_ready(cx)).await {
            self.close();
            return Err(TransportError::request(Origin::Ready, error));
        }
        match sender.send_request(request, end_of_stream) {
            Ok((reply, stream)) => {
                // A stream the peer's stream limit left pending-open has one
                // waker slot, shared by the readiness wait and the body
                // pump's capacity wait; whichever parks last evicts the
                // other, which then never wakes. Waiting here, under the
                // gate and before the pump exists, leaves the slot to the
                // pump. The stream exists and may reach the peer, so this
                // result never classifies the request: its outcome comes
                // only from its own reply. A failure only flags the
                // connection closed for new requests.
                if std::future::poll_fn(|cx| sender.poll_ready(cx))
                    .await
                    .is_err()
                {
                    self.close();
                }
                Ok((reply, stream, flight))
            }
            Err(error) => {
                let error = TransportError::request(Origin::Gate, error);
                if error.never_ran().is_some() {
                    self.close();
                }
                Err(error)
            }
        }
    }

    fn activity(&self) -> MutexGuard<'_, Activity> {
        // The guarded counters are updated in single statements, so a panic
        // while holding the lock cannot leave them inconsistent.
        self.activity.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn begin(self: &Arc<Self>) -> InFlight {
        let mut activity = self.activity();
        activity.in_flight += 1;
        activity.idle_since = None;
        InFlight(Arc::clone(self))
    }

    fn end(&self) {
        let mut activity = self.activity();
        activity.in_flight -= 1;
        if activity.in_flight == 0 {
            activity.idle_since = Some(Instant::now());
            drop(activity);
            self.changed.notify_one();
        }
    }

    /// How long until the connection is due to close for want of use.
    fn idle_remaining(&self) -> Duration {
        self.activity()
            .idle_since
            .map_or(POOL_IDLE_TIMEOUT, |since| {
                POOL_IDLE_TIMEOUT.saturating_sub(since.elapsed())
            })
    }

    /// Whether no request uses the connection and it has been unused for
    /// [`POOL_IDLE_TIMEOUT`] or was flagged closed.
    fn due(&self) -> bool {
        let activity = self.activity();
        activity.in_flight == 0
            && (self.is_closed()
                || activity
                    .idle_since
                    .is_some_and(|since| since.elapsed() >= POOL_IDLE_TIMEOUT))
    }

    /// Closes the connection when it is [due](Self::due). Checked without the
    /// gate first, so the watcher never queues on it ahead of requests; then
    /// again under the gate, so no request is between its readiness wait and
    /// its stream.
    async fn close_if_idle(&self) -> bool {
        if !self.due() {
            return false;
        }
        let mut gate = self.send.lock().await;
        if !self.due() {
            return false;
        }
        self.closed.store(true, Ordering::SeqCst);
        // Dropping the last handle lets h2 send GOAWAY and end.
        *gate = None;
        true
    }
}

fn builder() -> Builder {
    let mut builder = Builder::new();
    builder
        .initial_window_size(STREAM_WINDOW)
        .initial_connection_window_size(CONNECTION_WINDOW)
        .initial_max_send_streams(INITIAL_SEND_STREAMS)
        .max_send_buffer_size(SEND_BUFFER)
        .enable_push(false);
    builder
}

/// Hop-by-hop headers h2 forbids on a request.
fn strip_connection_headers(headers: &mut HeaderMap) {
    for name in [
        CONNECTION,
        KEEP_ALIVE,
        PROXY_CONNECTION,
        TRANSFER_ENCODING,
        UPGRADE,
    ] {
        headers.remove(name);
    }
}

/// Runs on the driver runtime.
async fn start(connector: &Connector, uri: &Uri) -> Result<Arc<Conn>, TransportError> {
    let io = connector
        .connect(uri)
        .await
        .map_err(TransportError::connect)?;
    let (conn, _supervisor) = adopt(io, peer::name(uri)).await?;
    Ok(conn)
}

/// Performs the h2 handshake over `io` and starts the tasks that drive and
/// supervise the connection on the current runtime. The returned handle ends
/// when the connection has closed.
async fn adopt(io: Io, peer: String) -> Result<(Arc<Conn>, JoinHandle<()>), TransportError> {
    let (send, mut connection) = builder()
        .handshake::<Io, Bytes>(io)
        .await
        .map_err(TransportError::connect)?;
    let pings = connection
        .ping_pong()
        .ok_or_else(|| TransportError::connect("h2 withheld the keepalive handle"))?;
    let driven = tokio::spawn(connection);
    let conn = Conn::new(send, peer);
    let supervisor = tokio::spawn(supervise(Arc::clone(&conn), pings, driven));
    tracing::info!(
        target: "api_bones_transport",
        event = "connection_opened",
        peer = %conn.peer,
    );
    Ok((conn, supervisor))
}

enum Ended {
    Connection(Result<Result<(), h2::Error>, tokio::task::JoinError>),
    Keepalive(Keepalive),
    Idle,
}

enum Keepalive {
    Unanswered,
    Failed(h2::Error),
}

/// Keeps `conn` alive and ends it: PINGs the peer every
/// [`KEEPALIVE_INTERVAL`] and closes the connection when one goes unanswered
/// for [`KEEPALIVE_TIMEOUT`], closes it after [`POOL_IDLE_TIMEOUT`] without a
/// request, and returns once the connection task has ended.
async fn supervise(
    conn: Arc<Conn>,
    mut pings: PingPong,
    mut driven: JoinHandle<Result<(), h2::Error>>,
) {
    let ended = {
        let keepalive = keepalive(&mut pings);
        let idle = idle(&conn);
        tokio::pin!(keepalive, idle);
        tokio::select! {
            result = &mut driven => Ended::Connection(result),
            failure = &mut keepalive => Ended::Keepalive(failure),
            () = &mut idle => Ended::Idle,
        }
    };
    conn.close();
    let finished = matches!(ended, Ended::Connection(_));
    match ended {
        Ended::Connection(Ok(Ok(()))) | Ended::Idle => {}
        Ended::Connection(Ok(Err(error))) => {
            tracing::debug!(target: "api_bones_transport", peer = %conn.peer, error = %error, "connection_error");
        }
        Ended::Connection(Err(error)) => {
            tracing::debug!(target: "api_bones_transport", peer = %conn.peer, error = %error, "connection_task_failed");
        }
        Ended::Keepalive(failure) => {
            let error = match &failure {
                Keepalive::Unanswered => None,
                Keepalive::Failed(error) => Some(error),
            };
            tracing::warn!(
                target: "api_bones_transport",
                event = "keepalive_failed",
                peer = %conn.peer,
                unanswered = error.is_none(),
                error = ?error,
            );
            // Dropping the connection resets every stream with an I/O error:
            // none of them classifies as never-ran.
            driven.abort();
        }
    }
    if !finished {
        // Idle close: the last handle is gone and h2 is saying GOAWAY. After
        // a keepalive failure the task was aborted and ends at once.
        drop(driven.await);
    }
    tracing::info!(
        target: "api_bones_transport",
        event = "connection_closed",
        peer = %conn.peer,
        open_ms = u64::try_from(conn.opened.elapsed().as_millis()).unwrap_or(u64::MAX),
    );
}

async fn keepalive(pings: &mut PingPong) -> Keepalive {
    loop {
        tokio::time::sleep(KEEPALIVE_INTERVAL).await;
        match tokio::time::timeout(KEEPALIVE_TIMEOUT, pings.ping(Ping::opaque())).await {
            Ok(Ok(_pong)) => {}
            Ok(Err(error)) => return Keepalive::Failed(error),
            Err(_elapsed) => return Keepalive::Unanswered,
        }
    }
}

/// Returns once the connection has been closed for want of use.
async fn idle(conn: &Conn) {
    loop {
        tokio::select! {
            () = tokio::time::sleep(conn.idle_remaining()) => {}
            () = conn.changed.notified() => {}
        }
        if conn.close_if_idle().await {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classify::NeverRan;
    use crate::connector::erase;
    use connectrpc::client::full_body;
    use http_body_util::BodyExt as _;
    use tokio::io::{DuplexStream, duplex};

    async fn answer(io: DuplexStream) {
        let mut server = h2::server::handshake(io).await.expect("server handshake");
        while let Some(Ok((_request, mut respond))) = server.accept().await {
            drop(respond.send_response(Response::new(()), true));
        }
    }

    fn call() -> (Parts, ClientBody) {
        Request::post("http://test/svc/Method")
            .body(full_body(Bytes::new()))
            .expect("request")
            .into_parts()
    }

    async fn use_once(conn: &Arc<Conn>) {
        let (parts, body) = call();
        let response = conn.send(parts, body).await.expect("sent");
        response.into_body().collect().await.expect("response body");
    }

    #[tokio::test(start_paused = true)]
    async fn a_connection_closes_after_the_idle_timeout_since_its_last_request() {
        let (client_io, server_io) = duplex(1 << 20);
        tokio::spawn(answer(server_io));
        let (conn, supervisor) = adopt(erase(client_io), "test".to_owned())
            .await
            .expect("adopt");
        let started = Instant::now();

        tokio::time::sleep(POOL_IDLE_TIMEOUT * 2 / 3).await;
        assert!(!conn.is_closed());
        use_once(&conn).await;
        tokio::time::sleep(POOL_IDLE_TIMEOUT * 2 / 3).await;
        assert!(!conn.is_closed(), "the request restarted the idle clock");

        supervisor.await.expect("supervisor");
        assert!(conn.is_closed());
        assert!(started.elapsed() >= POOL_IDLE_TIMEOUT * 2 / 3 + POOL_IDLE_TIMEOUT);

        let (parts, body) = call();
        let refused = conn
            .send(parts, body)
            .await
            .map(|_| ())
            .expect_err("closed");
        assert_eq!(refused.never_ran(), Some(NeverRan::Unsent));
    }

    #[tokio::test(start_paused = true)]
    async fn an_unanswered_keepalive_ping_closes_the_connection_and_fails_its_streams() {
        let (client_io, _silent_peer) = duplex(1 << 20);
        let (conn, supervisor) = adopt(erase(client_io), "test".to_owned())
            .await
            .expect("adopt");
        let started = Instant::now();
        let in_flight = {
            let conn = Arc::clone(&conn);
            tokio::spawn(async move {
                let (parts, body) = call();
                conn.send(parts, body).await.map(|_| ())
            })
        };

        supervisor.await.expect("supervisor");
        let elapsed = started.elapsed();
        assert!(
            elapsed >= KEEPALIVE_INTERVAL + KEEPALIVE_TIMEOUT,
            "{elapsed:?}"
        );
        assert!(
            elapsed < POOL_IDLE_TIMEOUT,
            "closed by the keepalive, not by idling"
        );
        assert!(conn.is_closed());

        let failed = in_flight.await.expect("task").expect_err("stream reset");
        assert_eq!(
            failed.never_ran(),
            None,
            "a keepalive timeout may have run it"
        );
    }
}
