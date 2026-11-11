// SPDX-License-Identifier: MIT
//! `HttpTransport` against a real in-process HTTP/2 server.
//!
//! The server is the `h2` crate's own, so each test can make the peer do
//! exactly what the classification is about: a GOAWAY with `NO_ERROR` or an
//! error code, a `REFUSED_STREAM`, a connection cut after a request arrived.
//! It counts accepted connections and every attempt of each request, keyed by
//! the request's `x-request-id`.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use api_bones_transport::{HttpTransport, failed_to_connect, never_ran};
use bytes::Bytes;
use connectrpc::ConnectError;
use connectrpc::client::{ClientBody, ClientTransport, full_body};
use connectrpc::rustls;
use futures_util::future::join_all;
use h2::Reason;
use http::{Request, Response};
use http_body_util::BodyExt as _;
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite};
use tokio::net::TcpListener;

const MAX_STREAMS: u32 = 200;

#[derive(Clone, Copy)]
enum Policy {
    /// Answer every request after `ANSWER_DELAY`.
    Answer,
    /// On the first connection, send a graceful GOAWAY (`NO_ERROR`) once this
    /// many streams were accepted.
    GoAwayAfter(usize),
    /// On the first connection, accept no stream: wait `UNPROCESSED_WAIT` so
    /// the client's streams are all on the wire, then send a GOAWAY with
    /// `INTERNAL_ERROR` and last-stream-id 0. Every stream the client sent
    /// is one the server did not process.
    AbruptUnprocessed,
    /// As `AbruptUnprocessed`, but the GOAWAY carries `NO_ERROR`: every stream
    /// the client sent, open or pending-open, is excluded and replayable.
    AbruptNoError,
    /// Close the connection as soon as a request arrived.
    DropAfterRequest,
    /// On the first request, send a graceful GOAWAY that does not exclude it,
    /// flush it, then close the connection without answering.
    GoAwayThenDrop,
    /// Refuse each request's first `times` attempts with `REFUSED_STREAM`,
    /// after `delay`.
    Refuse { times: usize, delay: Duration },
}

const ANSWER_DELAY: Duration = Duration::from_millis(20);

/// How long `Policy::AbruptUnprocessed` lets the client's burst reach the
/// server before the GOAWAY.
const UNPROCESSED_WAIT: Duration = Duration::from_millis(300);

struct State {
    policy: Policy,
    connections: AtomicUsize,
    /// Request id to the `connect-timeout-ms` each attempt carried.
    attempts: Mutex<HashMap<String, Vec<Option<String>>>>,
}

impl State {
    fn record<B>(&self, request: &Request<B>) -> usize {
        let header = |name: &str| {
            request
                .headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
        };
        let id = header("x-request-id").unwrap_or_default();
        let mut attempts = self.attempts.lock().expect("attempts");
        let seen = attempts.entry(id).or_default();
        seen.push(header("connect-timeout-ms"));
        seen.len()
    }
}

struct Server {
    addr: SocketAddr,
    state: Arc<State>,
}

impl Server {
    async fn start(policy: Policy) -> Self {
        Self::start_with(policy, None).await
    }

    async fn start_with(policy: Policy, tls: Option<tokio_rustls::TlsAcceptor>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let state = Arc::new(State {
            policy,
            connections: AtomicUsize::new(0),
            attempts: Mutex::default(),
        });
        let accepting = Arc::clone(&state);
        tokio::spawn(async move {
            while let Ok((tcp, _)) = listener.accept().await {
                let index = accepting.connections.fetch_add(1, Ordering::SeqCst);
                let state = Arc::clone(&accepting);
                match tls.clone() {
                    None => drop(tokio::spawn(serve(tcp, index, state))),
                    Some(acceptor) => drop(tokio::spawn(async move {
                        let tls = acceptor.accept(tcp).await.expect("tls handshake");
                        assert_eq!(tls.get_ref().1.alpn_protocol(), Some(&b"h2"[..]));
                        serve(tls, index, state).await;
                    })),
                }
            }
        });
        Self { addr, state }
    }

