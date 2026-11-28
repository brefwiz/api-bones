// SPDX-License-Identifier: MIT
//! Runs `features/sdk_ffi_layer.feature`.
//!
//! The library layer under the generated language bindings is reached through
//! the bindings' host, never through an SDK call (SPEC.md
//! `internal_behavior_owners`). Each scenario drives the layer's Rust API with
//! the bytes a host would pass.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use api_bones::connect::{ConnectCode, Idempotency};
use api_bones_i18n::register_catalog;
use api_bones_sdk::__private::{decode_call_error, encode_call_error};
use api_bones_sdk::{Arg, Attributes, CallCtx, Error, Handle, SpanKind, Tracer};
use api_bones_sdk_ffi::{
    FfiCallCtx, FfiError, FfiHandle, FfiHost, call_env, check_base_url, format_error, transport,
};
use bytes::Bytes;
use connectrpc::client::{ClientTransport, full_body};
use cucumber::{World, given, then, when};
use futures_util::stream;

/// The versions the failing call names: core, wrapper, runtime.
const VERSIONS: (u32, u32, u32) = (2, 1, 3);

#[derive(Default, World)]
struct FfiWorld {
    error: Option<Error>,
    bytes: Vec<u8>,
    formatted: Vec<u8>,
    crossed: Vec<u8>,
    handle: Option<Arc<FfiHandle>>,
    after_cancel: Option<Option<Vec<u8>>>,
    host: Option<Arc<dyn FfiHost>>,
    refusal: Option<Error>,
    sent: bool,
    checked: Option<Result<(), Error>>,
    address: String,
    unreachable: Option<connectrpc::ConnectError>,
    host_ctx: FfiCallCtx,
    core_ctx: Option<CallCtx>,
}

/// How a host's credential read goes wrong.
#[derive(Clone, Copy)]
enum Failure {
    Fails,
    Panics,
}

/// A host whose credential read goes wrong with `text`.
struct FailingHost {
    failure: Failure,
    text: String,
}

impl FfiHost for FailingHost {
    fn credential(&self, _refresh: bool) -> Result<Vec<u8>, FfiError> {
        match self.failure {
            Failure::Fails => Err(FfiError::Call {
                bytes: self.text.clone().into_bytes(),
            }),
            Failure::Panics => panic!("{}", self.text),
        }
    }

    fn anonymous(&self) -> bool {
        false
    }

    fn unauthenticated(&self) {}

    fn span_start(&self, _start: Vec<u8>) -> Vec<u8> {
        Vec::new()
    }

    fn span_end(&self, _end: Vec<u8>) {}
}

impl std::fmt::Debug for FfiWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FfiWorld").finish()
    }
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle.as_bytes())
}

#[given("a call that failed because the SDK versions do not match")]
fn given_mismatch(world: &mut FfiWorld) {
    let (core, wrapper, runtime) = VERSIONS;
    let error = Error::boundary_mismatch(core, wrapper, runtime);
    world.bytes = encode_call_error(&error);
    world.error = Some(error);
}

#[given(
    expr = "a call that failed with a message naming the instant {int} seconds after the epoch"
)]
fn given_instant(world: &mut FfiWorld, seconds: i64) {
    register_catalog(
        "bdd-instant",
        "en",
        r#"{"bdd-instant.errors.AT.title": "{at, time, medium}"}"#,
    );
    let mut error = Error::canceled();
    error.platform = "CONFLICT".to_owned();
    error.code = "AT".to_owned();
    error.emitter = "bdd-instant".to_owned();
    error
        .args
        .insert("at".to_owned(), Arg::Timestamp { seconds, nanos: 0 });
    world.bytes = encode_call_error(&error);
    world.error = Some(error);
}

#[given(expr = "a host whose credential read {word} with the text {string}")]
fn given_host(world: &mut FfiWorld, failure: String, text: String) {
    let failure = match failure.as_str() {
        "fails" => Failure::Fails,
        "panics" => Failure::Panics,
        other => panic!("unknown failure `{other}`"),
    };
    world.host = Some(Arc::new(FailingHost { failure, text }));
}

#[given("bytes that are no call error")]
fn given_garbage(world: &mut FfiWorld) {
    world.bytes = vec![0x0a];
}

