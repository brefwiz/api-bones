// SPDX-License-Identifier: MIT
//! The Node half of `HttpTransport::fetch()` on `wasm32`, run in Node by
//! `wasm-bindgen-test-runner` with the `node-exchange` feature.
//!
//! Every request goes through the host exchange slot
//! (`globalThis[Symbol.for("brefwiz.api-bones.node-exchange")]`). The tests
//! install a host in that slot that answers from a script and records what the
//! core asked of it: where it connects, with which TLS material, under which
//! owner and generation. The sockets themselves are the runtime's and are
//! exercised against real `node:http2` peers by the runtime's own suite and by
//! the sdk-call contract; here the subject is what the core decides.
//!
//! Native targets compile this file to nothing.

#![cfg(all(target_arch = "wasm32", feature = "node-exchange"))]

use std::time::Duration;

use api_bones_rt::{NoRandomSource, fill_random, sleep};
use api_bones_transport::HttpTransport;
use api_bones_transport::identity::{
    INSECURE_IN_FLEET, PEER_IDENTITY_REFUSED, UNUSABLE_PEER_IDENTITY, WORKLOAD_IDENTITY_UNAVAILABLE,
};
use buffa_types::google::protobuf::StringValue;
use connectrpc::client::{CallOptions, ClientConfig, call_unary};
use connectrpc::{ConnectError, ErrorCode, HasMessageView, Spec, StreamType};
use http::Uri;
use js_sys::{Function, Object, Reflect};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_test::wasm_bindgen_test;

const PROCEDURE: &str = "/bones.test.v1.Fetch/Read";

type Text = <StringValue as HasMessageView>::View<'static>;

/// The host the tests install: a recorder in the exchange slot that answers
/// every call with `responder`. The returned handle keeps the log (`seen`), the
/// knobs the responder reads (`state`) and `restore`, which puts the slot and
/// `SPIFFE_ENDPOINT_SOCKET` back.
const HOST: &str = r#"
const slot = Symbol.for("brefwiz.api-bones.node-exchange");
const previous = globalThis[slot];
const env = process.env;
const previousSocket = env.SPIFFE_ENDPOINT_SOCKET;
const seen = [];
const state = { stream: null, mode: "stream", outcome: "reset", chunks: "rotating" };

const varint = (n) => {
  const out = [];
  while (n >= 0x80) {
    out.push((n & 0x7f) | 0x80);
    n >>= 7;
  }
  out.push(n);
  return out;
};
const field = (number, bytes) => [(number << 3) | 2, ...varint(bytes.length), ...bytes];
const text = (value) => [...new TextEncoder().encode(value)];
const der = [0x30, 0x02, 1, 2];
const frame = (id) => {
  const svid = [...field(1, text(id)), ...field(2, der), ...field(3, text("key")), ...field(4, der)];
  const message = field(1, svid);
  return new Uint8Array([0, 0, 0, (message.length >> 8) & 255, message.length & 255, ...message]);
};
const join = (a, b) => {
  const out = new Uint8Array(a.length + b.length);
  out.set(a, 0);
  out.set(b, a.length);
  return out;
};
const scripts = {
  compressed: () => [new Uint8Array([1, 0, 0, 0, 0])],
  oversize: () => [new Uint8Array([0, 0xff, 0xff, 0xff, 0xff])],
  unusable: () => [frame("spiffe://td/bff")],
  // The first SVID arrives split inside its payload, the second whole behind
  // the rest of the first: only the newest is the identity.
  rotating: () => {
    const first = frame("spiffe://old/svc/bff");
    return [first.slice(0, 8), join(first.slice(8), frame("spiffe://new/svc/bff"))];
  },
};
state.push = (id) => state.stream.enqueue(frame(id));
state.end = () => state.stream.close();