    fn uri(&self, scheme: &str) -> String {
        let host = if scheme == "https" {
            "localhost"
        } else {
            "127.0.0.1"
        };
        format!("{scheme}://{host}:{}/test.v1.Echo/Call", self.addr.port())
    }

    fn connections(&self) -> usize {
        self.state.connections.load(Ordering::SeqCst)
    }

    fn attempts(&self, id: &str) -> Vec<Option<String>> {
        self.state
            .attempts
            .lock()
            .expect("attempts")
            .get(id)
            .cloned()
            .unwrap_or_default()
    }

    fn max_attempts(&self) -> usize {
        let attempts = self.state.attempts.lock().expect("attempts");
        attempts.values().map(Vec::len).max().unwrap_or(0)
    }
}

async fn serve<T>(io: T, index: usize, state: Arc<State>)
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let Ok(mut connection) = h2::server::Builder::new()
        .max_concurrent_streams(MAX_STREAMS)
        .handshake::<_, Bytes>(io)
        .await
    else {
        return;
    };
    if index == 0
        && matches!(
            state.policy,
            Policy::AbruptUnprocessed | Policy::AbruptNoError
        )
    {
        tokio::time::sleep(UNPROCESSED_WAIT).await;
        let reason = if matches!(state.policy, Policy::AbruptNoError) {
            Reason::NO_ERROR
        } else {
            Reason::INTERNAL_ERROR
        };
        connection.abrupt_shutdown(reason);
    }
    let mut streams = 0;
    while let Some(Ok((request, mut respond))) = connection.accept().await {
        streams += 1;
        let attempt = state.record(&request);
        match state.policy {
            Policy::DropAfterRequest => return,
            Policy::GoAwayThenDrop => {
                connection.graceful_shutdown();
                // Polling `accept` flushes the GOAWAY frames before the close.
                drop(tokio::time::timeout(Duration::from_millis(200), connection.accept()).await);
                return;
            }
            Policy::Refuse { times, delay } if attempt <= times => {
                tokio::spawn(async move {
                    tokio::time::sleep(delay).await;
                    respond.send_reset(Reason::REFUSED_STREAM);
                    drop(request);
                });
                continue;
            }
            Policy::GoAwayAfter(after) if index == 0 && streams == after => {
                connection.graceful_shutdown();
            }
            _ => {}
        }
        tokio::spawn(async move {
            tokio::time::sleep(ANSWER_DELAY).await;
            let response = Response::builder().status(200).body(()).expect("response");
            let _sent = respond.send_response(response, true);
            drop(request);
        });
    }
}

fn request(uri: &str, id: &str, timeout_ms: Option<u64>) -> Request<ClientBody> {
    request_with(uri, id, timeout_ms, full_body(Bytes::from_static(b"{}")))
}

fn request_with(
    uri: &str,
    id: &str,
    timeout_ms: Option<u64>,
    body: ClientBody,
) -> Request<ClientBody> {
    let mut builder = Request::post(uri)
        .header("content-type", "application/json")
        .header("x-request-id", id);
    if let Some(ms) = timeout_ms {
        builder = builder.header("connect-timeout-ms", ms);
    }
    builder.body(body).expect("request")
}

async fn call(
    transport: &HttpTransport,
    request: Request<ClientBody>,
) -> Result<u16, ConnectError> {
    let response = transport.send(request).await?;
    let status = response.status().as_u16();
    response
        .into_body()
        .collect()
        .await
        .map_err(|e| ConnectError::internal(e.to_string()))?;
    Ok(status)
}

async fn burst(
    transport: &HttpTransport,
    uri: &str,
    calls: usize,
) -> Vec<Result<u16, ConnectError>> {
    burst_with(transport, uri, calls, || {
        full_body(Bytes::from_static(b"{}"))
    })
    .await
}

