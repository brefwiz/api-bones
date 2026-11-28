// SPDX-License-Identifier: MIT
//! The call helper on `wasm32`, run in Node by `wasm-bindgen-test-runner`
//! against the real test server (`tests/wasm-server`, started by
//! `make ci-wasm-test`, which exports its base URL in `API_BONES_TEST_SERVER`).
//!
//! The calls go through the helper and `HttpTransport::fetch()`, which in Node
//! sends every request through the host exchange slot
//! (`globalThis[Symbol.for("brefwiz.api-bones.node-exchange")]`). The suite
//! installs a host there that carries each request to the server over
//! plaintext HTTP/2 with `node:http2`: what the helper hands a send (the
//! deadline the transport states, the credential, the abort) is observed
//! where it lands, on the server. The runtime's own exchange is exercised by
//! its suite and by the sdk-call contract.
//!
//! Native targets, and builds without the crate's own `wasm` feature, compile
//! this file to nothing.

#![cfg(all(feature = "wasm", target_arch = "wasm32"))]

use std::time::Duration;

use api_bones::connect::Idempotency;
use api_bones_batch::{Credential, CredentialAsk, CredentialSource, Identity};
use api_bones_rt::{Instant, sleep};
use api_bones_sdk::call::{AbortFlag, CALL_BUDGET, CallEnv};
use api_bones_sdk::{CANCELED, Error};
use api_bones_transport::HttpTransport;
use buffa_types::google::protobuf::{Empty, Int64Value, StringValue};
use connectrpc::client::{CallOptions, ClientConfig, call_unary};
use connectrpc::{ConnectError, HasMessageView, Spec, StreamType};
use futures_util::future::join;
use http::Uri;
use js_sys::{Array, Function, Reflect};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_test::wasm_bindgen_test;

const SERVICE: &str = "bones.test.v1.Fetch";
const DELAY: &str = "/bones.test.v1.Fetch/Delay";
const ABORTED: &str = "/bones.test.v1.Fetch/Aborted";

type Text = <StringValue as HasMessageView>::View<'static>;

#[path = "../../api-bones-transport/tests/support/test_server.rs"]
mod test_server;
use test_server::server;

/// The host the suite runs in: an exchange in the slot, unless one is already
/// there, that opens one plaintext HTTP/2 session per request to the test
/// server and closes it with the stream. Aborting the signal or cancelling the
/// body resets the stream with `CANCEL`, so the server sees the request
/// abandoned.
const HOST: &str = r#"
const slot = Symbol.for("brefwiz.api-bones.node-exchange");
if (globalThis[slot] !== undefined) return;
const http2 = process.getBuiltinModule("node:http2");
const abortError = () => new DOMException("the call was aborted", "AbortError");
globalThis[slot] = (request) => new Promise((resolve, reject) => {
  const { signal } = request;
  if (signal.aborted) return reject(abortError());
  const tcp = request.connect.tcp;
  if (tcp === undefined || tcp.tls !== null) {
    return reject(new Error("this test host carries plaintext HTTP/2 only"));
  }
  const session = http2.connect(`http://${tcp.host}:${tcp.port}`);
  const headers = { ":method": request.method, ":path": request.path };
  for (const [name, value] of request.headers) {
    const earlier = headers[name];
    headers[name] = earlier === undefined ? value : [].concat(earlier, value);
  }
  const stream = session.request(headers);
  let open = true;
  let body;
  const fail = (reason) => {
    reject(reason);
    if (open && body !== undefined) body.error(reason);
    open = false;
  };
  const cancel = () => {
    if (!stream.closed) stream.close(http2.constants.NGHTTP2_CANCEL);
  };
  const onAbort = () => {
    cancel();
    fail(abortError());
  };
  signal.addEventListener("abort", onAbort, { once: true });
  session.on("error", fail);
  stream.on("error", fail);
  stream.on("data", (chunk) => {
    if (open) body.enqueue(new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength));
  });
  stream.on("end", () => {
    if ((stream.rstCode ?? 0) !== 0) return fail(new Error(`the stream was reset (code ${stream.rstCode})`));
    if (open) {
      open = false;
      body.close();
    }
  });
  stream.on("close", () => {
    signal.removeEventListener("abort", onAbort);
    if (open) fail(new Error(`the stream closed before it ended (code ${stream.rstCode ?? "none"})`));
    session.close();
  });
  stream.on("response", (head) => {
    const out = [];
    for (const [name, value] of Object.entries(head)) {
      if (name.startsWith(":") || value === undefined) continue;
      for (const each of [].concat(value)) out.push([name, String(each)]);
    }
    resolve({
      status: Number(head[":status"]),
      headers: out,
      body: new ReadableStream({
        start(controller) {
          body = controller;
        },
        cancel() {
          open = false;
          cancel();
        },
      }),
    });
  });
  stream.end(request.body);
});
"#;

