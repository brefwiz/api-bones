// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! The Cucumber World for the shared contract lane.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use api_bones_connect::{Idempotency, PreconditionedTransport, index_generated_policy};
use connectrpc::ConnectError;
use connectrpc::client::{BoxFuture, ClientBody, ClientTransport};
use cucumber::World;
use http::{Request, Response};
use http_body_util::Empty;

#[derive(Debug, Default, World)]
pub struct RetryWorld {
    pub failure: Option<ConnectError>,
}

impl RetryWorld {
    /// The failure under test, or a panic naming the missing Given.
    pub fn failure(&self) -> &ConnectError {
        self.failure.as_ref().expect("no failure given")
    }
}

const PRECONDITION_METHOD: &str = "/pkg.v1.Svc/Method";

#[derive(Clone)]
struct RecordingTransport {
    seen: Arc<Mutex<Vec<http::HeaderMap>>>,
}

impl ClientTransport for RecordingTransport {
    type ResponseBody = Empty<bytes::Bytes>;
    type Error = ConnectError;

    fn send(
        &self,
        request: Request<ClientBody>,
    ) -> BoxFuture<'static, Result<Response<Self::ResponseBody>, Self::Error>> {
        self.seen.lock().unwrap().push(request.headers().clone());
        Box::pin(async { Ok(Response::new(Empty::new())) })
    }
}

#[derive(Debug, Default, World)]
pub struct PreconditionWorld {
    policy: Option<HashMap<String, Idempotency>>,
    caller_header: Option<String>,
    seen: Option<Arc<Mutex<Vec<http::HeaderMap>>>>,
    document: Option<String>,
    refusal: Option<String>,
}

impl PreconditionWorld {
    pub fn declare_policy_document(&mut self, version: u32, idempotency: &str) {
        let document = format!(
            r#"{{"schemaVersion": {version}, "methods": [{{
                "rpc": "{PRECONDITION_METHOD}",
                "procedure": "unary", "idempotency": "{idempotency}",
                "browserCache": {{"scope": "NO_STORE", "maxAgeSeconds": 0}},
                "sensitivity": "SENSITIVE", "maxEncodedUrlBytes": 4096
            }}]}}"#
        );
        self.policy = Some(index_generated_policy(&document).expect("supported schema version"));
    }

    /// A policy document whose `schemaVersion` is the given JSON literal, or
    /// absent.
    pub fn declare_raw_schema_version(&mut self, declared: &str) {
        let key = if declared == "absent" {
            String::new()
        } else {
            format!(r#""schemaVersion": {declared}, "#)
        };
        self.document = Some(format!(r#"{{{key}"methods": []}}"#));
    }

    pub fn build_client(&mut self) {
        let document = self.document.take().expect("no policy document given");
        self.refusal = index_generated_policy(&document)
            .err()
            .map(|e| e.to_string());
    }

    pub fn refusal(&self) -> Option<&str> {
        self.refusal.as_deref()
    }

    pub fn declare_method(&mut self, idempotency: Idempotency) {
        let mut policy = HashMap::new();
        policy.insert(PRECONDITION_METHOD.to_owned(), idempotency);
        self.policy = Some(policy);
    }

    pub fn declare_no_policy(&mut self) {
        self.policy = Some(HashMap::new());
    }

    pub fn set_caller_header(&mut self, value: &str) {
        self.caller_header = if value == "none" {
            None
        } else {
            Some(value.to_owned())
        };
    }

    pub async fn send(&mut self) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        self.seen = Some(seen.clone());
        let transport = PreconditionedTransport::new(
            RecordingTransport { seen },
            self.policy.take().expect("no method declared"),
        );
        let mut builder = Request::builder()
            .method("POST")
            .uri(format!("https://svc{PRECONDITION_METHOD}"));
        if let Some(value) = &self.caller_header {
            builder = builder.header("if-match", value.as_str());
        }
        let request = builder
            .body(connectrpc::client::full_body(bytes::Bytes::new()))
            .expect("build request");
        transport.send(request).await.expect("transport send");
    }

    pub fn sent_if_match(&self) -> Option<String> {
        let seen = self.seen.as_ref().expect("no call sent yet");
        let headers = seen.lock().unwrap();
        headers[0]
            .get("if-match")
            .map(|v| v.to_str().unwrap().to_owned())
    }
}

/// The failure under test, and what the SDK error type read from it.
#[derive(Debug, Default, World)]
pub struct ErrorInfoWorld {
    pub failure: Option<ConnectError>,
    pub read: Option<ReadError>,
}

/// What an SDK error type built from the failure exposes.
#[derive(Debug)]
pub struct ReadError {
    pub code: Option<String>,
    pub emitter: Option<String>,
    pub text: String,
}

/// A refusal as the replay rule is given it.
#[derive(Debug, Default, World)]
pub struct CsrfReplayWorld {
    pub status: u16,
    pub refusal: Option<String>,
    pub sent: Option<String>,
}

/// A test certificate authority.
pub struct Authority {
    pub cert: rcgen::Certificate,
    pub key: rcgen::KeyPair,
}

impl std::fmt::Debug for Authority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Authority").finish_non_exhaustive()
    }
}