globalThis[slot] = (request) => {
  seen.push(request);
  if ("unix" in request.connect) {
    if (state.mode === "refuse") return Promise.reject(new Error("the socket is gone"));
    if (state.mode === "503") return Promise.resolve({ status: 503, headers: [], body: null });
    const chunks = scripts[state.chunks]();
    return Promise.resolve({
      status: 200,
      headers: [["content-type", "application/grpc"]],
      body: new ReadableStream({
        start(controller) {
          state.stream = controller;
          for (const chunk of chunks) controller.enqueue(chunk);
        },
      }),
    });
  }
  switch (state.outcome) {
    case "peer-check": {
      state.refusal = request.peerAdmissible([new Uint8Array([1, 2, 3])]);
      return Promise.reject(new Error(String(state.refusal)));
    }
    case "empty-chain": {
      state.refusal = request.peerAdmissible([]);
      return Promise.reject(new Error(String(state.refusal)));
    }
    case "not-bytes": {
      state.refusal = request.peerAdmissible(["not DER"]);
      return Promise.reject(new Error(String(state.refusal)));
    }
    case "verify":
      return Promise.reject(Object.assign(new Error("chain"), { code: "UNABLE_TO_VERIFY_LEAF_SIGNATURE" }));
    case "abort":
      return Promise.reject(new DOMException("gone", "AbortError"));
    case "timeout":
      return Promise.reject(new DOMException("slow", "TimeoutError"));
    case "no-status":
      return Promise.resolve({ status: 0, headers: [], body: null });
    default:
      return Promise.reject(Object.assign(new Error("reset by peer"), { code: "ECONNRESET" }));
  }
};

return {
  state,
  take() {
    const taken = seen.splice(0).map((request) => ({
      connect: request.connect,
      owner: request.owner,
      generation: request.generation,
      method: request.method,
      path: request.path,
      bodyLength: request.body.length,
      check: request.peerAdmissible === null ? null : typeof request.peerAdmissible,
    }));
    return JSON.stringify(taken);
  },
  restore() {
    if (previous === undefined) delete globalThis[slot];
    else globalThis[slot] = previous;
    if (previousSocket === undefined) delete env.SPIFFE_ENDPOINT_SOCKET;
    else env.SPIFFE_ENDPOINT_SOCKET = previousSocket;
  },
};
"#;

/// A call to a host that has no exchange: counts the calls `fetch` gets.
const FETCH_SPY: &str = r#"
const slot = Symbol.for("brefwiz.api-bones.node-exchange");
const previous = globalThis[slot];
delete globalThis[slot];
const original = globalThis.fetch;
let calls = 0;
globalThis.fetch = function () {
  calls += 1;
  return Promise.reject(new Error("fetch must not be called"));
};
return {
  calls: () => calls,
  restore() {
    globalThis.fetch = original;
    if (previous !== undefined) globalThis[slot] = previous;
  },
};
"#;

/// The installed host; dropping it puts the slot back.
struct Host {
    handle: JsValue,
}

impl Host {
    fn install() -> Self {
        let handle = Function::new_no_args(HOST)
            .call0(&JsValue::NULL)
            .expect("the host installs");
        Self { handle }
    }

    fn state(&self) -> JsValue {
        get(&self.handle, "state")
    }

    fn set(&self, key: &str, value: &str) {
        Reflect::set(
            &self.state(),
            &JsValue::from_str(key),
            &JsValue::from_str(value),
        )
        .expect("the host state is writable");
    }

    fn method(&self, target: &JsValue, name: &str) {
        let function: Function = get(target, name).unchecked_into();
        function.call0(target).expect("the host method runs");
    }

    /// The requests the core made since the last call.
    fn take(&self) -> Vec<JsValue> {
        let take: Function = get(&self.handle, "take").unchecked_into();
        let json = take
            .call0(&self.handle)
            .expect("the host lists its requests")
            .as_string()
            .expect("as JSON text");
        js_sys::Array::from(&js_sys::JSON::parse(&json).expect("JSON")).to_vec()
    }

    fn end_stream(&self) {
        self.method(&self.state(), "end");
    }

