// SPDX-License-Identifier: MIT
//! [`HttpTransport::fetch`]: the host's `fetch` as a Connect client transport.
//!
//! The transport calls `globalThis.fetch`, so it runs in a Window, a Worker
//! and Node 18 or later, and the host supplies nothing. It sits under
//! connectrpc's client core, which is built without its `client` features: the
//! host owns the connection.
//!
//! - **Credentials** follow the request's lane. A path whose first segment is
//!   `public` is the public read lane: `omit`, with every header that
//!   identifies the caller removed. Any other path is the session lane:
//!   `include`, so the session cookie goes along, and a credentialed `GET`
//!   bypasses the HTTP cache.
//! - **CSRF.** The session lane echoes the token its BFF handed out for the
//!   request's origin, remembers the token every response carries, forgets it
//!   when the session ends (401), and replays a refused call once when the
//!   refusal carries a different token. Tokens live in
//!   `globalThis[Symbol.for("brefwiz.api-bones.page-shared")].csrfTokens`, an
//!   object keyed by origin, so every wasm module on a page shares them.
//! - **Deadline.** A [`Deadline`](crate::Deadline) in the request's extensions
//!   is stated in the timeout header, and the fetch is aborted when it passes
//!   before the response head arrives.
//! - **Abort on drop.** Dropping the call's future aborts its fetch; dropping
//!   the response body cancels its reader.
//! - **Errors.** A failed fetch (a `TypeError`) is `unavailable`; an abort is
//!   `canceled`.
//!
//! Browser wasm is single-threaded, which `api-bones-rt` enforces by refusing
//! to compile with `atomics`, so wrapping the futures and bodies that hold JS
//! values in `SendWrapper` meets connectrpc's `Send` bounds soundly.

use std::fmt;
use std::pin::Pin;
use std::task::{Context, Poll, ready};

use bytes::Bytes;
use connectrpc::ConnectError;
use connectrpc::client::{BoxFuture, ClientBody, ClientTransport};
use http::request::Parts;
use http::{HeaderMap, HeaderName, HeaderValue, Method, Request, Response, StatusCode, Uri};
use http_body::{Body, Frame};
use http_body_util::BodyExt as _;
use js_sys::{Function, Object, Promise, Reflect, Symbol, Uint8Array};
use send_wrapper::SendWrapper;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    AbortController, AbortSignal, Headers, ReadableStream, ReadableStreamDefaultReader,
    RequestCache, RequestCredentials, RequestInit, RequestRedirect,
};

use crate::deadline;

/// The request and response header that carries the BFF's CSRF token.
const CSRF_HEADER: &str = "x-csrf-token";

/// The headers that identify the caller. The public lane carries none.
const IDENTIFYING: [&str; 4] = [
    "authorization",
    "brefwiz-relayed-credential",
    "cookie",
    CSRF_HEADER,
];

/// The key under which every wasm module of a page shares its state.
const PAGE_SHARED: &str = "brefwiz.api-bones.page-shared";

/// The Connect client transport of a browser or Node host: `globalThis.fetch`.
/// See the [module documentation](self).
///
/// Clones share the page's CSRF tokens, as every instance does.
#[derive(Clone)]
pub struct HttpTransport(());

impl HttpTransport {
    /// The transport over the host's `fetch`. It takes no arguments: the host,
    /// the credentials mode and the CSRF token all come from the runtime and
    /// the request.
    #[must_use]
    pub fn fetch() -> Self {
        Self(())
    }
}

impl fmt::Debug for HttpTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpTransport")
            .field("mode", &"fetch")
            .finish_non_exhaustive()
    }
}

impl ClientTransport for HttpTransport {
    type ResponseBody = FetchBody;
    type Error = ConnectError;

    fn send(
        &self,
        request: Request<ClientBody>,
    ) -> BoxFuture<'static, Result<Response<FetchBody>, ConnectError>> {
        Box::pin(SendWrapper::new(call(request)))
    }
}

/// Which of a service's two paths a request takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Lane {
    /// The session-bearing path.
    Session,
    /// The public read path, under a `public` first path segment.
    Public,
}