/// The client configuration for the test server, with [`HOST`] installed so
/// the call has an exchange to go through.
fn config() -> ClientConfig {
    Function::new_no_args(HOST)
        .call0(&JsValue::NULL)
        .expect("the host exchange installs");
    ClientConfig::new(server().parse::<Uri>().expect("the base URL is a URI"))
}

fn millis(value: i64) -> Int64Value {
    Int64Value {
        value,
        ..Default::default()
    }
}

fn credential(subject: &str) -> Credential {
    Credential {
        identity: Identity {
            subject: subject.to_owned(),
            org: "org".to_owned(),
        },
        headers: Vec::new(),
    }
}

fn user(subject: &str) -> impl CredentialSource {
    let credential = credential(subject);
    move |_: CredentialAsk| {
        let answer: Result<Credential, ConnectError> = Ok(credential.clone());
        async move { answer }
    }
}

/// The `Delay` RPC through the call helper: the server answers after `wait`
/// with the `connect-timeout-ms` it saw.
async fn delay(env: &CallEnv, flag: Option<AbortFlag>, wait: i64) -> Result<String, Error> {
    let mut call = env.call(SERVICE, "Delay");
    if let Some(flag) = flag {
        call = call.with_abort(flag);
    }
    call.unary(Idempotency::NoSideEffects, |attempt| async move {
        let transport = attempt.transport(HttpTransport::fetch());
        let options = attempt.options()?;
        let spec = Spec::client(DELAY, StreamType::Unary);
        let response =
            call_unary::<_, Int64Value, Text>(&transport, &config(), spec, millis(wait), options)
                .await?;
        Ok(response.into_owned().value)
    })
    .await
}

/// How many `Delay` requests the server has seen the client abandon.
async fn abandoned() -> usize {
    let spec = Spec::client(ABORTED, StreamType::Unary);
    let transport = HttpTransport::fetch();
    let labels = call_unary::<_, Empty, Text>(
        &transport,
        &config(),
        spec,
        Empty::default(),
        CallOptions::default(),
    )
    .await
    .expect("the server answers the abandoned-request query")
    .into_owned()
    .value;
    labels.split(',').filter(|seen| *seen == "Delay").count()
}

/// Waits, bounded, until the server has noticed one more abandoned `Delay`.
async fn await_abandoned(before: usize) {
    for _ in 0..50 {
        if abandoned().await > before {
            return;
        }
        sleep(Duration::from_millis(100)).await;
    }
    panic!("the server never saw a `Delay` request abandoned");
}

/// A JS `AbortController` and its signal.
fn controller() -> (JsValue, web_sys::AbortSignal) {
    let constructor: Function =
        Reflect::get(&js_sys::global(), &JsValue::from_str("AbortController"))
            .expect("Node has AbortController")
            .dyn_into()
            .expect("AbortController is a constructor");
    let controller =
        Reflect::construct(&constructor, &Array::new()).expect("an AbortController is made");
    let signal = Reflect::get(&controller, &JsValue::from_str("signal"))
        .expect("a controller has a signal")
        .dyn_into()
        .expect("the signal is an AbortSignal");
    (controller, signal)
}