/// A loopback HTTP/2 server behind TLS that asks for, but does not require, a
/// client certificate and answers every request with 200.
pub struct LoopbackServer {
    pub addr: std::net::SocketAddr,
    /// Set only from inside the request handler, so it stays `None` for a
    /// server that never served a request.
    saw_client_cert: std::sync::Arc<std::sync::Mutex<Option<bool>>>,
    task: tokio::task::JoinHandle<()>,
}

impl LoopbackServer {
    /// Serve `cert` with `key`, trusting `client_ca` for any client certificate.
    pub async fn start(
        cert: connectrpc::rustls::pki_types::CertificateDer<'static>,
        key: connectrpc::rustls::pki_types::PrivateKeyDer<'static>,
        client_ca: connectrpc::rustls::pki_types::CertificateDer<'static>,
    ) -> Self {
        use std::sync::{Arc, Mutex};

        let mut roots = connectrpc::rustls::RootCertStore::empty();
        roots.add(client_ca).expect("trusted ca");
        let client_auth =
            connectrpc::rustls::server::WebPkiClientVerifier::builder(Arc::new(roots))
                .allow_unauthenticated()
                .build()
                .expect("client verifier");
        let mut config = connectrpc::rustls::ServerConfig::builder()
            .with_client_cert_verifier(client_auth)
            .with_single_cert(vec![cert], key)
            .expect("server config");
        config.alpn_protocols = vec![b"h2".to_vec()];
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let saw_client_cert: Arc<Mutex<Option<bool>>> = Arc::new(Mutex::new(None));
        let recorded = saw_client_cert.clone();
        let task = tokio::spawn(async move {
            while let Ok((tcp, _)) = listener.accept().await {
                let acceptor = acceptor.clone();
                let recorded = recorded.clone();
                tokio::spawn(async move {
                    let Ok(tls) = acceptor.accept(tcp).await else {
                        return;
                    };
                    let has_cert = tls.get_ref().1.peer_certificates().is_some();
                    let service = hyper::service::service_fn(move |_req| {
                        *recorded.lock().expect("record") = Some(has_cert);
                        async {
                            Ok::<_, std::convert::Infallible>(http::Response::new(
                                http_body_util::Full::new(bytes::Bytes::from_static(b"ok")),
                            ))
                        }
                    });
                    let _ = hyper::server::conn::http2::Builder::new(
                        hyper_util::rt::TokioExecutor::new(),
                    )
                    .serve_connection(hyper_util::rt::TokioIo::new(tls), service)
                    .await;
                });
            }
        });
        Self {
            addr,
            saw_client_cert,
            task,
        }
    }

    /// Whether the last request arrived with a client certificate; `None` when
    /// the server never served one.
    pub fn saw_client_cert(&self) -> Option<bool> {
        *self.saw_client_cert.lock().expect("read")
    }
}

impl std::fmt::Debug for LoopbackServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoopbackServer")
            .field("addr", &self.addr)
            .finish_non_exhaustive()
    }
}