#[given("a handle over two items")]
fn given_handle(world: &mut FfiWorld) {
    let span = Tracer::default().start(
        "walk",
        SpanKind::Internal,
        &CallCtx::default(),
        Attributes::new(),
    );
    let items: Vec<Result<u8, Error>> = vec![Ok(1), Ok(2)];
    world.handle = Some(FfiHandle::new(
        Handle::new(Box::pin(stream::iter(items)), span),
        |item: &u8| vec![*item],
    ));
}

#[when(expr = "the error is formatted for locale {string}")]
fn when_formatted(world: &mut FfiWorld, locale: String) {
    world.formatted = format_error(world.bytes.clone(), &locale, None);
}

#[when(expr = "the error is formatted for locale {string} in the time zone {string}")]
fn when_formatted_in_zone(world: &mut FfiWorld, locale: String, zone: String) {
    world.formatted = format_error(world.bytes.clone(), &locale, Some(zone));
}

#[given("an address on the loopback where nothing listens")]
async fn given_closed_port(world: &mut FfiWorld) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a free port");
    world.address = listener.local_addr().expect("an address").to_string();
}

#[when("a call is sent to that address through the binding transport")]
async fn when_sent(world: &mut FfiWorld) {
    let url = format!("http://{}", world.address);
    let request = http::Request::post(format!("{url}/bdd.v1.Service/Method"))
        .header("content-type", "application/json")
        .body(full_body(Bytes::from_static(b"{}")))
        .expect("a request");
    let sent = transport(&url).expect("a loopback address is served");
    world.unreachable = sent.send(request).await.err();
}

#[when(expr = "the base URL {string} is checked")]
fn when_checked(world: &mut FfiWorld, url: String) {
    world.checked = Some(check_base_url(&url).map(|_| ()));
}

#[when("a call is made through the env of that host")]
async fn when_called(world: &mut FfiWorld) {
    let host = world.host.clone().expect("a host");
    let sent = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&sent);
    let result = call_env(host)
        .call("bdd.v1.Service", "Method")
        .unary(Idempotency::NoSideEffects, move |_attempt| {
            let flag = Arc::clone(&flag);
            async move {
                flag.store(true, Ordering::SeqCst);
                Ok::<(), connectrpc::ConnectError>(())
            }
        })
        .await;
    world.sent = sent.load(Ordering::SeqCst);
    world.refusal = result.err();
}

#[when("the error crosses the binding boundary")]
fn when_crosses(world: &mut FfiWorld) {
    let error = world.error.clone().expect("a failed call");
    let FfiError::Call { bytes } = FfiError::from(error);
    world.crossed = bytes;
}

#[when("the host reads one item and cancels the handle twice")]
async fn when_cancelled(world: &mut FfiWorld) {
    let handle = world.handle.clone().expect("a handle");
    assert_eq!(handle.next().await.expect("one item"), Some(vec![1]));
    handle.cancel();
    handle.cancel();
    world.after_cancel = Some(handle.next().await.expect("a cancelled read"));
}

#[then(expr = "the text reads {string}")]
fn then_text(world: &mut FfiWorld, text: String) {
    assert!(
        contains(&world.formatted, &text),
        "the result carries `{text}`"
    );
}

#[then(expr = "the text does not read {string}")]
fn then_not_text(world: &mut FfiWorld, text: String) {
    assert!(
        !contains(&world.formatted, &text),
        "the result does not carry `{text}`"
    );
}

#[then("the base URL is accepted")]
fn then_accepted(world: &mut FfiWorld) {
    let checked = world.checked.take().expect("a checked URL");
    assert!(checked.is_ok(), "{:?}", checked.err());
}

#[then(expr = "the base URL is refused as invalid_argument with {string}")]
fn then_refused_url(world: &mut FfiWorld, reason: String) {
    let error = world
        .checked
        .take()
        .expect("a checked URL")
        .expect_err("refused");
    assert_eq!(error.connect_code, ConnectCode::InvalidArgument);
    assert_eq!(error.message, reason);
}

#[then(expr = "the call is refused as {string} before anything is sent")]
fn then_refused_call(world: &mut FfiWorld, code: String) {
    let error = world.refusal.as_ref().expect("a refused call");
    assert_eq!(error.connect_code.to_string(), code);
    assert!(!world.sent, "nothing was sent");
}

#[then(expr = "the refusal does not repeat {string}")]
fn then_not_repeated(world: &mut FfiWorld, text: String) {
    let error = world.refusal.as_ref().expect("a refused call");
    assert!(!format!("{error:?}").contains(&text));
    let FfiError::Call { bytes } = FfiError::from(error.clone());
    assert!(!contains(&bytes, &text));
}

