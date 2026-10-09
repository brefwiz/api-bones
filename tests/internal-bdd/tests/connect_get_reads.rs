// SPDX-License-Identifier: MIT
//! Runs `features/connect_get_reads.feature`.
//!
//! The Rust half of the GET selection the TypeScript package makes for the
//! browser. It is transport code callers wrap around their client directly,
//! outside any declared SDK surface (SPEC.md `internal_behavior_owners`).

use std::sync::{Arc, Mutex};

use api_bones::connect::{
    GetReadTransport, PreconditionedTransport, index_generated_policy, index_read_policy,
};
use connectrpc::ConnectError;
use connectrpc::client::{BoxFuture, ClientBody, ClientTransport, full_body};
use cucumber::{World, given, then, when};
use http::{Method, Request, Response};

const POLICY: &str = r#"{
    "schemaVersion": 2,
    "methods": [
        {"rpc": "/pkg.v1.Svc/Get", "procedure": "unary", "idempotency": "NO_SIDE_EFFECTS",
         "sensitivity": "NON_SENSITIVE", "maxEncodedUrlBytes": 512},
        {"rpc": "/pkg.v1.Svc/Secret", "procedure": "unary", "idempotency": "NO_SIDE_EFFECTS",
         "sensitivity": "SENSITIVE", "maxEncodedUrlBytes": 512},
        {"rpc": "/pkg.v1.Svc/Update", "procedure": "unary", "idempotency": "IDEMPOTENT",
         "sensitivity": "NON_SENSITIVE", "maxEncodedUrlBytes": 512}
    ]
}"#;

type Seen = Arc<Mutex<Vec<Request<()>>>>;

#[derive(Clone, Default)]
struct Recorder {
    seen: Seen,
}

impl ClientTransport for Recorder {
    type ResponseBody = ClientBody;
    type Error = ConnectError;

    fn send(
        &self,
        request: Request<ClientBody>,
    ) -> BoxFuture<'static, Result<Response<Self::ResponseBody>, Self::Error>> {
        self.seen.lock().unwrap().push(request.map(|_| ()));
        Box::pin(async { Ok(Response::new(full_body(Default::default()))) })
    }
}

#[derive(Default, World)]
struct GetWorld {
    recorder: Recorder,
    with_precondition: bool,
}

impl std::fmt::Debug for GetWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GetWorld").finish()
    }
}

impl GetWorld {
    async fn send(&self, path: &str, content_type: &str) {
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!("https://svc{path}"))
            .header("content-type", content_type)
            .body(full_body(b"\x0a\x03abc".as_slice().into()))
            .unwrap();
        let reads = index_read_policy(POLICY);
        let recorder = self.recorder.clone();
        if self.with_precondition {
            GetReadTransport::new(
                PreconditionedTransport::new(recorder, index_generated_policy(POLICY)),
                reads,
            )
            .send(request)
            .await
            .unwrap();
        } else {
            GetReadTransport::new(recorder, reads)
                .send(request)
                .await
                .unwrap();
        }
    }

    fn last(&self) -> (Method, http::Uri, http::HeaderMap) {
        let seen = self.recorder.seen.lock().unwrap();
        let request = seen.last().expect("no request reached the transport");
        (
            request.method().clone(),
            request.uri().clone(),
            request.headers().clone(),
        )
    }
}

#[given(
    expr = "a client transport built from a policy declaring {string} a side-effect-free non-sensitive read"
)]
fn given_transport(world: &mut GetWorld, _method: String) {
    world.with_precondition = false;
}

#[given("the transport also attaches the default precondition")]
fn given_precondition(world: &mut GetWorld) {
    world.with_precondition = true;
}

#[when(expr = "the client calls {string} with a protobuf message")]
async fn when_call(world: &mut GetWorld, method: String) {
    world
        .send(&format!("/pkg.v1.Svc/{method}"), "application/proto")
        .await;
}

#[when(expr = "the client opens a streaming call to {string}")]
async fn when_stream(world: &mut GetWorld, method: String) {
    world
        .send(
            &format!("/pkg.v1.Svc/{method}"),
            "application/connect+proto",
        )
        .await;
}

#[then(expr = "the request is a {word} to {string}")]
fn then_method_path(world: &mut GetWorld, method: String, path: String) {
    let (actual, uri, _) = world.last();
    assert_eq!(actual.as_str(), method);
    assert_eq!(uri.path(), path);
}

#[then(expr = "the query names {string}, {string} and a base64url message")]
fn then_query(world: &mut GetWorld, first: String, second: String) {
    let (_, uri, _) = world.last();
    let query = uri.query().expect("a GET carries its message in the query");
    assert!(query.contains(&first), "{query}");
    assert!(query.contains(&second), "{query}");
    let message = query
        .split('&')
        .find_map(|pair| pair.strip_prefix("message="))
        .expect("message parameter");
    assert!(!message.is_empty());
    assert!(
        message
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
        "{message}"
    );
}

#[then("the request has no content type")]
fn then_no_content_type(world: &mut GetWorld) {
    assert!(world.last().2.get("content-type").is_none());
}

#[then("the request keeps its content type")]
fn then_content_type(world: &mut GetWorld) {
    assert!(world.last().2.get("content-type").is_some());
}

#[then("the request carries no if-match header")]
fn then_no_if_match(world: &mut GetWorld) {
    assert!(world.last().2.get("if-match").is_none());
}

#[tokio::main]
async fn main() {
    let features = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/features/connect_get_reads.feature"
    );
    GetWorld::run(features).await;
}
