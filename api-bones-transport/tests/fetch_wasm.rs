// SPDX-License-Identifier: MIT
//! `HttpTransport::fetch()` on `wasm32`, run in Node by
//! `wasm-bindgen-test-runner` against the real test server
//! (`tests/wasm-server`, started by `make ci-wasm-test`, which exports its base
//! URL in `API_BONES_TEST_SERVER`).
//!
//! The calls go through connectrpc's client core the way a generated client
//! makes them, over `globalThis.fetch` of the Node process. Node's `fetch` has
//! no cookie jar, so a server cannot see the credentials mode; the tests read
//! it from a recorder that wraps `globalThis.fetch`, passes every call to the
//! real `fetch`, and notes what the transport asked for. The recorder is test
//! instrumentation: the transport has no injection point for it to use.
//!
//! Native targets compile this file to nothing.

#![cfg(target_arch = "wasm32")]

use std::time::Duration;

use api_bones_rt::{Instant, sleep, timeout};
use api_bones_transport::{Deadline, FetchBody, HttpTransport};
use buffa_types::google::protobuf::{Empty, Int64Value, StringValue};
use connectrpc::client::{
    BoxFuture, CallOptions, ClientBody, ClientConfig, ClientTransport, call_server_stream,
    call_unary, call_unary_get,
};
use connectrpc::{ConnectError, ErrorCode, HasMessageView, JsonSerialize, Spec, StreamType};
use http::{Request, Response, Uri};
use js_sys::{Array, Function, Object, Promise, Reflect, Symbol};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::wasm_bindgen_test;

const SESSION: &str = "/bones.test.v1.Fetch/Session";
const ROTATE: &str = "/bones.test.v1.Fetch/Rotate";
const WRITE: &str = "/bones.test.v1.Fetch/Write";
const READ: &str = "/bones.test.v1.Fetch/Read";
const DELAY: &str = "/bones.test.v1.Fetch/Delay";
const WATCH: &str = "/bones.test.v1.Fetch/Watch";
const ABORTED: &str = "/bones.test.v1.Fetch/Aborted";
const REDIRECTOR: &str = "/bones.test.v1.Fetch/Redirector";

/// The key under which every wasm module of a page shares its state, and the
/// member of it that holds the CSRF tokens by origin.
const PAGE_SHARED: &str = "brefwiz.api-bones.page-shared";
const CSRF_TOKENS: &str = "csrfTokens";

/// The header a call's CSRF token travels in.
const CSRF_HEADER: &str = "x-csrf-token";

/// A Connect GET URL stays within this many bytes.
const MAX_CONNECT_GET_URL_BYTES: usize = 4096;

/// The task the first instance hands the second, and where the second leaves
/// its outcome.
const SECOND_INSTANCE_TASK: &str = "brefwiz.api-bones.fetch-wasm.second-instance";

type Text = <StringValue as HasMessageView>::View<'static>;

// ---------------------------------------------------------------------------
// The server and the transport
// ---------------------------------------------------------------------------

#[path = "support/test_server.rs"]
mod test_server;
use test_server::server;

/// The public read lane of the test server.
fn public() -> String {
    format!("{}/public", server())
}

fn config(base: &str) -> ClientConfig {
    ClientConfig::new(base.parse::<Uri>().expect("the base URL is a URI"))
}

/// The transport under test, optionally bounded by a call deadline the way
/// the call helper bounds a call.
#[derive(Clone)]
struct Probe {
    transport: HttpTransport,
    deadline: Option<Deadline>,
}

impl Probe {
    fn new() -> Self {
        Self {
            transport: HttpTransport::fetch(),
            deadline: None,
        }
    }

    fn within(budget: Duration) -> Self {
        Self {
            transport: HttpTransport::fetch(),
            deadline: Some(Deadline(Instant::now() + budget)),
        }
    }
}

impl ClientTransport for Probe {
    type ResponseBody = FetchBody;
    type Error = ConnectError;

    fn send(
        &self,
        mut request: Request<ClientBody>,
    ) -> BoxFuture<'static, Result<Response<FetchBody>, ConnectError>> {
        if let Some(deadline) = self.deadline {
            request.extensions_mut().insert(deadline);
        }
        self.transport.send(request)
    }
}