    fn push(&self, id: &str) {
        let push: Function = get(&self.state(), "push").unchecked_into();
        push.call1(&self.state(), &JsValue::from_str(id))
            .expect("the SVID is delivered");
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        let restore: Function = get(&self.handle, "restore").unchecked_into();
        restore
            .call0(&self.handle)
            .expect("the host restores the slot");
    }
}

/// `value` followed down a dotted path.
fn get(value: &JsValue, path: &str) -> JsValue {
    path.split('.').fold(value.clone(), |at, key| {
        Reflect::get(&at, &JsValue::from_str(key)).unwrap_or(JsValue::UNDEFINED)
    })
}

fn set_socket(endpoint: Option<&str>) {
    let global: JsValue = js_sys::global().into();
    let env = get(&global, "process.env");
    match endpoint {
        Some(endpoint) => {
            Reflect::set(
                &env,
                &JsValue::from_str("SPIFFE_ENDPOINT_SOCKET"),
                &JsValue::from_str(endpoint),
            )
            .expect("the environment is writable");
        }
        None => {
            Reflect::delete_property(
                &env.unchecked_into::<Object>(),
                &JsValue::from_str("SPIFFE_ENDPOINT_SOCKET"),
            )
            .expect("the environment is writable");
        }
    }
}

/// One unary call to `base`.
async fn call(base: &str) -> Result<String, ConnectError> {
    let config = ClientConfig::new(base.parse::<Uri>().expect("the base URL is a URI"));
    let spec = Spec::client(PROCEDURE, StreamType::Unary);
    let request = StringValue {
        value: "hello".to_owned(),
        ..Default::default()
    };
    let response = call_unary::<_, StringValue, Text>(
        &HttpTransport::fetch(),
        &config,
        spec,
        request,
        CallOptions::default(),
    )
    .await?;
    Ok(response.into_owned().value)
}

/// The platform code a refusal carries, and the emitter that stamped it.
fn stamped(error: &ConnectError) -> Option<(String, String)> {
    api_bones::connect::error_info(error).map(|info| (info.code, info.emitter))
}

#[track_caller]
fn assert_refused(error: &ConnectError, code: &str, class: ErrorCode) {
    assert_eq!(error.code, class, "{error}");
    assert_eq!(
        stamped(error),
        Some((code.to_owned(), "bones".to_owned())),
        "{error}"
    );
}

// ---------------------------------------------------------------------------
// Target::of and the public profile
// ---------------------------------------------------------------------------

#[wasm_bindgen_test]
async fn a_public_url_is_reached_with_its_scheme_default_port_and_literal_host() {
    let host = Host::install();
    host.set("outcome", "reset");
    for base in [
        "http://example.com",
        "https://example.com",
        "http://example.com:8080",
        "https://[::1]:8443",
        "https://127.0.0.1",
    ] {
        call(base).await.expect_err("the host resets the call");
    }
    let seen = host.take();
    assert_eq!(seen.len(), 5);
    let shape = |index: usize| {
        let request = &seen[index];
        (
            get(request, "connect.tcp.host").as_string(),
            get(request, "connect.tcp.port").as_f64(),
            get(request, "connect.tcp.servername").as_string(),
            js_sys::JSON::stringify(&get(request, "connect.tcp.tls"))
                .ok()
                .map(String::from),
            get(request, "generation").as_f64(),
            get(request, "check").is_null(),
            get(request, "path").as_string(),
        )
    };
    let public = |host: &str, port: f64, tls: &str| {
        (
            Some(host.to_owned()),
            Some(port),
            Some(host.to_owned()),
            Some(tls.to_owned()),
            Some(0.0),
            true,
            Some(PROCEDURE.to_owned()),
        )
    };
    let default_roots = r#"{"ca":null,"cert":null,"key":null}"#;
    assert_eq!(shape(0), public("example.com", 80.0, "null"));
    assert_eq!(shape(1), public("example.com", 443.0, default_roots));
    assert_eq!(shape(2), public("example.com", 8080.0, "null"));
    assert_eq!(shape(3), public("::1", 8443.0, default_roots));
    assert_eq!(shape(4), public("127.0.0.1", 443.0, default_roots));
}