impl Lane {
    fn of(uri: &Uri) -> Self {
        let first = uri.path().trim_start_matches('/').split('/').next();
        if first == Some("public") {
            Self::Public
        } else {
            Self::Session
        }
    }

    const fn credentials(self) -> RequestCredentials {
        match self {
            Self::Session => RequestCredentials::Include,
            Self::Public => RequestCredentials::Omit,
        }
    }
}

/// A request as `fetch` will take it: the body is buffered, so a replay can
/// send it again.
struct Outgoing {
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    payload: Bytes,
    lane: Lane,
}

async fn call(request: Request<ClientBody>) -> Result<Response<FetchBody>, ConnectError> {
    let (mut head, body) = request.into_parts();
    let remaining = deadline::bound(&mut head.headers, &head.extensions)?;
    let payload = body.collect().await?.to_bytes();
    deadline::within(remaining, exchange(head, payload)).await
}

/// Sends the request, with the CSRF echo of its lane, up to the response head.
async fn exchange(head: Parts, payload: Bytes) -> Result<Response<FetchBody>, ConnectError> {
    let lane = Lane::of(&head.uri);
    let mut outgoing = Outgoing {
        method: head.method,
        uri: head.uri,
        headers: head.headers,
        payload,
        lane,
    };
    let origin = origin_of(&outgoing.uri);
    match lane {
        Lane::Public => {
            for name in IDENTIFYING {
                outgoing.headers.remove(name);
            }
        }
        Lane::Session => {
            let echoed = csrf_token(&origin).and_then(|token| HeaderValue::from_str(&token).ok());
            if let Some(token) = echoed {
                outgoing.headers.insert(CSRF_HEADER, token);
            }
        }
    }

    let mut response = fetch_head(&outgoing).await?;
    if lane == Lane::Session {
        let sent = outgoing
            .headers
            .get(CSRF_HEADER)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        learn(&origin, &response);
        let fresh = csrf_of(&response);
        if let Some(token) = replay_token(response.status(), fresh.as_deref(), sent.as_deref()) {
            discard(&response);
            outgoing.headers.insert(CSRF_HEADER, token);
            response = fetch_head(&outgoing).await?;
            learn(&origin, &response);
        }
    }
    into_response(&response)
}

/// Calls `fetch` and waits for the response head. Dropping the future before
/// the head arrives aborts the fetch.
async fn fetch_head(outgoing: &Outgoing) -> Result<web_sys::Response, ConnectError> {
    let guard = AbortOnDrop::new()?;
    let init = request_init(outgoing, &guard.controller.signal())?;
    let fetch = global_fetch()?;
    let global = js_sys::global();
    let promise = fetch
        .call2(
            &global,
            &JsValue::from_str(&outgoing.uri.to_string()),
            &init,
        )
        .map_err(|error| js_error(&error))?;
    let response = JsFuture::from(promise.unchecked_into::<Promise>())
        .await
        .map_err(|error| js_error(&error))?;
    guard.disarm();
    Ok(response.unchecked_into())
}

/// Aborts the fetch it was made for unless disarmed, so a dropped call
/// frees its connection.
struct AbortOnDrop {
    controller: AbortController,
    armed: bool,
}

impl AbortOnDrop {
    fn new() -> Result<Self, ConnectError> {
        let controller = AbortController::new().map_err(|error| js_error(&error))?;
        Ok(Self {
            controller,
            armed: true,
        })
    }

    fn disarm(mut self) {
        self.armed = false;
    }
}

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        if self.armed {
            self.controller.abort();
        }
    }
}

fn request_init(outgoing: &Outgoing, signal: &AbortSignal) -> Result<JsValue, ConnectError> {
    let headers = Headers::new().map_err(|error| js_error(&error))?;
    for (name, value) in &outgoing.headers {
        headers
            .append(name.as_str(), &String::from_utf8_lossy(value.as_bytes()))
            .map_err(|error| js_error(&error))?;
    }
    let init = RequestInit::new();
    init.set_method(outgoing.method.as_str());
    init.set_headers_headers(&headers);
    init.set_credentials(outgoing.lane.credentials());
    // A redirect would carry the origin's CSRF token to another origin and
    // file that origin's token under this one; it fails the fetch instead.
    init.set_redirect(RequestRedirect::Error);
    init.set_signal(Some(signal));
    let reads = outgoing.method == Method::GET;
    if reads && outgoing.lane == Lane::Session {
        init.set_cache(RequestCache::NoCache);
    }
    if !reads && outgoing.method != Method::HEAD && !outgoing.payload.is_empty() {
        init.set_body_opt_u8_array(Some(&Uint8Array::from(&outgoing.payload[..])));
    }
    Ok(init.into())
}