fn text(value: &str) -> StringValue {
    StringValue {
        value: value.to_owned(),
        ..Default::default()
    }
}

fn millis(value: i64) -> Int64Value {
    Int64Value {
        value,
        ..Default::default()
    }
}

/// One unary call whose answer is a string.
async fn unary<Req>(
    probe: &Probe,
    base: &str,
    procedure: &'static str,
    request: Req,
) -> Result<String, ConnectError>
where
    Req: buffa::Message + JsonSerialize,
{
    let spec = Spec::client(procedure, StreamType::Unary);
    let response =
        call_unary::<_, Req, Text>(probe, &config(base), spec, request, CallOptions::default())
            .await?;
    Ok(response.into_owned().value)
}

/// How many requests the server has seen the client abandon under `label`.
async fn abandoned(label: &str) -> usize {
    let labels = unary(&Probe::new(), &server(), ABORTED, Empty::default())
        .await
        .expect("the server answers the abandoned-request query");
    labels.split(',').filter(|seen| *seen == label).count()
}

/// Waits, bounded, until the server has noticed one more abandoned `label`.
async fn await_abandoned(label: &str, before: usize) {
    for _ in 0..50 {
        if abandoned(label).await > before {
            return;
        }
        sleep(Duration::from_millis(100)).await;
    }
    panic!("the server never saw a `{label}` request abandoned");
}

// ---------------------------------------------------------------------------
// The page-shared slot
// ---------------------------------------------------------------------------

/// The object at `parent[key]`, made when there is none.
fn child(parent: &JsValue, key: &JsValue) -> Object {
    if let Some(existing) = Reflect::get(parent, key)
        .ok()
        .and_then(|value| value.dyn_into::<Object>().ok())
    {
        return existing;
    }
    let created = Object::new();
    Reflect::set(parent, key, &created).expect("the page-shared slot is writable");
    created
}

/// The per-origin CSRF token object of the page-shared slot.
fn tokens() -> Object {
    let slot: JsValue = Symbol::for_(PAGE_SHARED).into();
    let shared = child(&js_sys::global(), &slot);
    child(&shared, &JsValue::from_str(CSRF_TOKENS))
}

/// The token the page holds for `origin`.
fn held(origin: &str) -> Option<String> {
    Reflect::get(&tokens(), &JsValue::from_str(origin))
        .ok()
        .and_then(|value| value.as_string())
}

/// Puts `token` in the page's slot for `origin`, or takes the entry out.
fn hold(origin: &str, token: Option<&str>) {
    let key = JsValue::from_str(origin);
    match token {
        Some(token) => {
            Reflect::set(&tokens(), &key, &JsValue::from_str(token))
                .expect("the page-shared slot is writable");
        }
        None => {
            Reflect::delete_property(&tokens(), &key).expect("the page-shared slot is writable");
        }
    }
}

// ---------------------------------------------------------------------------
// The fetch recorder
// ---------------------------------------------------------------------------

/// Wraps `globalThis.fetch` in a pass-through: every call reaches the real
/// `fetch`, and what the transport asked for is noted on the way.
const RECORDER: &str = r#"
const original = globalThis.fetch;
const seen = [];
globalThis.fetch = function (input, init) {
  const headers = (init && init.headers) || null;
  const header = (name) => (headers ? headers.get(name) : null);
  seen.push({
    url: String(input),
    method: init ? init.method : null,
    credentials: init ? init.credentials : null,
    cache: init ? init.cache : null,
    redirect: init ? init.redirect : null,
    csrf: header("x-csrf-token"),
    authorization: header("authorization"),
    timeout: header("connect-timeout-ms"),
    signal: init ? init.signal : null,
  });
  return original.call(globalThis, input, init);
};
return {
  seen,
  restore() {
    globalThis.fetch = original;
  },
};
"#;

/// One call the transport made to `fetch`.
struct Seen {
    url: String,
    method: String,
    credentials: String,
    cache: Option<String>,
    redirect: Option<String>,
    csrf: Option<String>,
    authorization: Option<String>,
    timeout: Option<String>,
    signal: JsValue,
}