#[then(expr = "the call fails as {string}")]
fn then_fails_as(world: &mut FfiWorld, code: String) {
    let error = world.unreachable.as_ref().expect("a failed call");
    assert_eq!(format!("{:?}", error.code).to_lowercase(), code);
}

#[then("the failure does not name the address")]
fn then_not_address(world: &mut FfiWorld) {
    let error = world.unreachable.as_ref().expect("a failed call");
    let (host, port) = world.address.split_once(':').expect("host and port");
    let text = error.to_string();
    assert!(!text.contains(host), "{text}");
    assert!(!text.contains(port), "{text}");
}

#[then("the text does not repeat the diagnostic message")]
fn then_not_diagnostic(world: &mut FfiWorld) {
    let diagnostic = world.error.as_ref().expect("a failed call").message.clone();
    assert!(!contains(&world.formatted, &diagnostic));
}

#[then(expr = "the result is refused with the rule {string}")]
fn then_refused(world: &mut FfiWorld, rule: String) {
    assert!(contains(&world.formatted, &rule), "the rule is `{rule}`");
    assert!(
        !contains(&world.formatted, "SDK versions"),
        "a refusal carries no text"
    );
}

#[then("the bytes read back as the same error with its three versions")]
fn then_round_trip(world: &mut FfiWorld) {
    let sent = world.error.as_ref().expect("a failed call");
    let back = decode_call_error(&world.crossed).expect("the bytes decode");
    assert_eq!(back.code, sent.code);
    assert_eq!(back.connect_code, sent.connect_code);
    assert_eq!(back.message, sent.message);
    let (core, wrapper, runtime) = VERSIONS;
    for (name, version) in [("core", core), ("wrapper", wrapper), ("runtime", runtime)] {
        assert!(
            matches!(back.args.get(name), Some(Arg::Int64(n)) if *n == i64::from(version)),
            "`{name}` reads back as {version}"
        );
    }
}

#[given(expr = "a host context naming the trace {string} and the state {string}")]
fn given_host_trace(world: &mut FfiWorld, traceparent: String, tracestate: String) {
    world.host_ctx.traceparent = Some(traceparent);
    world.host_ctx.tracestate = Some(tracestate);
}

#[given(expr = "a host context asking to fetch {int} pages ahead and to read the first page only")]
fn given_host_reads(world: &mut FfiWorld, ahead: u8) {
    world.host_ctx.prefetch = Some(ahead);
    world.host_ctx.first_only = true;
}

#[given("a host context that names nothing")]
fn given_host_nothing(world: &mut FfiWorld) {
    world.host_ctx = FfiCallCtx::default();
}

#[when(expr = "the host context is read for the {word} call")]
fn when_host_ctx_read(world: &mut FfiWorld, call: String) {
    world.core_ctx = Some(world.host_ctx.clone().call_ctx(call == "list"));
}

#[then(expr = "the call runs under the trace {string} and the state {string}")]
fn then_call_trace(world: &mut FfiWorld, traceparent: String, tracestate: String) {
    let ctx = world.core_ctx.as_ref().expect("a context was read");
    assert_eq!(ctx.traceparent, Some(traceparent));
    assert_eq!(ctx.tracestate, Some(tracestate));
}

#[then(expr = "the call fetches {int} pages ahead and reads the first page only")]
fn then_call_reads(world: &mut FfiWorld, ahead: u8) {
    let ctx = world.core_ctx.as_ref().expect("a context was read");
    assert_eq!(ctx.prefetch, Some(ahead));
    assert!(ctx.first_only);
}

#[then("the call leaves the reads to the SDK")]
fn then_call_leaves_reads(world: &mut FfiWorld) {
    let ctx = world.core_ctx.as_ref().expect("a context was read");
    assert_eq!(ctx.prefetch, None);
    assert!(!ctx.first_only);
}

#[then("the call starts a new trace")]
fn then_call_new_trace(world: &mut FfiWorld) {
    let ctx = world.core_ctx.as_ref().expect("a context was read");
    assert_eq!(ctx.traceparent, None);
    assert_eq!(ctx.tracestate, None);
}

#[then("the next read ends the stream")]
fn then_ends(world: &mut FfiWorld) {
    assert_eq!(world.after_cancel, Some(None));
}

#[tokio::main]
async fn main() {
    let features = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/features/sdk_ffi_layer.feature"
    );
    FfiWorld::run(features).await;
}