#[wasm_bindgen_test]
async fn a_base_url_that_is_not_http_or_https_is_refused_before_the_host_is_called() {
    let host = Host::install();
    for base in ["ftp://example.com", "ws://example.com"] {
        let error = call(base).await.expect_err("refused");
        assert_eq!(error.code, ErrorCode::InvalidArgument, "{base}: {error}");
    }
    let sent = host.take();
    assert!(sent.is_empty(), "{sent:?}");
}

#[wasm_bindgen_test]
async fn a_host_without_the_slot_fails_the_call_and_never_falls_back_to_fetch() {
    let spy = Function::new_no_args(FETCH_SPY)
        .call0(&JsValue::NULL)
        .expect("the spy installs");
    for base in [
        "https://example.com",
        "https://billing.prod.svc",
        "http://billing",
    ] {
        let error = call(base).await.expect_err("no exchange");
        assert_eq!(error.code, ErrorCode::FailedPrecondition, "{base}: {error}");
        assert!(error.to_string().contains("exchange"), "{base}: {error}");
    }
    let calls: Function = get(&spy, "calls").unchecked_into();
    assert_eq!(calls.call0(&spy).expect("calls").as_f64(), Some(0.0));
    let restore: Function = get(&spy, "restore").unchecked_into();
    restore.call0(&spy).expect("the spy restores fetch");
}

#[wasm_bindgen_test]
async fn a_rejection_of_the_host_maps_to_its_connect_class() {
    let host = Host::install();
    for (outcome, class) in [
        ("abort", ErrorCode::Canceled),
        ("timeout", ErrorCode::Canceled),
        ("reset", ErrorCode::Unavailable),
    ] {
        host.set("outcome", outcome);
        let error = call("https://example.com").await.expect_err("rejected");
        assert_eq!(error.code, class, "{outcome}: {error}");
        // A public call is never a refused peer, whatever the host's code says.
        assert_eq!(stamped(&error), None, "{outcome}");
    }
    // A chain verification failure on a public call is the host's own error.
    host.set("outcome", "verify");
    let error = call("https://example.com").await.expect_err("rejected");
    assert_eq!(error.code, ErrorCode::Unavailable);
    assert_eq!(stamped(&error), None);
    // A response with no valid status is internal.
    host.set("outcome", "no-status");
    host.take();
    let error = call("https://example.com")
        .await
        .expect_err("no status is internal");
    assert_eq!(error.code, ErrorCode::Internal, "{error}");
}

// ---------------------------------------------------------------------------
// The Workload API stream and the in-fleet profile
// ---------------------------------------------------------------------------