impl Drop for LoopbackServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// A pinned caller and the loopback server it connects to.
#[derive(Debug, Default, World)]
pub struct PinnedPeerWorld {
    /// The CA the bundle names, made on first use.
    pub test_ca: Option<Authority>,
    /// A CA the bundle does not name, made on first use.
    pub stranger_ca: Option<Authority>,
    /// The trust bundle the caller pins against, PEM.
    pub bundle: String,
    /// The certificate and key the server presents.
    pub server: Option<(
        connectrpc::rustls::pki_types::CertificateDer<'static>,
        connectrpc::rustls::pki_types::PrivateKeyDer<'static>,
    )>,
    /// The server a caller connects to.
    pub loopback: Option<LoopbackServer>,
    /// How the one request ended, once a caller has connected.
    pub outcome: Option<Result<u16, connectrpc::ConnectError>>,
    /// How building a caller ended, once one has been built.
    pub built: Option<Result<(), api_bones_connect::PinnedPeerError>>,
}

/// The record under test, as the descriptor set it compiles to.
#[derive(Default, cucumber::World)]
pub struct FieldLabelsWorld {
    pub source: String,
    pub pool: Option<prost_reflect::DescriptorPool>,
}

impl std::fmt::Debug for FieldLabelsWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FieldLabelsWorld").finish_non_exhaustive()
    }
}

/// The stream control under test: the control a scenario declares, the frame
/// it was written into, and what the codec read back from that frame.
#[derive(Default, cucumber::World)]
pub struct StreamControlWorld {
    pub source: String,
    pub pool: Option<prost_reflect::DescriptorPool>,
    pub control: Option<api_bones_stream::StreamControl>,
    pub ordinary: Option<String>,
    pub ordinary_proto: Vec<u8>,
    pub frame: Vec<u8>,
    pub json_frame: bool,
    pub read_back:
        Option<Result<Option<api_bones_stream::StreamControl>, api_bones_stream::MalformedControl>>,
}

impl std::fmt::Debug for StreamControlWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamControlWorld").finish_non_exhaustive()
    }
}

/// The keepalive under test: the payload bytes a scenario holds, the scripted
/// server stream the reader reads, and what the reader got.
#[derive(Default, cucumber::World)]
pub struct StreamKeepaliveWorld {
    pub payload: Vec<u8>,
    pub fields: Option<buffa::UnknownFields>,
    pub verdict: Option<bool>,
    pub script: Vec<WatchItem>,
    pub keepalives: usize,
    pub received: Vec<String>,
    pub failure: Option<String>,
    pub options: Option<connectrpc::client::CallOptions>,
}

impl std::fmt::Debug for StreamKeepaliveWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamKeepaliveWorld")
            .finish_non_exhaustive()
    }
}

/// The rotation machine under test and the actions it has returned so far.
/// Events are plain strings whose id doubles as their dedupe key.
#[derive(cucumber::World)]
pub struct StreamRotationWorld {
    pub machine: api_bones_stream::RotationMachine<String>,
    /// Frames the old stream has already received and the machine has not been shown.
    pub buffered: Arc<Mutex<Vec<api_bones_stream::RotationFrame<String>>>>,
    pub log: Vec<api_bones_stream::RotationAction<String>>,
    /// The consumer driving the shipped `rotating_stream` through a scripted open function.
    pub consumer: Consumer,
}

pub type WatchItem = Result<api_bones_contract_rust::WatchEvent, String>;

/// A stream of what the consumer's open function returns.
pub type WatchStream = std::pin::Pin<Box<dyn futures_util::Stream<Item = WatchItem> + Send>>;

