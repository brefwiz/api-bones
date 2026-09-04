// SPDX-License-Identifier: MIT
#![cfg(all(feature = "connect", feature = "axum"))]
//! Wire-level proof that a scoped handle carries its own credential.
//!
//! The unit tests in `connect::scoped` assert the handle's call options. These
//! assert what a real server actually receives, over a real connection pool,
//! with two scoped calls in flight at the same time — the situation a
//! process-wide injector cannot answer correctly.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use api_bones::connect::{CallCredential, ScopeCall, ScopedClient};
use axum::extract::{ConnectInfo, State};
use axum::http::HeaderMap;
use buffa_types::google::protobuf::{Empty, EmptyView};
use connectrpc::client::{ClientConfig, HttpClient, call_unary};
use connectrpc::{Spec, StreamType};

/// connectrpc 0.9 takes the whole procedure path as one `&'static str`, so the
/// service and method are no longer separate arguments. The test server still
/// keys what it saw on the last path segment, so the method names are unchanged.
const fn probe_spec(procedure: &'static str) -> Spec {
    Spec::client(procedure, StreamType::Unary)
}

/// What the server saw for one request.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Observed {
    authorization: Option<String>,
    peer: SocketAddr,
}

#[derive(Clone)]
struct Recorder {
    seen: Arc<Mutex<BTreeMap<String, Observed>>>,
    /// Held open until every concurrent request has arrived, so the requests
    /// genuinely overlap instead of being serialised by the server.
    gate: Option<Arc<tokio::sync::Barrier>>,
}

impl Recorder {
    fn new(gate: Option<Arc<tokio::sync::Barrier>>) -> Self {
        Self {
            seen: Arc::new(Mutex::new(BTreeMap::new())),
            gate,
        }
    }

    fn snapshot(&self) -> BTreeMap<String, Observed> {
        self.seen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

async fn record(
    State(recorder): State<Recorder>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    uri: axum::http::Uri,
    headers: HeaderMap,
) -> ([(axum::http::HeaderName, &'static str); 1], Vec<u8>) {
    let method = uri.path().rsplit('/').next().unwrap_or_default().to_owned();
    let authorization = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    recorder
        .seen
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(
            method,
            Observed {
                authorization,
                peer,
            },
        );

    if let Some(gate) = recorder.gate.as_ref() {
        gate.wait().await;
    }

    // Connect unary: 200 with the bare encoded message. `Empty` is zero bytes.
    (
        [(axum::http::header::CONTENT_TYPE, "application/proto")],
        Vec::new(),
    )
}

async fn start_server(recorder: Recorder) -> SocketAddr {
    let app = axum::Router::new()
        .fallback(axum::routing::post(record))
        .with_state(recorder);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    addr
}

/// Stands in for a generated Connect client: one transport, one connection
/// pool, one process-wide credential, plus a per-call scoped handle.
struct Client {
    transport: HttpClient,
    config: ClientConfig,
    auth: CallCredential,
}

impl ScopeCall for Client {}

impl Client {
    fn new(addr: SocketAddr, auth: CallCredential) -> Self {
        let uri: axum::http::Uri = format!("http://{addr}").parse().unwrap();
        Self {
            transport: HttpClient::plaintext(),
            config: ClientConfig::new(uri),
            auth,
        }
    }

    /// A call on the process's own identity.
    async fn probe(&self, spec: Spec) {
        call_unary::<_, Empty, EmptyView>(
            &self.transport,
            &self.config,
            spec,
            Empty::default(),
            self.auth.call_options(),
        )
        .await
        .unwrap();
    }

    /// A call on a credential that belongs to this call and no other.
    fn with_claim(&self, claim: &str) -> Scoped<'_> {
        Scoped(self.with_credential(claim))
    }
}

struct Scoped<'a>(ScopedClient<'a, Client>);

impl Scoped<'_> {
    async fn probe(&self, spec: Spec) {
        let client = self.0.client();
        call_unary::<_, Empty, EmptyView>(
            &client.transport,
            &client.config,
            spec,
            Empty::default(),
            self.0.call_options(),
        )
        .await
        .unwrap();
    }
}

/// The race the scoped handle exists to prevent: two provisioning calls for
/// different principals, in flight together on one client, each authorized as
/// itself.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_scoped_calls_reach_the_server_with_their_own_credential() {
    let recorder = Recorder::new(Some(Arc::new(tokio::sync::Barrier::new(2))));
    let addr = start_server(recorder.clone()).await;
    let client = Client::new(addr, CallCredential::bearer("process-identity"));

    let first = client.with_claim("claim-for-first");
    let second = client.with_claim("claim-for-second");
    tokio::join!(
        first.probe(probe_spec("/scoped.v1.Probe/First")),
        second.probe(probe_spec("/scoped.v1.Probe/Second"))
    );

    let seen = recorder.snapshot();
    assert_eq!(
        seen.get("First").and_then(|o| o.authorization.clone()),
        Some("Bearer claim-for-first".to_owned()),
        "server saw: {seen:?}"
    );
    assert_eq!(
        seen.get("Second").and_then(|o| o.authorization.clone()),
        Some("Bearer claim-for-second".to_owned()),
        "server saw: {seen:?}"
    );
}

/// Scoping must not cost a connection: successive calls under different
/// credentials come back on the same pooled connection, and an unscoped call
/// still presents the process identity.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scoped_calls_share_the_parent_connection_pool() {
    let recorder = Recorder::new(None);
    let addr = start_server(recorder.clone()).await;
    let client = Client::new(addr, CallCredential::bearer("process-identity"));

    client
        .with_claim("claim-one")
        .probe(probe_spec("/scoped.v1.Probe/One"))
        .await;
    client
        .with_claim("claim-two")
        .probe(probe_spec("/scoped.v1.Probe/Two"))
        .await;
    client.probe(probe_spec("/scoped.v1.Probe/Unscoped")).await;

    let seen = recorder.snapshot();
    assert_eq!(
        seen.get("One").and_then(|o| o.authorization.clone()),
        Some("Bearer claim-one".to_owned())
    );
    assert_eq!(
        seen.get("Two").and_then(|o| o.authorization.clone()),
        Some("Bearer claim-two".to_owned())
    );
    assert_eq!(
        seen.get("Unscoped").and_then(|o| o.authorization.clone()),
        Some("Bearer process-identity".to_owned())
    );

    let connections: std::collections::BTreeSet<SocketAddr> =
        seen.values().map(|o| o.peer).collect();
    assert_eq!(
        connections.len(),
        1,
        "scoping opened a second connection: {connections:?}"
    );
}
