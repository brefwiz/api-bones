// SPDX-License-Identifier: MIT
//! Runs `features/connect_get_reads.feature`.
//!
//! The Rust half of the GET selection the TypeScript package makes for the
//! browser. It is transport code callers wrap around their client directly,
//! outside any declared SDK surface (SPEC.md `internal_behavior_owners`).

use std::sync::{Arc, Mutex};

use api_bones::connect::{
    GetReadTransport, PreconditionedTransport, PublicReadTransport, index_generated_policy,
    index_public_read_policy, index_read_policy,
};
use connectrpc::ConnectError;
use connectrpc::client::{BoxFuture, ClientBody, ClientTransport, full_body};
use cucumber::{World, given, then, when};
use http::{Method, Request, Response};

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
    lane: Recorder,
    entries: Vec<String>,
    mount: String,
    precondition_layer: Option<String>,
    credentialed_only: bool,
}

impl std::fmt::Debug for GetWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GetWorld").finish()
    }
}

impl GetWorld {
    fn policy(&self) -> String {
        format!(
            r#"{{"schemaVersion": 2, "methods": [{}]}}"#,
            self.entries.join(",")
        )
    }

    async fn send(&self, path: &str, content_type: &str, body: Vec<u8>, headers: &[(&str, &str)]) {
        let mut builder = Request::builder()
            .method(Method::POST)
            .uri(format!("https://svc{}{path}", self.mount))
            .header("content-type", content_type);
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        let request = builder.body(full_body(body.into())).unwrap();
        let json = self.policy();
        let recorder = self.recorder.clone();
        let reads = index_read_policy(&json);
        let sent = match self.precondition_layer.as_deref() {
            Some("outer") => PreconditionedTransport::new(
                GetReadTransport::new(recorder, reads),
                index_generated_policy(&json),
            )
            .send(request)
            .await
            .map(|_| ()),
            Some(_) => GetReadTransport::new(
                PreconditionedTransport::new(recorder, index_generated_policy(&json)),
                reads,
            )
            .send(request)
            .await
            .map(|_| ()),
            None if self.credentialed_only => GetReadTransport::new(recorder, reads)
                .send(request)
                .await
                .map(|_| ()),
            None => PublicReadTransport::new(
                GetReadTransport::new(recorder, reads),
                self.lane.clone(),
                index_public_read_policy(&json),
            )
            .send(request)
            .await
            .map(|_| ()),
        };
        sent.unwrap();
    }

    fn last(&self) -> (Method, http::Uri, http::HeaderMap) {
        let lane = self.lane.seen.lock().unwrap();
        let credentialed = self.recorder.seen.lock().unwrap();
        let request = lane
            .last()
            .or_else(|| credentialed.last())
            .expect("no request reached a transport");
        (
            request.method().clone(),
            request.uri().clone(),
            request.headers().clone(),
        )
    }

    fn sent_to(&self, method: &str) -> (Method, http::Uri, http::HeaderMap) {
        let seen = self.recorder.seen.lock().unwrap();
        let request = seen
            .iter()
            .rev()
            .find(|r| r.uri().path().ends_with(&format!("/{method}")))
            .expect("no such call reached the transport");
        (
            request.method().clone(),
            request.uri().clone(),
            request.headers().clone(),
        )
    }
}

fn entry(rpc: &str, idempotency: &str, sensitivity: &str, scope: &str, budget: &str) -> String {
    format!(
        r#"{{"rpc": "/pkg.v1.Svc/{rpc}", "procedure": "unary", "idempotency": "{idempotency}",
            "browserCache": {{"scope": "{scope}", "maxAgeSeconds": 60}},
            "sensitivity": "{sensitivity}", "maxEncodedUrlBytes": {budget}}}"#
    )
}

#[given(
    expr = "a policy declaring {string} as {string} with sensitivity {string}, cache scope {string} and a URL budget of {int}"
)]
fn given_policy(
    world: &mut GetWorld,
    rpc: String,
    idempotency: String,
    sensitivity: String,
    scope: String,
    budget: i64,
) {
    world.credentialed_only = true;
    world.entries.push(entry(
        &rpc,
        &idempotency,
        &sensitivity,
        &scope,
        &budget.to_string(),
    ));
}

#[given(expr = "the policy also holds an entry {string} with no cache policy")]
fn given_broken(world: &mut GetWorld, rpc: String) {
    world.entries.push(format!(
        r#"{{"rpc": "/pkg.v1.Svc/{rpc}", "procedure": "unary", "idempotency": "NO_SIDE_EFFECTS",
            "sensitivity": "NON_SENSITIVE", "maxEncodedUrlBytes": 512}}"#
    ));
}

#[given(expr = "the default precondition transport with the GET transport as its {word}")]
fn given_precondition(world: &mut GetWorld, layer: String) {
    world.precondition_layer = Some(layer);
}

#[given("the public read declaration is malformed")]
fn given_malformed_public(world: &mut GetWorld) {
    let last = world.entries.last_mut().expect("no public read declared");
    *last = last.replace("\"number\": 1", "\"number\": 0");
}