fn fire(controller: &JsValue) {
    Reflect::get(controller, &JsValue::from_str("abort"))
        .expect("a controller aborts")
        .dyn_into::<Function>()
        .expect("abort is a function")
        .call0(controller)
        .expect("the controller aborts");
}

#[wasm_bindgen_test]
async fn the_server_sees_the_call_budget_as_the_time_remaining() {
    let env = CallEnv::new(user("a"));
    let stated: u64 = delay(&env, None, 30)
        .await
        .expect("the call finishes inside its budget")
        .parse()
        .expect("the server saw a connect-timeout-ms header");
    let budget = u64::try_from(CALL_BUDGET.as_millis()).expect("the budget fits");
    assert!(
        (budget - 1_000..=budget).contains(&stated),
        "{stated} ms is not the time left of the call budget"
    );
}

#[wasm_bindgen_test]
async fn an_identity_that_changed_sends_nothing_and_says_so() {
    let reads = std::rc::Rc::new(std::cell::Cell::new(0_u32));
    let seen = std::rc::Rc::clone(&reads);
    let env = CallEnv::new(move |_: CredentialAsk| {
        let n = seen.get();
        seen.set(n + 1);
        let answer: Result<Credential, ConnectError> =
            Ok(credential(if n == 0 { "a" } else { "b" }));
        async move { answer }
    });
    let before = abandoned().await;
    let refusal = delay(&env, None, 30).await.expect_err("refused");
    assert_eq!(refusal.code, api_bones_sdk::IDENTITY_CHANGED);
    assert_eq!(abandoned().await, before);
}

#[wasm_bindgen_test]
async fn an_abort_signal_cancels_the_call_and_aborts_its_fetch() {
    let before = abandoned().await;
    let (controller, signal) = controller();
    let flag = AbortFlag::from_signal(Some(&signal));
    let env = CallEnv::new(user("a"));
    let started = Instant::now();
    let (result, ()) = join(delay(&env, Some(flag), 10_000), async {
        sleep(Duration::from_millis(200)).await;
        fire(&controller);
    })
    .await;
    assert_eq!(result.expect_err("canceled").code, CANCELED);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    await_abandoned(before).await;
}

#[wasm_bindgen_test]
async fn a_signal_aborted_before_the_call_sends_nothing() {
    let before = abandoned().await;
    let (controller, signal) = controller();
    fire(&controller);
    let flag = AbortFlag::from_signal(Some(&signal));
    let env = CallEnv::new(user("a"));
    let refusal = delay(&env, Some(flag), 30).await.expect_err("canceled");
    assert_eq!(refusal.code, CANCELED);
    assert_eq!(abandoned().await, before);
}

#[wasm_bindgen_test]
async fn a_call_without_a_signal_is_never_canceled() {
    let flag = AbortFlag::from_signal(None);
    assert!(!flag.is_aborted());
    let env = CallEnv::new(user("a"));
    delay(&env, Some(flag), 30).await.expect("answered");
}

struct Probe {
    canceled: std::rc::Rc<std::cell::Cell<bool>>,
}

impl api_bones_sdk::__private::Opened for Probe {
    fn cancel(&self) {
        self.canceled.set(true);
    }

    fn span(&self) -> Option<String> {
        Some("span".to_owned())
    }
}

#[wasm_bindgen_test]
async fn a_lazy_handle_canceled_before_its_first_read_refuses_that_read_unopened() {
    let started = std::rc::Rc::new(std::cell::Cell::new(false));
    let seen = started.clone();
    let lazy = api_bones_sdk::__private::Lazy::new(async move {
        seen.set(true);
        Ok(Probe {
            canceled: std::rc::Rc::default(),
        })
    });
    lazy.cancel();
    let refusal = lazy.open().await.err().expect("canceled");
    assert_eq!(refusal.code, CANCELED);
    assert!(!started.get(), "the opening call never started");
    assert!(lazy.open().await.expect("closed").is_none());
}

