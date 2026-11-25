// SPDX-License-Identifier: MIT
#![cfg(feature = "connect")]
//! The transport's replay of a request that never ran is not a retry attempt.
//!
//! `fetch_with_retry` runs over the real `HttpTransport` against an HTTP/2
//! peer that refuses a stream with `REFUSED_STREAM`, which proves the request
//! never ran. The transport replays it on a fresh connection inside the same
//! fetch, so the retry loop sees one fetch that answered. The replay must
//! leave the loop's whole attempt count to later answers: a throttle keeps its
//! three repeats and an `Unavailable` keeps the caller's `retries`.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use api_bones::connect::fetch_with_retry;
use api_bones_transport::HttpTransport;
use bytes::Bytes;
use connectrpc::client::{ClientTransport, full_body};
use connectrpc::{ConnectError, ErrorCode};
use h2::Reason;
use http::header::RETRY_AFTER;
use http::{HeaderMap, HeaderValue, Request, Response, StatusCode};
use http_body_util::BodyExt as _;
use tokio::net::{TcpListener, TcpStream};

/// What the peer does with the nth request that reaches the wire, replays
/// included.
#[derive(Clone, Copy)]
enum Answer {
    /// Reset the stream with `REFUSED_STREAM`: the request never ran.
    Refuse,
    /// Answer with this status; a 429 names a `retry-after` of zero.
    Reply(StatusCode),
}

struct Peer {
    port: u16,
    script: Vec<Answer>,
    sent: AtomicUsize,
    connections: AtomicUsize,
    /// The connection each request reached the wire on, in arrival order.
    arrivals: Mutex<Vec<usize>>,
}

impl Peer {
    async fn start(script: Vec<Answer>) -> Arc<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let peer = Arc::new(Self {
            port,
            script,
            sent: AtomicUsize::new(0),
            connections: AtomicUsize::new(0),
            arrivals: Mutex::default(),
        });
        let accepting = Arc::clone(&peer);
        tokio::spawn(async move {
            while let Ok((tcp, _)) = listener.accept().await {
                let index = accepting.connections.fetch_add(1, Ordering::SeqCst);
                drop(tokio::spawn(serve(tcp, index, Arc::clone(&accepting))));
            }
        });
        peer
    }

    fn uri(&self) -> String {
        format!("http://127.0.0.1:{}/test.v1.Echo/Call", self.port)
    }

    fn sent(&self) -> usize {
        self.sent.load(Ordering::SeqCst)
    }

    fn connections(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }

    fn arrivals(&self) -> Vec<usize> {
        self.arrivals.lock().expect("arrivals").clone()
    }
}

async fn serve(tcp: TcpStream, index: usize, peer: Arc<Peer>) {
    let Ok(mut connection) = h2::server::handshake(tcp).await else {
        return;
    };
    while let Some(Ok((request, mut respond))) = connection.accept().await {
        let position = peer.sent.fetch_add(1, Ordering::SeqCst);
        peer.arrivals.lock().expect("arrivals").push(index);
        match peer
            .script
            .get(position)
            .copied()
            .unwrap_or(Answer::Reply(StatusCode::OK))
        {
            Answer::Refuse => respond.send_reset(Reason::REFUSED_STREAM),
            Answer::Reply(status) => {
                let mut builder = Response::builder().status(status);
                if status == StatusCode::TOO_MANY_REQUESTS {
                    builder = builder.header(RETRY_AFTER, "0");
                }
                let _sent = respond.send_response(builder.body(()).expect("response"), true);
            }
        }
        drop(request);
    }
}