/// A consumer whose open function counts its calls, records the header each
/// request carries and returns scripted streams, first the stream the
/// consumer reads and then its replacement.
#[derive(Default)]
pub struct Consumer {
    /// Whether the response type carries the stream control field:
    /// `WatchEvent`, declared with `stream_controlled!`, when it does, and
    /// `PlainEvent` when it does not.
    pub declared: bool,
    /// Whether the compiler lets the response type open through
    /// `rotating_stream`.
    pub client_accepts: bool,
    /// One entry per open call `rotating_stream` made: the header its request
    /// carried.
    pub opens: Arc<Mutex<Vec<(String, String)>>>,
    /// Feeds the first stream.
    pub first: Option<tokio::sync::mpsc::UnboundedSender<WatchItem>>,
    /// Feeds the replacement stream.
    pub replacement: Option<tokio::sync::mpsc::UnboundedSender<WatchItem>>,
    /// What the consumer reads, when its response type is declared.
    pub stream: Option<WatchStream>,
    /// The ids of the events the consumer has received.
    pub received: Vec<String>,
}

impl Default for StreamRotationWorld {
    fn default() -> Self {
        let buffered: Arc<Mutex<Vec<api_bones_stream::RotationFrame<String>>>> = Arc::default();
        let taken = buffered.clone();
        Self {
            machine: api_bones_stream::RotationMachine::with_buffered_old(move || {
                std::mem::take(&mut *taken.lock().expect("buffer"))
            }),
            buffered,
            log: Vec::new(),
            consumer: Consumer::default(),
        }
    }
}

impl std::fmt::Debug for StreamRotationWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamRotationWorld")
            .finish_non_exhaustive()
    }
}

/// The instance contract under test, as the descriptor set it compiles to.
#[derive(Default, cucumber::World)]
pub struct OverridableDefaultsWorld {
    pub source: String,
    /// The short name of the message under test.
    pub instance: String,
    pub pool: Option<prost_reflect::DescriptorPool>,
}

impl std::fmt::Debug for OverridableDefaultsWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OverridableDefaultsWorld")
            .finish_non_exhaustive()
    }
}

type PagingStream = futures_util::stream::BoxStream<
    'static,
    Result<api_bones_page::Page<String>, api_bones_page::PageError>,
>;

/// A list-only service, the client that pages it and what they returned.
#[derive(cucumber::World)]
#[world(init = Self::new)]
pub struct UnbatchedPagingWorld {
    pub pages: Vec<Vec<u32>>,
    pub requests: Arc<std::sync::atomic::AtomicUsize>,
    pub server: Option<tokio::task::JoinHandle<()>>,
    pub items: Vec<u32>,
    pub refusal: Option<ConnectError>,
}

impl std::fmt::Debug for UnbatchedPagingWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UnbatchedPagingWorld")
            .field("pages", &self.pages)
            .field("items", &self.items)
            .finish_non_exhaustive()
    }
}

impl UnbatchedPagingWorld {
    fn new() -> Self {
        Self {
            pages: Vec::new(),
            requests: Arc::default(),
            server: None,
            items: Vec::new(),
            refusal: None,
        }
    }
}

impl Drop for UnbatchedPagingWorld {
    fn drop(&mut self) {
        if let Some(server) = self.server.take() {
            server.abort();
        }
    }
}

/// The page iterator and the PostgreSQL-backed widget service it calls.
#[derive(cucumber::World)]
#[world(init = Self::new)]
pub struct PagingWorld {
    pub batch: BatchWorld,
    pub cache: api_bones_page::RevisionCache,
    pub identity: Arc<Mutex<String>>,
    pub retained: Vec<api_bones_page::Page<String>>,
    pub latest: Vec<api_bones_page::Page<String>>,
    pub reported_total: Option<api_bones_contract_rust::field_wire::Total>,
    pub total_page: Option<api_bones_page::Page<u32>>,
    pub revalidation_start: usize,
    pub remembered_revisions: HashMap<String, String>,
    pub seed: Option<api_bones_page::Page<String>>,
    pub seed_cursor: String,
    pub seed_request_count: usize,
    pub lock_after_first: bool,
    pub outage_page: Option<usize>,
    pub recovery_after: usize,
    pub failed_attempts: Arc<Mutex<Vec<connectrpc::ErrorCode>>>,
    pub paused: Option<PagingStream>,
    pub idempotency: Idempotency,
    pub items: Vec<String>,
    pub pages: Vec<Vec<String>>,
    pub revisions: Vec<String>,
    pub cursor_refusal: Option<connectrpc::ConnectError>,
    pub error: Option<api_bones_page::PageError>,
    /// Whether the stream was exhausted right after its last yielded value.
    pub ended: bool,
}