#[given(expr = "a public read {string} served at the mount {string}")]
fn given_public(world: &mut GetWorld, rpc: String, mount: String) {
    let base = entry(&rpc, "NO_SIDE_EFFECTS", "NON_SENSITIVE", "NO_STORE", "512");
    let base = base.trim_end().trim_end_matches('}');
    world.entries.push(format!(
        r#"{base}, "publicRead": {{"maxAgeSeconds": 60, "origins": "ANY",
            "orgField": {{"name": "org", "jsonName": "org", "number": 1}}}}}}"#
    ));
    world.mount = mount;
}

#[when(expr = "the client calls {string} with a protobuf message")]
async fn when_call(world: &mut GetWorld, method: String) {
    world
        .send(
            &format!("/pkg.v1.Svc/{method}"),
            "application/proto",
            b"\x0a\x03abc".to_vec(),
            &[],
        )
        .await;
}

#[when(expr = "the client calls {string} with a protobuf message of {int} bytes")]
async fn when_call_sized(world: &mut GetWorld, method: String, size: usize) {
    world
        .send(
            &format!("/pkg.v1.Svc/{method}"),
            "application/proto",
            vec![7; size],
            &[],
        )
        .await;
}

#[when(expr = "the client calls {string} with a JSON message")]
async fn when_call_json(world: &mut GetWorld, method: String) {
    world
        .send(
            &format!("/pkg.v1.Svc/{method}"),
            "application/json",
            br#"{"id":"abc"}"#.to_vec(),
            &[],
        )
        .await;
}

#[when(expr = "the client calls {string} with a protobuf message compressed with {string}")]
async fn when_call_compressed(world: &mut GetWorld, method: String, coding: String) {
    world
        .send(
            &format!("/pkg.v1.Svc/{method}"),
            "application/proto",
            b"\x1f\x8b".to_vec(),
            &[("content-encoding", &coding)],
        )
        .await;
}

#[when(expr = "the client calls {string} with a protobuf message and a bearer token")]
async fn when_call_bearer(world: &mut GetWorld, method: String) {
    world
        .send(
            &format!("/pkg.v1.Svc/{method}"),
            "application/proto",
            b"\x0a\x03abc".to_vec(),
            &[("authorization", "Bearer secret")],
        )
        .await;
}

#[when(
    expr = "the client calls {string} with a protobuf message and a bearer token, a cookie, a CSRF token and a product header"
)]
async fn when_call_all_credentials(world: &mut GetWorld, method: String) {
    world
        .send(
            &format!("/pkg.v1.Svc/{method}"),
            "application/proto",
            b"\x0a\x03abc".to_vec(),
            &[
                ("authorization", "Bearer secret"),
                ("cookie", "sid=1"),
                ("x-csrf-token", "csrf"),
                ("x-product", "p"),
            ],
        )
        .await;
}

#[when(
    expr = "the client calls {string} with a protobuf message and a bearer token on the credentialed transport"
)]
async fn when_call_credentialed(world: &mut GetWorld, method: String) {
    world.credentialed_only = true;
    when_call_bearer(world, method).await;
}

#[when(expr = "the client opens a streaming call to {string}")]
async fn when_stream(world: &mut GetWorld, method: String) {
    world
        .send(
            &format!("/pkg.v1.Svc/{method}"),
            "application/connect+proto",
            b"x".to_vec(),
            &[],
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

#[then(expr = "the request keeps its content type {string}")]
fn then_content_type(world: &mut GetWorld, expected: String) {
    let (_, _, headers) = world.last();
    assert_eq!(headers.get("content-type").unwrap(), expected.as_str());
}

#[then("the request went to the anonymous lane")]
fn then_anonymous_lane(world: &mut GetWorld) {
    assert_eq!(world.lane.seen.lock().unwrap().len(), 1);
    assert!(world.recorder.seen.lock().unwrap().is_empty());
}

#[then("the request went to the credentialed transport")]
fn then_credentialed_transport(world: &mut GetWorld) {
    assert_eq!(world.recorder.seen.lock().unwrap().len(), 1);
    assert!(world.lane.seen.lock().unwrap().is_empty());
}

#[then("the request carries no authorization, cookie, x-csrf-token or x-product header")]
fn then_no_credentials(world: &mut GetWorld) {
    let (_, _, headers) = world.last();
    for name in ["authorization", "cookie", "x-csrf-token", "x-product"] {
        assert!(headers.get(name).is_none(), "{name} travelled");
    }
}

#[then(expr = "the call to {string} was a GET with no if-match header")]
fn then_get_no_if_match(world: &mut GetWorld, method: String) {
    let (actual, _, headers) = world.sent_to(&method);
    assert_eq!(actual, Method::GET);
    assert!(headers.get("if-match").is_none());
}

#[then(expr = "the call to {string} was a POST with if-match {string}")]
fn then_post_if_match(world: &mut GetWorld, method: String, expected: String) {
    let (actual, _, headers) = world.sent_to(&method);
    assert_eq!(actual, Method::POST);
    assert_eq!(headers.get("if-match").unwrap(), expected.as_str());
}

#[tokio::main]
async fn main() {
    let features = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/features/connect_get_reads.feature"
    );
    GetWorld::run(features).await;
}