/// One call over the transport, as a generated client makes it: a throttle
/// and an unavailable peer come back as the Connect errors the retry loop
/// reads.
async fn send(transport: HttpTransport, uri: String) -> Result<(), ConnectError> {
    let request = Request::post(uri)
        .header("content-type", "application/json")
        .body(full_body(Bytes::from_static(b"{}")))
        .expect("request");
    let response = transport.send(request).await?;
    let status = response.status();
    let retry_after = response.headers().get(RETRY_AFTER).cloned();
    response
        .into_body()
        .collect()
        .await
        .map_err(|e| ConnectError::internal(e.to_string()))?;
    match status {
        StatusCode::OK => Ok(()),
        StatusCode::TOO_MANY_REQUESTS => {
            let mut headers = HeaderMap::new();
            headers.insert(
                RETRY_AFTER,
                retry_after.unwrap_or_else(|| HeaderValue::from_static("0")),
            );
            Err(
                ConnectError::new(ErrorCode::ResourceExhausted, "limit reached")
                    .with_headers(headers),
            )
        }
        _ => Err(ConnectError::unavailable("draining")),
    }
}

/// Runs `fetch_with_retry` with `retries` over the real transport against a
/// peer following `script`. Returns the outcome, how many fetches the loop
/// made, and the peer.
async fn run(script: Vec<Answer>, retries: u8) -> (Result<(), ConnectError>, usize, Arc<Peer>) {
    let peer = Peer::start(script).await;
    let transport = HttpTransport::h2c();
    let uri = peer.uri();
    let fetches = AtomicUsize::new(0);
    let outcome = tokio::time::timeout(
        Duration::from_secs(30),
        fetch_with_retry(
            || {
                fetches.fetch_add(1, Ordering::SeqCst);
                send(transport.clone(), uri.clone())
            },
            retries,
            None,
        ),
    )
    .await
    .expect("the call hung instead of completing");
    (outcome, fetches.load(Ordering::SeqCst), peer)
}

/// The first request was refused and sent again on a fresh connection.
fn assert_replayed(peer: &Peer) {
    let arrivals = peer.arrivals();
    assert!(arrivals.len() >= 2, "no replay reached the wire");
    assert_ne!(
        arrivals[0], arrivals[1],
        "the replay went to the connection that refused it"
    );
    assert_eq!(peer.connections(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_transport_replay_leaves_a_throttle_its_three_repeats() {
    use Answer::{Refuse, Reply};
    let throttled = Reply(StatusCode::TOO_MANY_REQUESTS);
    let served = Reply(StatusCode::OK);

    // The refused first request is replayed inside fetch one, and the three
    // throttles that follow are still repeated until the call is served.
    let (outcome, fetches, peer) =
        run(vec![Refuse, throttled, throttled, throttled, served], 0).await;
    outcome.expect("served after three throttle repeats");
    assert_eq!(fetches, 4);
    assert_eq!(peer.sent(), 5);
    assert_replayed(&peer);

    // The replay grants no repeat either: a fourth throttle ends the call.
    let script = vec![Refuse, throttled, throttled, throttled, throttled, served];
    let (outcome, fetches, peer) = run(script, 0).await;
    assert_eq!(
        outcome.expect_err("repeats spent").code,
        ErrorCode::ResourceExhausted
    );
    assert_eq!(fetches, 4);
    assert_eq!(peer.sent(), 5);
    assert_replayed(&peer);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_transport_replay_leaves_the_blind_retries_their_count() {
    use Answer::{Refuse, Reply};
    let unavailable = Reply(StatusCode::SERVICE_UNAVAILABLE);
    let served = Reply(StatusCode::OK);

    let (outcome, fetches, peer) = run(vec![Refuse, unavailable, unavailable, served], 2).await;
    outcome.expect("served after two blind retries");
    assert_eq!(fetches, 3);
    assert_eq!(peer.sent(), 4);
    assert_replayed(&peer);

    // The replay grants no retry either: a third unavailable ends the call.
    let script = vec![Refuse, unavailable, unavailable, unavailable, served];
    let (outcome, fetches, peer) = run(script, 2).await;
    assert_eq!(
        outcome.expect_err("retries spent").code,
        ErrorCode::Unavailable
    );
    assert_eq!(fetches, 3);
    assert_eq!(peer.sent(), 4);
    assert_replayed(&peer);
}