/// `globalThis.fetch`, read on every call.
fn global_fetch() -> Result<Function, ConnectError> {
    Reflect::get(&js_sys::global(), &JsValue::from_str("fetch"))
        .ok()
        .and_then(|value| value.dyn_into::<Function>().ok())
        .ok_or_else(|| ConnectError::unavailable("globalThis.fetch is not a function in this host"))
}

/// A failure the host raised: an abort is `canceled`, anything else (a fetch
/// `TypeError` included) is `unavailable`.
fn js_error(error: &JsValue) -> ConnectError {
    let text = |key: &str| {
        Reflect::get(error, &JsValue::from_str(key))
            .ok()
            .and_then(|value| value.as_string())
    };
    let message = text("message").unwrap_or_else(|| "the host's fetch failed".to_owned());
    match text("name").as_deref() {
        Some("AbortError" | "TimeoutError") => ConnectError::canceled(message),
        _ => ConnectError::unavailable(message),
    }
}

/// The CSRF token a response carries.
fn csrf_of(response: &web_sys::Response) -> Option<String> {
    response
        .headers()
        .get(CSRF_HEADER)
        .ok()
        .flatten()
        .filter(|token| !token.is_empty())
}

/// Keeps the token a response carries, and drops the origin's token when the
/// response says the session ended.
fn learn(origin: &str, response: &web_sys::Response) {
    if response.status() == 401 {
        forget_csrf(origin);
    } else if let Some(token) = csrf_of(response) {
        remember_csrf(origin, &token);
    }
}

/// The token to replay a refused call with, or `None` when it must not be
/// replayed: exactly when the refusal is a 403 carrying a token different from
/// the one sent (including when none was sent). `api-bones-connect` states the
/// same rule as `csrf_replay_token`; it depends on this crate, so it cannot be
/// imported here.
fn replay_token(status: u16, refusal: Option<&str>, sent: Option<&str>) -> Option<HeaderValue> {
    let token =
        refusal.filter(|token| status == 403 && !token.is_empty() && Some(*token) != sent)?;
    HeaderValue::from_str(token).ok()
}

/// Releases the body of a response nobody will read.
fn discard(response: &web_sys::Response) {
    if let Some(body) = response.body() {
        cancel_in_background(body.cancel());
    }
}

/// Lets a cancellation finish without the caller waiting for it; a rejection
/// (the stream already errored) is of no interest.
fn cancel_in_background(cancelled: Promise) {
    wasm_bindgen_futures::spawn_local(async move {
        let _outcome = JsFuture::from(cancelled).await;
    });
}

fn origin_of(uri: &Uri) -> String {
    match (uri.scheme_str(), uri.authority()) {
        (Some(scheme), Some(authority)) => format!("{scheme}://{authority}"),
        _ => String::new(),
    }
}

fn into_response(response: &web_sys::Response) -> Result<Response<FetchBody>, ConnectError> {
    let status = StatusCode::from_u16(response.status())
        .map_err(|_| ConnectError::internal("the host answered a status outside 100-999"))?;
    let mut headers = HeaderMap::new();
    let entries = response.headers();
    let iterator = js_sys::try_iter(&entries).map_err(|error| js_error(&error))?;
    for entry in iterator.into_iter().flatten() {
        let entry = entry.map_err(|error| js_error(&error))?;
        if let Some((name, value)) = header_of(&entry) {
            headers.append(name, value);
        }
    }
    let mut answer = Response::new(FetchBody::new(response.body()));
    *answer.status_mut() = status;
    *answer.headers_mut() = headers;
    Ok(answer)
}

