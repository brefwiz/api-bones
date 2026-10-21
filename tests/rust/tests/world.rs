// SPDX-License-Identifier: MIT
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