impl std::fmt::Debug for PagingWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PagingWorld")
            .field("items", &self.items)
            .field("pages", &self.pages)
            .finish_non_exhaustive()
    }
}

impl PagingWorld {
    fn new() -> Self {
        Self {
            batch: BatchWorld::new(),
            cache: api_bones_page::RevisionCache::default(),
            identity: Arc::new(Mutex::new("alice:org-a".into())),
            retained: Vec::new(),
            latest: Vec::new(),
            reported_total: None,
            total_page: None,
            revalidation_start: 0,
            remembered_revisions: HashMap::new(),
            seed: None,
            seed_cursor: String::new(),
            seed_request_count: 0,
            lock_after_first: false,
            outage_page: None,
            recovery_after: 0,
            failed_attempts: Arc::default(),
            paused: None,
            idempotency: Idempotency::NoSideEffects,
            items: Vec::new(),
            pages: Vec::new(),
            revisions: Vec::new(),
            cursor_refusal: None,
            error: None,
            ended: false,
        }
    }
}

/// The credential a caller of `org` presents: the identity the batch runtime
/// guards and the bearer token the widgets server reads the organization from.
/// Two tokens of one subject and organization are the same identity under
/// different credential headers.
pub type WidgetIssuers = Arc<Mutex<HashMap<String, Arc<quorum_chain_issuer::IssuerSubstrate>>>>;

pub async fn token_credential(
    issuers: &WidgetIssuers,
    subject: &str,
    org: &str,
) -> api_bones_batch::Credential {
    credential_for_scopes(issuers, subject, org, &["widgets:read"]).await
}

pub async fn writer_credential(
    issuers: &WidgetIssuers,
    subject: &str,
    org: &str,
) -> api_bones_batch::Credential {
    credential_for_scopes(issuers, subject, org, &["widgets:read", "widgets:write"]).await
}

async fn credential_for_scopes(
    issuers: &WidgetIssuers,
    subject: &str,
    org: &str,
    scopes: &[&str],
) -> api_bones_batch::Credential {
    let org_id = quorum_identity::OrgId::new(org);
    let issuer = issuers
        .lock()
        .expect("issuer registry")
        .entry(org.to_owned())
        .or_insert_with(|| {
            Arc::new(quorum_chain_issuer::IssuerSubstrate::with_keypair(
                org_id.clone(),
                quorum_chain_issuer::KeyPair::new(),
            ))
        })
        .clone();
    let token = quorum_workload_identity_issuer::canonical_claim_token(
        &quorum_identity::CredentialIssuer::platform("widgets"),
        &quorum_identity::PrincipalId::from_owned(subject.to_owned()),
        scopes,
        issuer.as_ref(),
        &org_id,
    )
    .await
    .expect("platform issuer signs widget credential");
    let token = quorum_workload_identity::encode_claim_token(&token);
    let claim =
        quorum_workload_identity::verify_canonical_claim_biscuit(&token, &issuer.public_key())
            .expect("issued credential has a verified identity");
    api_bones_batch::Credential {
        identity: api_bones_batch::Identity {
            subject: claim.principal().to_owned(),
            org: claim
                .org_id()
                .expect("signed organization")
                .as_str()
                .to_owned(),
        },
        headers: vec![(
            "authorization".to_owned(),
            format!("Bearer {token}").into_bytes(),
        )],
    }
}

/// One request the widgets server was asked.
#[derive(Debug, Clone, Default)]
pub struct WidgetRequest {
    /// The RPC's short name: `GetWidget`, `BatchGetWidgets`, ...
    pub rpc: &'static str,
    /// The ids (or the parent) the request asked for, in request order.
    pub ids: Vec<String>,
    pub cursor: String,
    pub page_size: Option<u32>,
    /// The `x-trace` header the request arrived with.
    pub trace: String,
    pub lane: String,
    /// The `connect-timeout-ms` header the request arrived with.
    pub timeout_ms: String,
    pub if_none_match: String,
    pub list_cache: String,
    pub encoded_len: usize,
}