impl Seen {
    fn of(entry: &JsValue) -> Self {
        let field = |name: &str| Reflect::get(entry, &JsValue::from_str(name)).unwrap_or_default();
        let string = |name: &str| field(name).as_string();
        Self {
            url: string("url").unwrap_or_default(),
            method: string("method").unwrap_or_default(),
            credentials: string("credentials").unwrap_or_default(),
            cache: string("cache"),
            redirect: string("redirect"),
            csrf: string("csrf"),
            authorization: string("authorization"),
            timeout: string("timeout"),
            signal: field("signal"),
        }
    }

    /// Whether the transport aborted this fetch.
    fn aborted(&self) -> bool {
        Reflect::get(&self.signal, &JsValue::from_str("aborted"))
            .ok()
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
    }
}

/// The installed recorder; dropping it puts the real `fetch` back.
struct Recorder {
    handle: JsValue,
}

impl Recorder {
    fn install() -> Self {
        let handle = Function::new_with_args("", RECORDER)
            .call0(&JsValue::NULL)
            .expect("the recorder installs");
        Self { handle }
    }

    /// The fetches made since the last call.
    fn take(&self) -> Vec<Seen> {
        let seen: Array = Reflect::get(&self.handle, &JsValue::from_str("seen"))
            .expect("the recorder keeps its log")
            .unchecked_into();
        let taken = seen.iter().map(|entry| Seen::of(&entry)).collect();
        seen.set_length(0);
        taken
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        let restore: Function = Reflect::get(&self.handle, &JsValue::from_str("restore"))
            .expect("the recorder can restore fetch")
            .unchecked_into();
        restore
            .call0(&self.handle)
            .expect("the recorder restores fetch");
    }
}

// ---------------------------------------------------------------------------
// AC-F4: the transport against the real server
// ---------------------------------------------------------------------------

#[wasm_bindgen_test]
async fn a_unary_call_round_trips() {
    let recorder = Recorder::install();
    let answer = unary(&Probe::new(), &server(), READ, text("hello"))
        .await
        .expect("the call succeeds");
    assert_eq!(answer, "hello");
    let seen = recorder.take();
    assert_eq!(seen.len(), 1, "one fetch for one call");
    assert_eq!(seen[0].method, "POST");
    assert!(seen[0].url.ends_with(READ), "url: {}", seen[0].url);
}

#[wasm_bindgen_test]
async fn the_session_lane_sends_credentials_and_the_public_lane_none() {
    let recorder = Recorder::install();

    unary(&Probe::new(), &server(), READ, text("session"))
        .await
        .expect("the session lane answers");
    let session = recorder.take();
    assert_eq!(session.len(), 1);
    assert_eq!(session[0].credentials, "include");

    // The public lane drops every header that identifies the caller.
    let options = CallOptions::default()
        .with_header("authorization", "Bearer must-not-travel")
        .with_header(CSRF_HEADER, "must-not-travel");
    let spec = Spec::client(READ, StreamType::Unary);
    let answer = call_unary::<_, StringValue, Text>(
        &Probe::new(),
        &config(&public()),
        spec,
        text("public"),
        options,
    )
    .await
    .expect("the public lane answers")
    .into_owned()
    .value;
    assert_eq!(answer, "public");
    let omitted = recorder.take();
    assert_eq!(omitted.len(), 1);
    assert_eq!(omitted[0].credentials, "omit");
    assert!(
        omitted[0].url.contains("/public/"),
        "url: {}",
        omitted[0].url
    );
    assert_eq!(omitted[0].authorization, None);
    assert_eq!(omitted[0].csrf, None);
}