#[wasm_bindgen_test]
async fn a_lazy_handle_has_no_span_before_it_opens_and_one_after() {
    let lazy = api_bones_sdk::__private::Lazy::new(async {
        Ok(Probe {
            canceled: std::rc::Rc::default(),
        })
    });
    assert_eq!(lazy.span(), None);
    lazy.open().await.expect("opened").expect("a handle");
    assert_eq!(lazy.span().as_deref(), Some("span"));
}

#[wasm_bindgen_test]
async fn a_second_read_while_the_first_opens_is_refused_busy() {
    let canceled = std::rc::Rc::new(std::cell::Cell::new(false));
    let flag = canceled.clone();
    let lazy = api_bones_sdk::__private::Lazy::new(async move {
        sleep(Duration::from_millis(30)).await;
        Ok(Probe { canceled: flag })
    });
    let (first, second) = join(lazy.open(), lazy.open()).await;
    assert!(first.expect("opened").is_some());
    assert_eq!(second.err().expect("busy").code, api_bones_sdk::HANDLE_BUSY);
    assert!(!canceled.get());
}

/// A browser host whose members are the JS `members`, as an object literal body.
fn host(members: &str) -> JsValue {
    Function::new_no_args(&format!("return {{ {members} }};"))
        .call0(&JsValue::NULL)
        .expect("the host object is built")
}

fn member(host: &JsValue, name: &str) -> JsValue {
    Reflect::get(host, &JsValue::from_str(name)).expect("the host has the member")
}

#[wasm_bindgen_test]
async fn a_host_without_a_credential_function_fails_closed_naming_why() {
    let env = CallEnv::from_host(host("credential: 1"));
    let refusal = delay(&env, None, 30).await.expect_err("refused");
    assert!(refusal.message.contains("is not a function"), "{refusal:?}");
}

#[wasm_bindgen_test]
async fn a_host_credential_that_throws_synchronously_is_unavailable() {
    let env = CallEnv::from_host(host("credential() { throw new Error('boom'); }"));
    let refusal = delay(&env, None, 30).await.expect_err("refused");
    assert!(refusal.message.contains("could not answer"), "{refusal:?}");
}

#[wasm_bindgen_test]
async fn a_host_credential_that_rejects_is_unavailable() {
    let env = CallEnv::from_host(host("credential: () => Promise.reject(new Error('no'))"));
    let refusal = delay(&env, None, 30).await.expect_err("refused");
    assert!(refusal.message.contains("could not answer"), "{refusal:?}");
}

#[wasm_bindgen_test]
async fn a_host_credential_that_answers_a_non_bytes_value_is_refused() {
    let env = CallEnv::from_host(host("credential: () => 'text'"));
    let refusal = delay(&env, None, 30).await.expect_err("refused");
    assert!(refusal.message.contains("is not bytes"), "{refusal:?}");
}

#[wasm_bindgen_test]
async fn a_settled_credential_read_leaves_the_host_signal_unaborted() {
    let host = host(
        "credential(refresh, signal) { this.signal = signal; return Promise.resolve(new Uint8Array()); }",
    );
    let env = CallEnv::from_host(host.clone());
    delay(&env, None, 30)
        .await
        .expect_err("an empty answer names nothing");
    let signal: web_sys::AbortSignal = member(&host, "signal").dyn_into().expect("a signal");
    assert!(!signal.aborted());
}

#[wasm_bindgen_test]
async fn a_call_canceled_while_its_credential_is_awaited_aborts_the_host_signal() {
    let host =
        host("credential(refresh, signal) { this.signal = signal; return new Promise(() => {}); }");
    let env = CallEnv::from_host(host.clone());
    let (controller, signal) = controller();
    let flag = AbortFlag::from_signal(Some(&signal));
    let (result, ()) = join(delay(&env, Some(flag), 30), async {
        sleep(Duration::from_millis(100)).await;
        fire(&controller);
    })
    .await;
    assert_eq!(result.expect_err("canceled").code, CANCELED);
    let seen: web_sys::AbortSignal = member(&host, "signal").dyn_into().expect("a signal");
    assert!(seen.aborted());
}