/// A scripted whole-request failure.
#[derive(Debug, Clone)]
pub struct FailRule {
    /// The failure applies to the batch request carrying this id; any batch
    /// request when absent.
    pub carrying: Option<String>,
    pub code: connectrpc::ErrorCode,
    pub times: u32,
}

/// A barrier the widgets server holds every request at until the step opens it.
///
/// `arrived` gains a permit when a request reaches the server, `release` is
/// what a held request waits on, and `finished` gains a permit when a held
/// request's handler ends, whether it answered or was dropped.
pub struct Gate {
    pub arrived: tokio::sync::Semaphore,
    pub release: tokio::sync::Semaphore,
    pub finished: tokio::sync::Semaphore,
}

impl Default for Gate {
    fn default() -> Self {
        Self {
            arrived: tokio::sync::Semaphore::new(0),
            release: tokio::sync::Semaphore::new(0),
            finished: tokio::sync::Semaphore::new(0),
        }
    }
}

impl Gate {
    /// Let every held and every later request through.
    pub fn open(&self) {
        self.release
            .add_permits(tokio::sync::Semaphore::MAX_PERMITS / 2);
    }
}

/// What the widgets server holds and is told to do, and what it has seen.
pub struct WidgetsData {
    /// (organization, id) to name.
    pub widgets: HashMap<(String, String), String>,
    /// (organization, widget id) to parts.
    pub parts: HashMap<(String, String), Vec<String>>,
    pub owners: HashMap<(String, String), String>,
    pub part_page_size: usize,
    pub authorization_fault: Option<(String, String)>,
    pub delay: std::time::Duration,
    /// When set, every request is held at the gate instead of waiting out `delay`.
    pub gate: Option<Arc<Gate>>,
    pub failures: Vec<FailRule>,
    pub reverse_answers: bool,
    pub truncate_answers: bool,
    /// The credential the caller's token source switches to when a batch
    /// request arrives.
    pub swap_on_batch: Option<api_bones_batch::Credential>,
    pub source_fails_on_batch: bool,
    pub source_fault_on_batch: Option<String>,
    pub in_flight: usize,
    pub max_in_flight: usize,
    /// Every request seen, in arrival order.
    pub seen: Vec<WidgetRequest>,
}

impl Default for WidgetsData {
    fn default() -> Self {
        Self {
            widgets: HashMap::new(),
            parts: HashMap::new(),
            owners: HashMap::new(),
            part_page_size: 2,
            authorization_fault: None,
            delay: std::time::Duration::ZERO,
            gate: None,
            failures: Vec::new(),
            reverse_answers: false,
            truncate_answers: false,
            swap_on_batch: None,
            source_fails_on_batch: false,
            source_fault_on_batch: None,
            in_flight: 0,
            max_in_flight: 0,
            seen: Vec::new(),
        }
    }
}

/// A running `Widgets` server on loopback.
pub struct WidgetsServed {
    pub task: Mutex<Option<tokio::task::JoinHandle<()>>>,
    pub db: Arc<api_bones_contract_rust::widget_fixture::BatchDb>,
    pub address: std::net::SocketAddr,
    pub start: Arc<dyn Fn(connectrpc::BoundServer) -> tokio::task::JoinHandle<()> + Send + Sync>,
}

impl WidgetsServed {
    pub async fn stop(&self) {
        let task = self.task.lock().expect("server lifecycle").take();
        if let Some(task) = task {
            task.abort();
            let _ = task.await;
        }
    }

    pub async fn restart(&self) {
        let bound = connectrpc::Server::bind(self.address)
            .await
            .expect("same endpoint restarts");
        let mut task = self.task.lock().expect("server lifecycle");
        assert!(task.is_none(), "restart follows an actual stop");
        *task = Some((self.start)(bound));
    }
}

impl Drop for WidgetsServed {
    fn drop(&mut self) {
        if let Some(task) = self.task.lock().expect("server lifecycle").take() {
            task.abort();
        }
    }
}