#[wasm_bindgen_test]
async fn the_csrf_token_is_echoed_and_a_refused_one_is_replaced() {
    let origin = server();
    hold(&origin, None);
    let recorder = Recorder::install();
    let probe = Probe::new();

    // The session call hands out a token; the transport keeps it.
    let minted = unary(&probe, &origin, SESSION, Empty::default())
        .await
        .expect("the session call succeeds");
    assert_eq!(held(&origin).as_deref(), Some(minted.as_str()));
    recorder.take();

    // A write echoes it, and goes once.
    let answer = unary(&probe, &origin, WRITE, text("first"))
        .await
        .expect("the write succeeds");
    assert_eq!(answer, "first");
    let echoed = recorder.take();
    assert_eq!(echoed.len(), 1, "an echoed token needs no replay");
    assert_eq!(echoed[0].csrf.as_deref(), Some(minted.as_str()));

    // The server invalidates the token without telling the caller: the stale
    // echo is refused, the refusal's token replaces it, and the call is
    // replayed with it.
    let rotate = Spec::client(ROTATE, StreamType::Unary);
    call_unary::<_, Empty, <Empty as HasMessageView>::View<'static>>(
        &probe,
        &config(&origin),
        rotate,
        Empty::default(),
        CallOptions::default(),
    )
    .await
    .expect("the rotation succeeds");
    recorder.take();
    let answer = unary(&probe, &origin, WRITE, text("second"))
        .await
        .expect("the replayed write succeeds");
    assert_eq!(answer, "second");
    let replayed = recorder.take();
    assert_eq!(replayed.len(), 2, "one refusal, one replay");
    assert_eq!(replayed[0].csrf.as_deref(), Some(minted.as_str()));
    let fresh = replayed[1]
        .csrf
        .clone()
        .expect("the replay carries a token");
    assert_ne!(fresh, minted);
    assert_eq!(held(&origin).as_deref(), Some(fresh.as_str()));

    // A token the page never held is learned from the refusal.
    hold(&origin, None);
    let answer = unary(&probe, &origin, WRITE, text("third"))
        .await
        .expect("the write after a forgotten token succeeds");
    assert_eq!(answer, "third");
    let learned = recorder.take();
    assert_eq!(learned.len(), 2);
    assert_eq!(learned[0].csrf, None);
    assert_eq!(learned[1].csrf.as_deref(), Some(fresh.as_str()));
    assert_eq!(held(&origin).as_deref(), Some(fresh.as_str()));
}

#[wasm_bindgen_test]
async fn a_redirect_fails_the_call_and_leaks_no_token() {
    let main = server();
    let other = unary(&Probe::new(), &main, REDIRECTOR, Empty::default())
        .await
        .expect("the server names its redirecting origin");
    assert_ne!(other, main);
    hold(&other, None);
    hold(&main, None);
    let recorder = Recorder::install();

    // The redirecting origin holds a token of its own; the 307 it answers with
    // would send that token to the server and file the server's under it.
    hold(&other, Some("origin-a-token"));
    let error = unary(&Probe::new(), &other, WRITE, text("redirected"))
        .await
        .expect_err("a redirect fails the call");
    assert_eq!(error.code, ErrorCode::Unavailable, "{error:?}");

    let seen = recorder.take();
    assert_eq!(seen.len(), 1, "a redirect is neither followed nor replayed");
    assert_eq!(seen[0].redirect.as_deref(), Some("error"));
    assert_eq!(seen[0].csrf.as_deref(), Some("origin-a-token"));
    assert!(seen[0].url.starts_with(&other), "url: {}", seen[0].url);
    assert_eq!(held(&other).as_deref(), Some("origin-a-token"));
    assert_eq!(held(&main), None);
    hold(&other, None);
}

#[wasm_bindgen_test]
async fn a_read_goes_as_a_connect_get_within_the_url_bound() {
    let recorder = Recorder::install();
    let spec = Spec::client(READ, StreamType::Unary);
    let answer = call_unary_get::<_, StringValue, Text>(
        &Probe::new(),
        &config(&server()),
        spec,
        text("get"),
        CallOptions::default(),
    )
    .await
    .expect("the Connect GET succeeds")
    .into_owned()
    .value;
    assert_eq!(answer, "get");
    let seen = recorder.take();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].method, "GET");
    assert!(seen[0].url.contains("connect=v1"), "url: {}", seen[0].url);
    assert!(
        seen[0].url.len() <= MAX_CONNECT_GET_URL_BYTES,
        "the URL is {} bytes",
        seen[0].url.len()
    );
    assert_eq!(seen[0].credentials, "include");
    assert_eq!(seen[0].cache.as_deref(), Some("no-cache"));
}

#[wasm_bindgen_test]
async fn a_failed_fetch_is_unavailable() {
    // Nothing listens on port 1, so the host's fetch rejects with a TypeError.
    let error = unary(&Probe::new(), "http://127.0.0.1:1", READ, text("nobody"))
        .await
        .expect_err("nobody answers");
    assert_eq!(error.code, ErrorCode::Unavailable, "{error:?}");
}