/// A burst whose request bodies come from `body`.
async fn burst_with(
    transport: &HttpTransport,
    uri: &str,
    calls: usize,
    body: fn() -> ClientBody,
) -> Vec<Result<u16, ConnectError>> {
    tokio::time::timeout(
        Duration::from_secs(60),
        join_all((0..calls).map(|i| {
            let transport = transport.clone();
            let request = request_with(uri, &format!("r{i}"), None, body());
            async move { call(&transport, request).await }
        })),
    )
    .await
    .unwrap_or_else(|_| panic!("a burst of {calls} calls hung instead of completing"))
}

fn all_ok(results: &[Result<u16, ConnectError>]) {
    for (i, result) in results.iter().enumerate() {
        assert_eq!(result.as_ref().ok(), Some(&200), "call r{i}: {result:?}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cold_burst_of_300_calls_shares_one_connection() {
    let server = Server::start(Policy::Answer).await;
    let results = burst(&HttpTransport::h2c(), &server.uri("http"), 300).await;
    all_ok(&results);
    assert_eq!(server.connections(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cold_tls_burst_shares_one_connection_negotiated_as_h2() {
    let (acceptor, client) = tls_pair();
    let server = Server::start_with(Policy::Answer, Some(acceptor)).await;
    let results = burst(&HttpTransport::fleet(client), &server.uri("https"), 100).await;
    all_ok(&results);
    assert_eq!(server.connections(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn five_hundred_calls_over_a_200_stream_limit_all_succeed() {
    let server = Server::start(Policy::Answer).await;
    let results = burst(&HttpTransport::h2c(), &server.uri("http"), 500).await;
    all_ok(&results);
    assert_eq!(server.connections(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_goaway_mid_burst_leaves_every_call_handled_exactly_once() {
    let server = Server::start(Policy::GoAwayAfter(100)).await;
    let results = burst(&HttpTransport::h2c(), &server.uri("http"), 300).await;
    all_ok(&results);
    for i in 0..300 {
        assert_eq!(
            server.attempts(&format!("r{i}")).len(),
            1,
            "r{i} handled once"
        );
    }
    assert_eq!(
        server.connections(),
        2,
        "one connection per peer at a time: the excluded calls share one fresh connection"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_goaway_mid_burst_of_empty_bodies_leaves_every_call_handled_exactly_once() {
    // An empty body ends the stream with its HEADERS frame, so a stream the
    // server accepted has already run when the GOAWAY arrives.
    let server = Server::start(Policy::GoAwayAfter(100)).await;
    let results = burst_with(&HttpTransport::h2c(), &server.uri("http"), 300, || {
        full_body(Bytes::new())
    })
    .await;
    all_ok(&results);
    for i in 0..300 {
        assert_eq!(
            server.attempts(&format!("r{i}")).len(),
            1,
            "r{i} handled once"
        );
    }
    assert_eq!(
        server.connections(),
        2,
        "one connection per peer at a time: a single GOAWAY costs one fresh connection"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_goaway_below_the_open_and_pending_open_streams_replays_each_exactly_once() {
    // 300 calls over a 200-stream limit: 200 streams are open, one is
    // pending-open and the rest wait for the right to open. The GOAWAY
    // excludes them all.
    const CALLS: usize = 300;
    let server = Server::start(Policy::AbruptNoError).await;
    let results = burst(&HttpTransport::h2c(), &server.uri("http"), CALLS).await;
    all_ok(&results);
    for i in 0..CALLS {
        assert_eq!(
            server.attempts(&format!("r{i}")).len(),
            1,
            "r{i} reached the server once"
        );
    }
    assert_eq!(
        server.connections(),
        2,
        "one connection per peer at a time: a single GOAWAY costs one fresh connection"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stream_a_goaway_did_not_exclude_is_not_replayed_when_the_connection_ends() {
    let server = Server::start(Policy::GoAwayThenDrop).await;
    let result = call(
        &HttpTransport::h2c(),
        request(&server.uri("http"), "included", None),
    )
    .await;
    assert!(result.is_err());
    assert_eq!(server.attempts("included").len(), 1);
    assert_eq!(server.connections(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_error_coded_goaway_is_not_replayed() {
    const CALLS: usize = 50;
    let server = Server::start(Policy::AbruptUnprocessed).await;
    let results = burst(&HttpTransport::h2c(), &server.uri("http"), CALLS).await;

    // The first connection processed no stream, so the streams the client put
    // on it are the ones the GOAWAY excluded: the server never saw them.
    let unseen: Vec<usize> = (0..CALLS)
        .filter(|i| server.attempts(&format!("r{i}")).is_empty())
        .collect();
    assert!(
        !unseen.is_empty(),
        "the GOAWAY excluded streams the server never saw"
    );
    for i in &unseen {
        assert!(
            results[*i].is_err(),
            "r{i} was excluded by an error-coded GOAWAY and must not be replayed: {:?}",
            results[*i]
        );
    }
    assert!(
        server.max_attempts() <= 1,
        "no request reached the server twice"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_request_sent_then_cut_off_is_not_replayed() {
    let server = Server::start(Policy::DropAfterRequest).await;
    let result = call(
        &HttpTransport::h2c(),
        request(&server.uri("http"), "cut", None),
    )
    .await;
    assert!(result.is_err());
    assert_eq!(server.attempts("cut").len(), 1);
    assert_eq!(server.connections(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refused_stream_is_replayed_once_with_a_smaller_timeout() {
    let server = Server::start(Policy::Refuse {
        times: 1,
        delay: Duration::from_millis(50),
    })
    .await;
    let transport = HttpTransport::h2c();
    let result = call(
        &transport,
        request(&server.uri("http"), "once", Some(5_000)),
    )
    .await;
    assert_eq!(result.ok(), Some(200));
    let attempts = server.attempts("once");
    assert_eq!(attempts.len(), 2);
    let stated = |i: usize| -> u64 {
        attempts[i]
            .as_deref()
            .expect("timeout header")
            .parse()
            .expect("ms")
    };
    assert_eq!(stated(0), 5_000);
    assert!(
        stated(1) < stated(0),
        "the replay states the time remaining"
    );
    assert_eq!(
        server.connections(),
        2,
        "the replay went to a fresh connection"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stream_refused_twice_fails_after_exactly_two_attempts() {
    let server = Server::start(Policy::Refuse {
        times: 2,
        delay: Duration::ZERO,
    })
    .await;
    let result = call(
        &HttpTransport::h2c(),
        request(&server.uri("http"), "twice", None),
    )
    .await;
    assert!(result.is_err());
    assert_eq!(server.attempts("twice").len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn no_replay_past_the_deadline() {
    let server = Server::start(Policy::Refuse {
        times: 1,
        delay: Duration::from_millis(120),
    })
    .await;
    let result = call(
        &HttpTransport::h2c(),
        request(&server.uri("http"), "late", Some(60)),
    )
    .await;
    assert!(result.is_err());
    assert_eq!(server.attempts("late").len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_streaming_body_is_never_replayed() {
    let server = Server::start(Policy::Refuse {
        times: 1,
        delay: Duration::ZERO,
    })
    .await;
    let frames = futures_util::stream::iter(vec![Ok::<_, ConnectError>(http_body::Frame::data(
        Bytes::from_static(b"{}"),
    ))]);
    let body = http_body_util::StreamBody::new(frames).boxed();
    let result = call(
        &HttpTransport::h2c(),
        request_with(&server.uri("http"), "stream", None, body),
    )
    .await;
    assert!(result.is_err());
    assert_eq!(server.attempts("stream").len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_http1_only_server_fails_cleanly() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let service = hyper::service::service_fn(|_request| async {
                Ok::<_, std::convert::Infallible>(Response::new(
                    http_body_util::Empty::<Bytes>::new(),
                ))
            });
            tokio::spawn(
                hyper::server::conn::http1::Builder::new()
                    .serve_connection(hyper_util::rt::TokioIo::new(tcp), service),
            );
        }
    });
    let uri = format!("http://127.0.0.1:{}/test.v1.Echo/Call", addr.port());
    let outcome = tokio::time::timeout(
        Duration::from_secs(10),
        call(&HttpTransport::h2c(), request(&uri, "h1", None)),
    )
    .await;
    assert!(
        matches!(outcome, Ok(Err(_))),
        "fails, and does not hang: {outcome:?}"
    );
}

/// What a TLS listener saw from a client: the first byte on the wire, and the
/// application data after the handshake.
#[derive(Default)]
struct Seen {
    first_wire_bytes: Mutex<Vec<u8>>,
    application_data: Mutex<Vec<u8>>,
}

/// A TLS listener that completes the handshakes its ALPN offer allows, records
/// what the client sends for up to 500 ms, then closes. It never answers.
async fn spy(acceptor: tokio_rustls::TlsAcceptor) -> (SocketAddr, Arc<Seen>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let seen = Arc::new(Seen::default());
    let recording = Arc::clone(&seen);
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let (acceptor, seen) = (acceptor.clone(), Arc::clone(&recording));
            tokio::spawn(async move {
                let mut head = [0_u8; 5];
                let peeked = tcp.peek(&mut head).await.unwrap_or(0);
                seen.first_wire_bytes
                    .lock()
                    .expect("seen")
                    .extend_from_slice(&head[..peeked]);
                let Ok(mut tls) = acceptor.accept(tcp).await else {
                    return;
                };
                let mut buf = [0_u8; 4096];
                while let Ok(Ok(n)) =
                    tokio::time::timeout(Duration::from_millis(500), tls.read(&mut buf)).await
                {
                    if n == 0 {
                        break;
                    }
                    seen.application_data
                        .lock()
                        .expect("seen")
                        .extend_from_slice(&buf[..n]);
                }
            });
        }
    });
    (addr, seen)
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// A call through `fleet` to a TLS server offering `alpn` fails in bounded
/// time, and the server saw a TLS handshake and no HTTP/1.1 request.
async fn assert_fails_without_http1(alpn: Vec<Vec<u8>>) {
    let (acceptor, client) = tls_pair_offering(alpn);
    let (addr, seen) = spy(acceptor).await;
    let uri = format!("https://localhost:{}/test.v1.Echo/Call", addr.port());
    let outcome = tokio::time::timeout(
        Duration::from_secs(10),
        call(&HttpTransport::fleet(client), request(&uri, "alpn", None)),
    )
    .await;
    assert!(
        matches!(outcome, Ok(Err(_))),
        "fails, and does not hang: {outcome:?}"
    );

    let wire = seen.first_wire_bytes.lock().expect("seen").clone();
    assert_eq!(
        wire.first(),
        Some(&0x16),
        "the wire starts with a TLS handshake"
    );
    assert!(!contains(&wire, b"POST ") && !contains(&wire, b"HTTP/"));
    let data = seen.application_data.lock().expect("seen").clone();
    assert!(
        !contains(&data, b"HTTP/1.") && !contains(&data, b"POST "),
        "no HTTP/1.1 request reached the server: {data:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tls_server_offering_only_http1_fails_cleanly() {
    assert_fails_without_http1(vec![b"http/1.1".to_vec()]).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tls_server_offering_no_alpn_fails_cleanly() {
    assert_fails_without_http1(Vec::new()).await;
}

#[test]
fn a_connection_outlives_the_runtime_that_opened_it() {
    let host = tokio::runtime::Runtime::new().expect("server runtime");
    let server = host.block_on(Server::start(Policy::Answer));
    let uri = server.uri("http");
    let transport = HttpTransport::h2c();

    let opener = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("opening runtime");
    let first = opener.block_on(call(&transport, request(&uri, "first", None)));
    assert_eq!(first.ok(), Some(200));
    drop(opener);

    let other = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("other runtime");
    let second = other.block_on(call(&transport, request(&uri, "second", None)));
    assert_eq!(second.ok(), Some(200));
    assert_eq!(
        server.connections(),
        1,
        "the call reused the first runtime's connection"
    );
}

/// A peer that accepts TCP connections and closes each at once, so every dial
/// fails at the HTTP/2 handshake. Returns its port and the accept count.
async fn hanging_up_peer() -> (u16, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let accepted = Arc::new(AtomicUsize::new(0));
    let counting = Arc::clone(&accepted);
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            counting.fetch_add(1, Ordering::SeqCst);
            drop(tcp);
        }
    });
    (port, accepted)
}

// One thread polls every call up to its wait for the peer's slot before the
// dial can report back, so all of them queue behind the first dial.
#[tokio::test]
async fn a_failed_dial_is_shared_with_every_caller_queued_behind_it() {
    let (port, accepted) = hanging_up_peer().await;
    // The peer hangs up before any TLS handshake, so the dial itself fails:
    // over cleartext the h2 preface is only written, and a dial could succeed.
    let (_acceptor, client) = tls_pair();
    let uri = format!("https://localhost:{port}/test.v1.Echo/Call");
    let transport = HttpTransport::fleet(client);

    let results = burst(&transport, &uri, 20).await;
    for result in &results {
        let error = result.as_ref().expect_err("the peer hangs up");
        assert!(failed_to_connect(error), "{error:?}");
        assert_eq!(never_ran(error), None, "a connect failure is not replayed");
    }
    assert_eq!(
        accepted.load(Ordering::SeqCst),
        1,
        "the burst dialed once, not once per caller"
    );

    let later = call(&transport, request(&uri, "later", None)).await;
    assert!(later.is_err());
    assert_eq!(
        accepted.load(Ordering::SeqCst),
        2,
        "a caller arriving after the failure dials afresh"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn dropping_the_transport_closes_its_connections() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let uri = format!(
        "http://127.0.0.1:{}/test.v1.Echo/Call",
        listener.local_addr().expect("addr").port()
    );
    let (closed_tx, closed_rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.expect("accept");
        let mut connection = h2::server::handshake(tcp).await.expect("handshake");
        while let Some(Ok((_request, mut respond))) = connection.accept().await {
            let response = Response::builder().status(200).body(()).expect("response");
            drop(respond.send_response(response, true));
        }
        closed_tx.send(()).ok();
    });

    let transport = HttpTransport::h2c();
    assert_eq!(
        call(&transport, request(&uri, "owned", None)).await.ok(),
        Some(200)
    );
    drop(transport);
    tokio::time::timeout(Duration::from_secs(5), closed_rx)
        .await
        .expect("the connection closed long before the idle timeout")
        .expect("server task");
}

/// A TLS acceptor offering `h2` for `localhost` (see [`tls_pair_offering`]), and a client config trusting
/// it with no ALPN of its own (the transport sets it).
fn tls_pair() -> (tokio_rustls::TlsAcceptor, Arc<rustls::ClientConfig>) {
    tls_pair_offering(vec![b"h2".to_vec()])
}

/// As [`tls_pair`], with the server offering `alpn` (none when empty).
fn tls_pair_offering(alpn: Vec<Vec<u8>>) -> (tokio_rustls::TlsAcceptor, Arc<rustls::ClientConfig>) {
    let certified = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()]).expect("cert");
    let cert = certified.cert.der().clone();
    let key = rustls::pki_types::PrivatePkcs8KeyDer::from(certified.key_pair.serialize_der());
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());

    let mut server = rustls::ServerConfig::builder_with_provider(Arc::clone(&provider))
        .with_safe_default_protocol_versions()
        .expect("versions")
        .with_no_client_auth()
        .with_single_cert(vec![cert.clone()], key.into())
        .expect("server config");
    server.alpn_protocols = alpn;

    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert).expect("root");
    let client = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
    (
        tokio_rustls::TlsAcceptor::from(Arc::new(server)),
        Arc::new(client),
    )
}