/// What one group of a batch list delivered when the consumer read it to the
/// end.
#[derive(Debug, Clone)]
pub struct GroupRead {
    pub parent_id: String,
    pub items: Vec<String>,
    pub error: Option<ConnectError>,
}

/// The batch runtime under test, the connectrpc widgets server it calls over
/// the wire and what it returned.
pub type WidgetPartsCall = api_bones_page::ListCall<
    api_bones_contract_rust::ListWidgetPartsRequest,
    api_bones_contract_rust::ListWidgetPartsResponse,
>;

#[derive(cucumber::World)]
#[world(init = Self::new)]
pub struct BatchWorld {
    pub data: Arc<Mutex<WidgetsData>>,
    /// The credential the client's token source answers with.
    pub credential: Arc<Mutex<api_bones_batch::Credential>>,
    pub issuers: WidgetIssuers,
    pub source_failed: Arc<std::sync::atomic::AtomicBool>,
    pub metadata: Arc<Mutex<String>>,
    /// Installed as the credential once the first chunk has been delivered.
    pub after_first_chunk: Arc<Mutex<Option<api_bones_batch::Credential>>>,
    /// Every wire batch request the runtime reported ending.
    pub records: Arc<Mutex<Vec<api_bones_batch::BatchRecord>>>,
    pub served: Option<Arc<WidgetsServed>>,
    pub client: Option<
        api_bones_batch::Batched<
            api_bones_contract_rust::WidgetsClient<connectrpc::client::HttpClient>,
        >,
    >,
    /// Ids a Given generated, in order.
    pub generated: Vec<String>,
    pub results: Vec<Result<api_bones_contract_rust::Widget, ConnectError>>,
    /// How many leading results belong to the first token's callers.
    pub first_token_calls: usize,
    pub batch_headers: api_bones_batch::CallHeaders,
    pub groups: Vec<GroupRead>,
    pub group_calls: Vec<Option<WidgetPartsCall>>,
    pub pending_batches:
        Vec<tokio::task::JoinHandle<Vec<Result<api_bones_contract_rust::Widget, ConnectError>>>>,
    pub continuation: Option<WidgetPartsCall>,
    pub continuation_identity: Option<api_bones_batch::Credential>,
    pub unbatched_widget: Option<Result<api_bones_contract_rust::Widget, ConnectError>>,
    pub malformed_batches: Vec<Vec<ConnectError>>,
    pub construction_refusal: Option<ConnectError>,
    pub whole_batch_error: Option<ConnectError>,
    pub provider_work: [usize; 2],
}

impl std::fmt::Debug for BatchWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BatchWorld")
            .field("results", &self.results.len())
            .field("groups", &self.groups.len())
            .finish_non_exhaustive()
    }
}

impl BatchWorld {
    pub(crate) fn new() -> Self {
        Self {
            data: Arc::default(),
            credential: Arc::default(),
            issuers: Arc::default(),
            source_failed: Arc::default(),
            metadata: Arc::default(),
            after_first_chunk: Arc::default(),
            records: Arc::default(),
            served: None,
            client: None,
            generated: Vec::new(),
            results: Vec::new(),
            first_token_calls: 0,
            batch_headers: api_bones_batch::CallHeaders::new(),
            groups: Vec::new(),
            group_calls: Vec::new(),
            pending_batches: Vec::new(),
            continuation: None,
            continuation_identity: None,
            unbatched_widget: None,
            malformed_batches: Vec::new(),
            construction_refusal: None,
            whole_batch_error: None,
            provider_work: [0; 2],
        }
    }
}

/// The peer a fleet client dials and what came of the call.
#[derive(Default, cucumber::World)]
pub struct FleetConnectBoundWorld {
    pub port: u16,
    pub plaintext: bool,
    pub peer: Option<tokio::task::JoinHandle<()>>,
    pub elapsed: Option<std::time::Duration>,
    pub failure: Option<ConnectError>,
}

impl std::fmt::Debug for FleetConnectBoundWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FleetConnectBoundWorld")
            .finish_non_exhaustive()
    }
}