#[wasm_bindgen_test]
async fn the_deadline_is_stated_as_the_time_remaining() {
    let recorder = Recorder::install();
    let probe = Probe::within(Duration::from_secs(5));
    let stated = unary(&probe, &server(), DELAY, millis(30))
        .await
        .expect("the call finishes inside its deadline");

    let remaining: u64 = stated
        .parse()
        .expect("the server saw a connect-timeout-ms header");
    assert!(
        (3_000..=5_000).contains(&remaining),
        "{remaining} ms is not the time left of a 5 s deadline"
    );
    let seen = recorder.take();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].timeout.as_deref(), Some(stated.as_str()));
}

#[wasm_bindgen_test]
async fn a_passed_deadline_is_deadline_exceeded_and_aborts_the_fetch() {
    let recorder = Recorder::install();
    let before = abandoned("Delay").await;
    recorder.take();

    let probe = Probe::within(Duration::from_millis(200));
    let started = Instant::now();
    let error = unary(&probe, &server(), DELAY, millis(10_000))
        .await
        .expect_err("the server answers too late");

    assert_eq!(error.code, ErrorCode::DeadlineExceeded, "{error:?}");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the call outlived its deadline by {:?}",
        started.elapsed()
    );
    let seen = recorder.take();
    assert_eq!(seen.len(), 1);
    let stated: u64 = seen[0]
        .timeout
        .as_deref()
        .and_then(|value| value.parse().ok())
        .expect("the request stated its timeout");
    assert!((1..=200).contains(&stated), "stated {stated} ms");
    assert!(seen[0].aborted(), "the fetch was not aborted");
    await_abandoned("Delay", before).await;
}

#[wasm_bindgen_test]
async fn dropping_a_call_aborts_its_fetch() {
    let recorder = Recorder::install();
    let before = abandoned("Delay").await;
    recorder.take();

    let probe = Probe::new();
    let base = server();
    let outcome = timeout(
        Duration::from_millis(200),
        unary(&probe, &base, DELAY, millis(10_000)),
    )
    .await;
    assert!(outcome.is_err(), "the call must still be running");

    let seen = recorder.take();
    assert_eq!(seen.len(), 1);
    assert!(
        seen[0].aborted(),
        "dropping the call left its fetch running"
    );
    await_abandoned("Delay", before).await;
}

#[wasm_bindgen_test]
async fn a_server_stream_is_read_message_by_message() {
    let recorder = Recorder::install();
    let spec = Spec::client(WATCH, StreamType::ServerStream);
    let mut stream = call_server_stream::<_, Int64Value, Text>(
        &Probe::new(),
        &config(&server()),
        spec,
        millis(3),
        CallOptions::default(),
    )
    .await
    .expect("the stream opens");

    let mut ticks = Vec::new();
    while let Some(message) = stream
        .message::<StringValue>()
        .await
        .expect("the stream reads")
    {
        ticks.push(message.to_owned_message().value);
    }
    assert_eq!(ticks, ["tick-0", "tick-1", "tick-2"]);
    let seen = recorder.take();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].credentials, "include");
}

#[wasm_bindgen_test]
async fn dropping_a_server_stream_cancels_it() {
    let recorder = Recorder::install();
    let before = abandoned("Watch").await;
    recorder.take();

    let spec = Spec::client(WATCH, StreamType::ServerStream);
    let mut stream = call_server_stream::<_, Int64Value, Text>(
        &Probe::new(),
        &config(&server()),
        spec,
        millis(100_000),
        CallOptions::default(),
    )
    .await
    .expect("the stream opens");
    for expected in ["tick-0", "tick-1"] {
        let message = stream
            .message::<StringValue>()
            .await
            .expect("the stream reads")
            .expect("the stream is still open");
        assert_eq!(message.to_owned_message().value, expected);
    }
    drop(stream);

    await_abandoned("Watch", before).await;
}

// ---------------------------------------------------------------------------
// AC-F5: two wasm instances on one page share the CSRF slot
// ---------------------------------------------------------------------------