#[wasm_bindgen_test]
async fn an_in_fleet_call_is_made_with_the_workload_identity_or_refused() {
    let host = Host::install();
    host.set("outcome", "peer-check");

    // An in-fleet call over http is refused before anything is sent.
    let error = call("http://billing.prod.svc").await.expect_err("plain");
    assert_refused(&error, INSECURE_IN_FLEET, ErrorCode::FailedPrecondition);
    let sent = host.take();
    assert!(sent.is_empty(), "{sent:?}");

    // No socket, or one that is not a unix socket.
    set_socket(None);
    let error = call("https://billing.prod.svc")
        .await
        .expect_err("no socket");
    assert_refused(
        &error,
        WORKLOAD_IDENTITY_UNAVAILABLE,
        ErrorCode::Unavailable,
    );
    let sent = host.take();
    assert!(sent.is_empty(), "{sent:?}");
    set_socket(Some("tcp://127.0.0.1:8081"));
    let error = call("https://billing.prod.svc").await.expect_err("tcp");
    assert_refused(
        &error,
        WORKLOAD_IDENTITY_UNAVAILABLE,
        ErrorCode::Unavailable,
    );
    let sent = host.take();
    assert!(sent.is_empty(), "{sent:?}");

    // The Workload API stream fails: the socket cannot be reached, answers
    // with a status other than 200, sends a compressed frame, or announces a
    // frame larger than the reader accepts.
    set_socket(Some("unix:///run/spire/agent.sock"));
    host.set("mode", "refuse");
    let error = call("https://billing.prod.svc").await.expect_err("refuse");
    assert_refused(
        &error,
        WORKLOAD_IDENTITY_UNAVAILABLE,
        ErrorCode::Unavailable,
    );
    assert!(error.to_string().contains("the socket is gone"), "{error}");
    host.set("mode", "503");
    let error = call("https://billing.prod.svc").await.expect_err("503");
    assert_refused(
        &error,
        WORKLOAD_IDENTITY_UNAVAILABLE,
        ErrorCode::Unavailable,
    );
    assert!(error.to_string().contains("503"), "{error}");
    host.set("mode", "stream");
    for (script, needle) in [("compressed", "compressed"), ("oversize", "exceeds")] {
        host.set("chunks", script);
        let error = call("https://billing.prod.svc").await.expect_err(script);
        assert_refused(
            &error,
            WORKLOAD_IDENTITY_UNAVAILABLE,
            ErrorCode::Unavailable,
        );
        assert!(error.to_string().contains(needle), "{script}: {error}");
    }
    let streams = host.take();
    assert_eq!(streams.len(), 4, "each failure reopened the stream");
    let stream = &streams[0];
    assert_eq!(
        get(stream, "connect.unix").as_string().as_deref(),
        Some("/run/spire/agent.sock")
    );
    assert_eq!(get(stream, "generation").as_f64(), Some(0.0));
    assert_eq!(get(stream, "method").as_string().as_deref(), Some("POST"));
    assert_eq!(
        get(stream, "path").as_string().as_deref(),
        Some("/SpiffeWorkloadAPI/FetchX509SVID")
    );
    assert_eq!(get(stream, "bodyLength").as_f64(), Some(5.0));
    assert!(get(stream, "check").is_null());

    // An SVID that names no /svc/<workload> gives no peer.
    host.set("chunks", "unusable");
    let error = call("https://billing.prod.svc")
        .await
        .expect_err("unusable");
    assert_refused(
        &error,
        UNUSABLE_PEER_IDENTITY,
        ErrorCode::FailedPrecondition,
    );
    host.end_stream();
    sleep(Duration::from_millis(50)).await;
    host.take();

    // Two SVIDs arrive in one read, the first split across reads: the newest
    // is the identity, and the peer follows its prefix.
    host.set("chunks", "rotating");
    let error = call("https://billing.prod.svc").await.expect_err("peer");
    assert_refused(&error, PEER_IDENTITY_REFUSED, ErrorCode::Unavailable);
    assert!(
        error.to_string().contains("spiffe://new/svc/billing"),
        "{error}"
    );
    let first = host.take();
    let fleet = first.last().expect("the in-fleet request");
    let first_generation = get(fleet, "generation").as_f64().expect("generation");
    assert!(first_generation >= 1.0);
    assert_eq!(
        get(fleet, "connect.tcp.host").as_string().as_deref(),
        Some("billing.prod.svc")
    );
    assert_eq!(get(fleet, "connect.tcp.port").as_f64(), Some(443.0));
    assert_eq!(get(fleet, "check").as_string().as_deref(), Some("function"));
    for (member, begin) in [
        ("ca", "-----BEGIN CERTIFICATE-----"),
        ("cert", "-----BEGIN CERTIFICATE-----"),
        ("key", "-----BEGIN PRIVATE KEY-----"),
    ] {
        let pem = get(fleet, &format!("connect.tcp.tls.{member}"))
            .as_string()
            .expect("PEM");
        assert!(pem.starts_with(begin), "{member}");
    }
    let owner = get(fleet, "owner").as_string().expect("owner");
    assert_eq!(owner.len(), 32);
    assert!(
        owner.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "{owner}"
    );
    let refusal = get(&host.state(), "refusal").as_string().expect("refusal");
    assert!(refusal.contains(PEER_IDENTITY_REFUSED), "{refusal}");
    assert!(
        refusal.contains("spiffe://new/svc/billing"),
        "the check expects the peer of the newest SVID: {refusal}"
    );

    // How a rejected in-fleet exchange maps: the peer check's refusal, a chain
    // verification failure, a chain the check cannot read and a host failure
    // that is neither.
    for outcome in ["peer-check", "empty-chain", "not-bytes", "verify"] {
        host.set("outcome", outcome);
        let error = call("https://billing.prod.svc").await.expect_err(outcome);
        assert_refused(&error, PEER_IDENTITY_REFUSED, ErrorCode::Unavailable);
    }
    host.set("outcome", "reset");
    let error = call("https://billing.prod.svc").await.expect_err("reset");
    assert_eq!(error.code, ErrorCode::Unavailable);
    assert_eq!(stamped(&error), None, "{error}");
    assert!(error.to_string().contains("reset by peer"), "{error}");
    host.set("outcome", "abort");
    let error = call("https://billing.prod.svc").await.expect_err("abort");
    assert_eq!(error.code, ErrorCode::Canceled);
    assert_eq!(stamped(&error), None);

    // The same identity serves every call, under one owner and one generation.
    let later = host.take();
    assert_eq!(later.len(), 6);
    for request in &later {
        assert_eq!(
            get(request, "owner").as_string().as_deref(),
            Some(owner.as_str())
        );
        assert_eq!(get(request, "generation").as_f64(), Some(first_generation));
    }

    // A new SVID is a new generation, and its trust domain is the peer's.
    host.push("spiffe://third/svc/bff");
    sleep(Duration::from_millis(50)).await;
    host.set("outcome", "peer-check");
    let error = call("https://billing.prod.svc").await.expect_err("peer");
    assert_refused(&error, PEER_IDENTITY_REFUSED, ErrorCode::Unavailable);
    assert!(
        error.to_string().contains("spiffe://third/svc/billing"),
        "{error}"
    );
    let rotated = host.take();
    assert_eq!(
        get(&rotated[0], "generation").as_f64(),
        Some(first_generation + 1.0)
    );
    assert_eq!(
        get(&rotated[0], "owner").as_string().as_deref(),
        Some(owner.as_str())
    );
    host.end_stream();
    sleep(Duration::from_millis(50)).await;
}