/// A `[name, value]` entry of a `Headers` iteration, when both are valid.
fn header_of(entry: &JsValue) -> Option<(HeaderName, HeaderValue)> {
    let text = |index: f64| Reflect::get_f64(entry, index).ok()?.as_string();
    let name = HeaderName::from_bytes(text(0.0)?.as_bytes()).ok()?;
    let value = HeaderValue::from_str(&text(1.0)?).ok()?;
    Some((name, value))
}

/// The page's CSRF token for `origin`.
fn csrf_token(origin: &str) -> Option<String> {
    Reflect::get(&csrf_tokens(), &JsValue::from_str(origin))
        .ok()
        .and_then(|value| value.as_string())
        .filter(|token| !token.is_empty())
}

fn remember_csrf(origin: &str, token: &str) {
    let _stored = Reflect::set(
        &csrf_tokens(),
        &JsValue::from_str(origin),
        &JsValue::from_str(token),
    );
}

fn forget_csrf(origin: &str) {
    let _deleted = Reflect::delete_property(&csrf_tokens(), &JsValue::from_str(origin));
}

/// The per-origin token object inside the page-shared slot, made on first use
/// by whichever wasm module of the page gets here first.
fn csrf_tokens() -> Object {
    let slot: JsValue = Symbol::for_(PAGE_SHARED).into();
    let shared = child(&js_sys::global(), &slot);
    child(&shared, &JsValue::from_str("csrfTokens"))
}

/// The object at `parent[key]`, created when there is none.
fn child(parent: &JsValue, key: &JsValue) -> Object {
    let existing = Reflect::get(parent, key)
        .ok()
        .and_then(|value| value.dyn_into::<Object>().ok());
    if let Some(existing) = existing {
        return existing;
    }
    let created = Object::new();
    let _stored = Reflect::set(parent, key, &created);
    created
}

/// A fetch response body, read through its `ReadableStreamDefaultReader`.
///
/// Dropping it before the stream ended cancels the reader, which stops the
/// transfer.
pub struct FetchBody {
    reading: SendWrapper<Reading>,
}

impl FetchBody {
    fn new(stream: Option<ReadableStream>) -> Self {
        let reader = stream.map(|stream| stream.get_reader().unchecked_into());
        Self {
            reading: SendWrapper::new(Reading {
                reader,
                pending: None,
            }),
        }
    }
}

impl fmt::Debug for FetchBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FetchBody").finish_non_exhaustive()
    }
}

impl Body for FetchBody {
    type Data = Bytes;
    type Error = ConnectError;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, ConnectError>>> {
        self.get_mut().reading.poll_next(cx)
    }
}

struct Reading {
    /// `None` once the stream ended or failed.
    reader: Option<ReadableStreamDefaultReader>,
    /// The read in flight, kept across polls.
    pending: Option<Pin<Box<JsFuture>>>,
}

impl Reading {
    fn poll_next(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, ConnectError>>> {
        let Some(reader) = &self.reader else {
            return Poll::Ready(None);
        };
        let pending = self
            .pending
            .get_or_insert_with(|| Box::pin(JsFuture::from(reader.read())));
        let outcome = ready!(pending.as_mut().poll(cx));
        self.pending = None;
        let chunk = match outcome {
            Ok(chunk) => chunk,
            Err(error) => {
                self.reader = None;
                return Poll::Ready(Some(Err(js_error(&error))));
            }
        };
        let done = Reflect::get(&chunk, &JsValue::from_str("done"))
            .ok()
            .and_then(|done| done.as_bool())
            .unwrap_or(true);
        if done {
            self.reader = None;
            return Poll::Ready(None);
        }
        let value = Reflect::get(&chunk, &JsValue::from_str("value")).unwrap_or(JsValue::UNDEFINED);
        match value.dyn_into::<Uint8Array>() {
            Ok(bytes) => Poll::Ready(Some(Ok(Frame::data(Bytes::from(bytes.to_vec()))))),
            Err(_) => {
                self.reader = None;
                Poll::Ready(Some(Err(ConnectError::internal(
                    "the host's response stream produced a chunk that is not bytes",
                ))))
            }
        }
    }
}

impl Drop for Reading {
    fn drop(&mut self) {
        if let Some(reader) = self.reader.take() {
            cancel_in_background(reader.cancel());
        }
    }
}