/// Loads the test module a second time, as a second wasm instance with its own
/// memory in the same Node process, and runs the test named by `suffix` in it.
/// Resolves to whether that test passed.
///
/// The runner starts the test as `run.cjs`, which requires the module's
/// `wasm-bindgen-test.js` next to it; requiring that file again after
/// dropping it from the module cache instantiates the `.wasm` again.
const SECOND_INSTANCE: &str = r#"
const main = process.mainModule;
if (!main) {
  throw new Error("the runner did not start the suite as a CommonJS module");
}
const file = main.filename.replace(/[^/\\]*$/, "wasm-bindgen-test.js");
const Module = main.constructor;
const first = Module._cache[file];
delete Module._cache[file];
let second;
try {
  second = main.require(file);
} finally {
  if (first) {
    Module._cache[file] = first;
  }
}
const name = Object.keys(second.__wasm).find((key) => key.endsWith(suffix));
if (!name) {
  throw new Error("the second instance exports no test named " + suffix);
}
const context = new second.WasmBindgenTestContext(false);
return context.run([second.__wasm[name]]);
"#;

fn task_key() -> JsValue {
    Symbol::for_(SECOND_INSTANCE_TASK).into()
}

/// The task the first instance left for this one, when this is the second.
fn second_instance_task() -> Option<JsValue> {
    Reflect::get(&js_sys::global(), &task_key())
        .ok()
        .filter(JsValue::is_object)
}

/// Does nothing in the suite's own instance. In the second instance it makes a
/// write call with that instance's own transport and records how it ended.
#[wasm_bindgen_test]
async fn second_instance_write() {
    let Some(task) = second_instance_task() else {
        return;
    };
    let outcome = match unary(
        &Probe::new(),
        &server(),
        WRITE,
        text("from the second instance"),
    )
    .await
    {
        Ok(answer) => format!("ok:{answer}"),
        Err(error) => format!("err:{error:?}"),
    };
    Reflect::set(
        &task,
        &JsValue::from_str("outcome"),
        &JsValue::from_str(&outcome),
    )
    .expect("the task takes its outcome");
}

#[wasm_bindgen_test]
async fn two_instances_share_the_csrf_slot() {
    let origin = server();
    hold(&origin, None);
    let recorder = Recorder::install();

    // The second instance has no token of its own to echo: the first call it
    // makes is refused, learns the server's token, and replays with it.
    let task = Object::new();
    Reflect::set(&js_sys::global(), &task_key(), &task).expect("the global takes the task");
    let promise: Promise = Function::new_with_args("suffix", SECOND_INSTANCE)
        .call1(
            &JsValue::NULL,
            &JsValue::from_str("::second_instance_write"),
        )
        .expect("a second instance of the test module loads")
        .unchecked_into();
    let passed = JsFuture::from(promise)
        .await
        .expect("the second instance runs its test");
    Reflect::delete_property(&js_sys::global(), &task_key()).expect("the global drops the task");
    assert_eq!(
        passed.as_bool(),
        Some(true),
        "the second instance's test failed"
    );

    let outcome = Reflect::get(&task, &JsValue::from_str("outcome"))
        .ok()
        .and_then(|value| value.as_string());
    assert_eq!(
        outcome.as_deref(),
        Some("ok:from the second instance"),
        "the second instance's write did not succeed"
    );
    let second = recorder.take();
    assert_eq!(second.len(), 2, "one refusal, one replay");
    assert_eq!(second[0].csrf, None);
    let learned = second[1]
        .csrf
        .clone()
        .expect("the second instance replayed with the server's token");

    // The slot is the one place the token lives, and this instance reads it.
    assert_eq!(held(&origin).as_deref(), Some(learned.as_str()));

    // This instance never saw the server's token, yet it echoes the one the
    // other learned, so its first write goes through in one fetch.
    let answer = unary(
        &Probe::new(),
        &origin,
        WRITE,
        text("from the first instance"),
    )
    .await
    .expect("the first instance's write succeeds");
    assert_eq!(answer, "from the first instance");
    let first = recorder.take();
    assert_eq!(first.len(), 1, "the shared token needs no replay");
    assert_eq!(first[0].csrf.as_deref(), Some(learned.as_str()));
}