// ---------------------------------------------------------------------------
// A host with no secure random source
// ---------------------------------------------------------------------------

/// Takes `crypto` off the global object for as long as it lives and puts back
/// the property it found.
struct WithoutCrypto {
    global: JsValue,
    found: JsValue,
}

impl WithoutCrypto {
    fn take() -> Self {
        let global = js_sys::global();
        let found = Object::get_own_property_descriptor(&global, &JsValue::from_str("crypto"));
        let hidden = Object::new();
        Reflect::set(&hidden, &"value".into(), &JsValue::UNDEFINED).unwrap();
        Reflect::set(&hidden, &"configurable".into(), &JsValue::TRUE).unwrap();
        Object::define_property(&global, &JsValue::from_str("crypto"), &hidden);
        Self {
            global: global.into(),
            found: found.into(),
        }
    }
}

impl Drop for WithoutCrypto {
    fn drop(&mut self) {
        let global: &Object = self.global.unchecked_ref();
        if self.found.is_undefined() {
            Reflect::delete_property(global, &JsValue::from_str("crypto")).unwrap();
        } else {
            Object::define_property(
                global,
                &JsValue::from_str("crypto"),
                self.found.unchecked_ref(),
            );
        }
    }
}

#[wasm_bindgen_test]
fn a_host_that_loses_crypto_after_load_has_no_random_source() {
    let mut bytes = [0_u8; 16];
    assert_eq!(fill_random(&mut bytes), Ok(()));
    {
        let _lost = WithoutCrypto::take();
        assert_eq!(fill_random(&mut bytes), Err(NoRandomSource));
    }
    assert_eq!(
        fill_random(&mut bytes),
        Ok(()),
        "the source is read on every draw, not cached"
    );
}
