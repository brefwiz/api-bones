// SPDX-License-Identifier: MIT
//! The Node half of [`HttpTransport::fetch`](crate::HttpTransport::fetch): every
//! request goes through the host exchange the runtime's `node` entry installs,
//! and Rust decides everything that is policy.
//!
//! The host keeps the sockets (`node:http2`) and nothing else. Rust reads
//! `process.env.SPIFFE_ENDPOINT_SOCKET`, speaks the SPIFFE Workload API over the
//! exchange's unix form, keeps the X509-SVID stream open, and picks the profile
//! of each URL (see [`crate::identity`]). The browser path is untouched: Window
//! and Worker hosts (and Deno and Bun) never reach this module.
//!
//! # The exchange
//!
//! The slot is `globalThis[Symbol.for("brefwiz.api-bones.node-exchange")]`:
//!
//! ```ts
//! type Exchange = (request: ExchangeRequest) => Promise<ExchangeResponse>;
//!
//! type ExchangeRequest = {
//!   connect:
//!     | { tcp: { host: string; port: number; servername: string;
//!                tls: null | { ca: string | null; cert: string | null;
//!                              key: string | null } } }
//!     | { unix: string };
//!   owner: string;                    // opaque per wasm module instance, stable for its life
//!   generation: number;               // integer >= 0
//!   method: string;                   // "POST" for every Connect call
//!   path: string;                     // path and query, e.g. "/pkg.Svc/Method"
//!   headers: [string, string][];      // lower-case names, no pseudo-headers
//!   body: Uint8Array;                 // empty when the request has none
//!   signal: AbortSignal;
//!   peerAdmissible: ((chain: Uint8Array[]) => string | undefined) | null;
//! };
//!
//! type ExchangeResponse = {
//!   status: number;
//!   headers: [string, string][];      // lower-case names, no pseudo-headers
//!   body: ReadableStream<Uint8Array>;
//! };
//! ```
//!
//! - `tcp.tls === null` is plaintext HTTP/2 (h2c) and happens only for a public
//!   `http:` URL. Otherwise TLS with ALPN `h2`, `servername` as the SNI name
//!   (the runtime omits it when the host is an IP literal).
//! - `tls.ca`, `tls.cert` and `tls.key` are PEM text. `ca === null` means the
//!   platform's default roots (Node's, which honour `NODE_EXTRA_CA_CERTS`);
//!   `cert` and `key` are `null` together, and then no client certificate is
//!   presented. A non-null `ca` is the only trust of the connection.
//! - `unix` is a socket path. It carries only the Workload API stream; the
//!   runtime should `unref()` such a session so a long-lived stream never keeps
//!   the process alive, and speaks h2c over it with `:authority` of the runtime's
//!   choosing.
//! - **Generation.** `0` is a connection with no identity (public, plaintext and
//!   the unix form). Every other value is the identity generation the request
//!   was minted under: it grows by one each time the Workload API delivers a
//!   new X509-SVID. Every request also carries `owner`, an opaque id of the wasm
//!   module instance, stable for its life. The runtime pools sessions per
//!   `(owner, connect, generation)`, compares generations only within one owner, never
//!   serves a request from a session of another generation, and closes the idle
//!   sessions of a connect once it sees a newer generation on it, within that owner. A call in
//!   flight finishes on its own generation's session.
//! - **`peerAdmissible`.** Present (non-null) only for an in-fleet call, where
//!   it is a wasm-bindgen `Closure` built by this module: bound to the wasm
//!   module instance that built it and to the expected peer of the request's
//!   generation. The runtime installs it as the connection's
//!   `checkServerIdentity`, replacing the hostname check (an SVID names no DNS
//!   host): it walks the verified peer chain from the leaf (`cert.raw`, then
//!   each `issuerCertificate` until it repeats) into a `Uint8Array[]` of DER,
//!   calls `peerAdmissible(chain)` synchronously, and fails the handshake with
//!   `new Error(<returned string>)` when it returns a string; `undefined`
//!   admits. Node calls `checkServerIdentity` only after the chain verified
//!   against `ca`. The runtime must use the closure of the request that opens
//!   the connection, and never cache it past that request: it is dropped when
//!   the identity generation it belongs to is replaced and its calls have
//!   finished, and calling a dropped closure throws, which refuses the
//!   connection.
//! - **Response.** `body` carries the response bytes as the server sent them: the
//!   runtime sends no `accept-encoding` and inflates nothing. Cancelling the
//!   stream, or aborting `signal` at any point, resets the HTTP/2 stream with
//!   `CANCEL`. No redirect is followed.
//!
//! # Errors
//!
//! The exchange's promise rejects with an `Error`, optionally with a string
//! `code` (Node's TLS and system error codes). Rust maps a rejection as:
//!
//! - an in-fleet call whose `peerAdmissible` refused, or whose `code` is a chain
//!   verification failure (`UNABLE_TO_VERIFY_LEAF_SIGNATURE`,
//!   `SELF_SIGNED_CERT_IN_CHAIN`, `CERT_HAS_EXPIRED`, `ERR_TLS_CERT_*` and the
//!   like): `PEER_IDENTITY_REFUSED`, `unavailable`;
//! - `name === "AbortError"` or `"TimeoutError"`: `canceled`;
//! - anything else: `unavailable` with the error's `message`.
//!
//! Refusals Rust makes before it calls the exchange are typed errors too:
//! `INSECURE_IN_FLEET` (an in-fleet `http:` URL), `WORKLOAD_IDENTITY_UNAVAILABLE`
//! (no unix `SPIFFE_ENDPOINT_SOCKET`, or the Workload API stream failed) and
//! `UNUSABLE_PEER_IDENTITY` (an SVID that names no `/svc/<workload>`). A Node
//! host without the slot gets `failed_precondition`; the call never falls back to
//! `fetch`.
//!
//! Browser wasm and Node wasm are single-threaded here, which `api-bones-rt`
//! enforces by refusing `atomics`, so holding JS values in `Rc` and
//! thread-locals is sound.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use bytes::Bytes;
use connectrpc::ConnectError;
use http::{HeaderMap, Method, Response, StatusCode, Uri};
use http_body_util::BodyExt as _;
use js_sys::{Array, Function, Object, Promise, Reflect, Symbol, Uint8Array};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{AbortSignal, ReadableStream};

use crate::fetch::{AbortOnDrop, FetchBody, header_of, js_error};
use crate::identity::{self, Profile, Refusal, Svid};

/// The slot the runtime's `node` entry installs the exchange in.
const EXCHANGE_SLOT: &str = "brefwiz.api-bones.node-exchange";

/// The Workload API call that streams the caller's X509-SVID.
const FETCH_X509_SVID: &str = "/SpiffeWorkloadAPI/FetchX509SVID";

fn lookup(root: &JsValue, path: &[&str]) -> Option<JsValue> {
    let mut at = root.clone();
    for key in path {
        at = Reflect::get(&at, &JsValue::from_str(key)).ok()?;
        if at.is_undefined() || at.is_null() {
            return None;
        }
    }
    Some(at)
}

/// Whether the host reports a Node version (`process.versions.node`).
pub(crate) fn has_node_version() -> bool {
    let global: JsValue = js_sys::global().into();
    lookup(&global, &["process", "versions", "node"]).is_some_and(|v| v.as_string().is_some())
}

/// Whether the host is Node: a `process` with a Node version, and none of the
/// globals of a browser (or of Deno and Bun).
pub(crate) fn is_node() -> bool {
    let global: JsValue = js_sys::global().into();
    identity::is_node_host(
        |name| lookup(&global, &[name]).is_some(),
        has_node_version(),
    )
}

fn installed() -> Result<Function, ConnectError> {
    let global: JsValue = js_sys::global().into();
    let slot: JsValue = Symbol::for_(EXCHANGE_SLOT).into();
    Reflect::get(&global, &slot)
        .ok()
        .and_then(|value| value.dyn_into::<Function>().ok())
        .ok_or_else(|| {
            ConnectError::failed_precondition(
                "this Node host has no api-bones exchange: load the SDK runtime's node entry",
            )
        })
}

fn object(entries: &[(&str, JsValue)]) -> Object {
    let object = Object::new();
    for (key, value) in entries {
        let _stored = Reflect::set(&object, &JsValue::from_str(key), value);
    }
    object
}

fn text(value: &str) -> JsValue {
    JsValue::from_str(value)
}

/// What a call needs of the identity of the current generation to check one
/// peer.
struct PeerCheck {
    expected: String,
    /// Refusals the closure has made, so a failed exchange can say whether it
    /// was a refused peer.
    refusals: Rc<Cell<u32>>,
    closure: Closure<dyn Fn(JsValue) -> JsValue>,
}

impl PeerCheck {
    fn new(expected: String) -> Self {
        let refusals = Rc::new(Cell::new(0_u32));
        let counted = Rc::clone(&refusals);
        let want = expected.clone();
        let closure =
            Closure::<dyn Fn(JsValue) -> JsValue>::new(move |chain: JsValue| {
                match admit(&chain, &want) {
                    Ok(()) => JsValue::UNDEFINED,
                    Err(detail) => {
                        counted.set(counted.get().saturating_add(1));
                        text(&detail)
                    }
                }
            });
        Self {
            expected,
            refusals,
            closure,
        }
    }

    fn function(&self) -> &JsValue {
        self.closure.as_ref()
    }
}

/// The verdict of [`identity::admits`] over the `Uint8Array[]` of DER the host
/// hands over, as the message the host fails the handshake with.
fn admit(chain: &JsValue, expected: &str) -> Result<(), String> {
    let refused = || {
        format!(
            "{}: the server did not present {expected}",
            identity::PEER_IDENTITY_REFUSED
        )
    };
    let mut der = Vec::new();
    for item in Array::from(chain).iter() {
        let bytes = item.dyn_into::<Uint8Array>().map_err(|_| refused())?;
        der.push(bytes.to_vec());
    }
    identity::admits(&der, expected).map_err(|_| refused())
}

/// One generation of the caller's identity: an X509-SVID, and the peer checks
/// built for it. Dropped when the generation is replaced and the calls that
/// took it have finished, which drops the closures with it.
struct Identity {
    generation: u32,
    svid: Svid,
    peers: RefCell<HashMap<String, Rc<PeerCheck>>>,
}

impl Identity {
    fn check_for(&self, peer: &str) -> Rc<PeerCheck> {
        let mut peers = self.peers.borrow_mut();
        Rc::clone(
            peers
                .entry(peer.to_owned())
                .or_insert_with(|| Rc::new(PeerCheck::new(peer.to_owned()))),
        )
    }
}

enum State {
    Idle,
    Opening(Promise),
    Live(Rc<Identity>),
}

/// 128 random bits as hex, from the host's `crypto.getRandomValues`. `None`
/// when the host has no such source: the owner isolates the sessions of one
/// module instance from another's, so a guessable one is no owner at all.
fn random_owner() -> Option<String> {
    let bytes = Uint8Array::new_with_length(16);
    let global: JsValue = js_sys::global().into();
    let crypto = lookup(&global, &["crypto"])?;
    Reflect::get(&crypto, &text("getRandomValues"))
        .ok()?
        .dyn_into::<Function>()
        .ok()?
        .call1(&crypto, &bytes)
        .ok()?;
    Some(identity::owner_id(&bytes.to_vec()))
}

fn no_owner() -> ConnectError {
    ConnectError::failed_precondition(
        "this Node host has no crypto.getRandomValues, so the SDK cannot isolate its sessions",
    )
}

/// The caller's Workload API connection, one per wasm module instance.
struct Workload {
    /// Opaque, random, stable for the life of the module instance. Generations
    /// are comparable only within one owner. `None` when the host has no
    /// randomness; every exchange call then fails closed.
    owner: Option<String>,
    generation: Cell<u32>,
    state: RefCell<State>,
}

thread_local! {
    static WORKLOAD: Rc<Workload> = Rc::new(Workload {
        owner: random_owner(),
        generation: Cell::new(0),
        state: RefCell::new(State::Idle),
    });
}

/// Settles the promise an opening call waits on, once.
#[derive(Clone, Default)]
struct Settle(Rc<RefCell<Option<(Function, Function)>>>);

impl Settle {
    fn arm(&self, resolve: Function, reject: Function) {
        *self.0.borrow_mut() = Some((resolve, reject));
    }

    fn ready(&self) {
        let armed = self.0.borrow_mut().take();
        if let Some((resolve, _)) = armed {
            let _called = resolve.call0(&JsValue::UNDEFINED);
        }
    }

    fn failed(&self, detail: &str) {
        let armed = self.0.borrow_mut().take();
        if let Some((_, reject)) = armed {
            let _called = reject.call1(&JsValue::UNDEFINED, &text(detail));
        }
    }
}

enum Step {
    Ready(Rc<Identity>),
    Wait(Promise),
}

impl Workload {
    /// The identity now, or the promise of the first one, opening the stream when
    /// none is open.
    fn step(self: &Rc<Self>) -> Result<Step, ConnectError> {
        match &*self.state.borrow() {
            State::Live(identity) => return Ok(Step::Ready(Rc::clone(identity))),
            State::Opening(ready) => return Ok(Step::Wait(ready.clone())),
            State::Idle => {}
        }
        let owner = self.owner.clone().ok_or_else(no_owner)?;
        let global: JsValue = js_sys::global().into();
        let endpoint = lookup(&global, &["process", "env", "SPIFFE_ENDPOINT_SOCKET"])
            .and_then(|value| value.as_string())
            .ok_or_else(|| {
                Refusal::WorkloadIdentityUnavailable.into_error("SPIFFE_ENDPOINT_SOCKET is not set")
            })?;
        let path = identity::workload_socket(&endpoint)
            .ok_or_else(|| {
                Refusal::WorkloadIdentityUnavailable
                    .into_error("SPIFFE_ENDPOINT_SOCKET names no unix socket")
            })?
            .to_owned();
        let settle = Settle::default();
        let ready = Promise::new(&mut |resolve, reject| settle.arm(resolve, reject));
        *self.state.borrow_mut() = State::Opening(ready.clone());
        wasm_bindgen_futures::spawn_local(Rc::clone(self).run(owner, path, settle));
        Ok(Step::Wait(ready))
    }

    /// Follows the stream until it ends, then forgets the identity so the next
    /// call opens it again.
    async fn run(self: Rc<Self>, owner: String, path: String, settle: Settle) {
        let ended = self.read(&owner, &path, &settle).await;
        *self.state.borrow_mut() = State::Idle;
        settle.failed(&ended);
    }

    /// Reads X509-SVID updates, installing each as the next generation. Returns
    /// why the stream ended.
    async fn read(&self, owner: &str, path: &str, settle: &Settle) -> String {
        let guard = match AbortOnDrop::new() {
            Ok(guard) => guard,
            Err(error) => return error.to_string(),
        };
        let mut body = match open_stream(owner, path, &guard.controller.signal()).await {
            Ok(body) => body,
            Err(detail) => return detail,
        };
        let mut pending = Vec::new();
        while let Some(frame) = body.frame().await {
            match frame {
                Ok(frame) => {
                    if let Ok(data) = frame.into_data() {
                        pending.extend_from_slice(&data);
                    }
                }
                Err(error) => return error.to_string(),
            }
            match take_svids(&mut pending) {
                Ok(svids) => {
                    if let Some(svid) = svids.into_iter().last() {
                        self.install(svid);
                        settle.ready();
                    }
                }
                Err(detail) => return detail,
            }
        }
        "the Workload API stream ended".to_owned()
    }

    fn install(&self, svid: Svid) {
        let generation = self.generation.get().saturating_add(1);
        self.generation.set(generation);
        *self.state.borrow_mut() = State::Live(Rc::new(Identity {
            generation,
            svid,
            peers: RefCell::default(),
        }));
    }
}

/// The identity of the caller now, waiting for the Workload API's first answer.
async fn current_identity() -> Result<Rc<Identity>, ConnectError> {
    let workload = WORKLOAD.with(Rc::clone);
    loop {
        match workload.step()? {
            Step::Ready(identity) => return Ok(identity),
            Step::Wait(ready) => {
                let _settled = JsFuture::from(ready).await.map_err(|detail| {
                    Refusal::WorkloadIdentityUnavailable
                        .into_error(&detail.as_string().unwrap_or_default())
                })?;
            }
        }
    }
}

/// The complete X509-SVID messages of `pending`, removed from it.
fn take_svids(pending: &mut Vec<u8>) -> Result<Vec<Svid>, String> {
    let (messages, rest) = identity::grpc_frames(pending)?;
    let svids = messages
        .into_iter()
        .map(identity::first_svid)
        .collect::<Result<Vec<_>, _>>()?;
    let consumed = pending.len() - rest.len();
    pending.drain(..consumed);
    Ok(svids)
}

/// One call to the exchange.
struct Call<'a> {
    connect: Object,
    owner: &'a str,
    generation: u32,
    method: &'a str,
    path: &'a str,
    headers: Array,
    body: &'a [u8],
    signal: &'a AbortSignal,
    check: Option<&'a PeerCheck>,
}

impl Call<'_> {
    fn request(&self) -> Object {
        object(&[
            ("connect", self.connect.clone().into()),
            ("owner", text(self.owner)),
            ("generation", JsValue::from_f64(f64::from(self.generation))),
            ("method", text(self.method)),
            ("path", text(self.path)),
            ("headers", self.headers.clone().into()),
            ("body", Uint8Array::from(self.body).into()),
            ("signal", self.signal.clone().into()),
            (
                "peerAdmissible",
                self.check
                    .map_or(JsValue::NULL, |check| check.function().clone()),
            ),
        ])
    }

    async fn send(&self, exchange: &Function) -> Result<JsValue, JsValue> {
        let outcome = exchange.call1(&JsValue::UNDEFINED, &self.request())?;
        JsFuture::from(Promise::resolve(&outcome)).await
    }
}

/// The Workload API stream: `{unix}` form, generation 0, no identity.
async fn open_stream(owner: &str, path: &str, signal: &AbortSignal) -> Result<FetchBody, String> {
    let exchange = installed().map_err(|error| error.to_string())?;
    let headers = Array::of3(
        &Array::of2(&text("content-type"), &text("application/grpc")),
        &Array::of2(&text("te"), &text("trailers")),
        &Array::of2(&text("workload.spiffe.io"), &text("true")),
    );
    let frame = identity::fetch_request_frame();
    let call = Call {
        connect: object(&[("unix", text(path))]),
        owner,
        generation: 0,
        method: "POST",
        path: FETCH_X509_SVID,
        headers,
        body: &frame,
        signal,
        check: None,
    };
    let head = call
        .send(&exchange)
        .await
        .map_err(|error| js_error(&error).to_string())?;
    let (status, _headers, body) = parse_head(&head).map_err(|error| error.to_string())?;
    if status != StatusCode::OK {
        return Err(format!("the Workload API answered {status}"));
    }
    Ok(FetchBody::new(body))
}

/// The status, headers and body stream of an exchange response.
fn parse_head(
    head: &JsValue,
) -> Result<(StatusCode, HeaderMap, Option<ReadableStream>), ConnectError> {
    let status = lookup(head, &["status"])
        .and_then(|value| value.as_f64())
        .and_then(|value| value.to_string().parse::<u16>().ok())
        .and_then(|value| StatusCode::from_u16(value).ok())
        .ok_or_else(|| ConnectError::internal("the exchange answered no valid status"))?;
    let mut headers = HeaderMap::new();
    if let Some(entries) = lookup(head, &["headers"]) {
        for entry in Array::from(&entries).iter() {
            if let Some((name, value)) = header_of(&entry) {
                headers.append(name, value);
            }
        }
    }
    let body = lookup(head, &["body"])
        .filter(JsValue::is_object)
        .map(JsCast::unchecked_into::<ReadableStream>);
    Ok((status, headers, body))
}

/// Where a URI is to be reached.
struct Target {
    scheme: String,
    host: String,
    port: u16,
}

impl Target {
    fn of(uri: &Uri) -> Result<Self, ConnectError> {
        let scheme = match uri.scheme_str() {
            Some(scheme @ ("http" | "https")) => scheme.to_owned(),
            _ => {
                return Err(ConnectError::invalid_argument(
                    "the base URL is not an http or https URL",
                ));
            }
        };
        let host = uri
            .host()
            .ok_or_else(|| ConnectError::invalid_argument("the base URL names no host"))?
            .trim_matches(['[', ']'])
            .to_owned();
        let port = uri
            .port_u16()
            .unwrap_or(if scheme == "https" { 443 } else { 80 });
        Ok(Self { scheme, host, port })
    }

    fn connect(&self, tls: JsValue) -> Object {
        let tcp = object(&[
            ("host", text(&self.host)),
            ("port", JsValue::from_f64(f64::from(self.port))),
            ("servername", text(&self.host)),
            ("tls", tls),
        ]);
        object(&[("tcp", tcp.into())])
    }
}

/// Sends a request through the host exchange and returns the response head and
/// its body stream.
pub(crate) async fn send(
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
    payload: &Bytes,
) -> Result<Response<FetchBody>, ConnectError> {
    let exchange = installed()?;
    let owner = WORKLOAD
        .with(|workload| workload.owner.clone())
        .ok_or_else(no_owner)?;
    let target = Target::of(uri)?;
    let profile = identity::profile_of(&target.scheme, &target.host).map_err(|refusal| {
        refusal.into_error(&format!(
            "{} is a workload of the fleet and needs https",
            target.host
        ))
    })?;

    let (connect, generation, check, held): (_, _, Option<Rc<PeerCheck>>, Option<Rc<dyn Any>>) =
        match profile {
            Profile::Public => {
                let tls = if target.scheme == "https" {
                    JsValue::from(object(&[
                        ("ca", JsValue::NULL),
                        ("cert", JsValue::NULL),
                        ("key", JsValue::NULL),
                    ]))
                } else {
                    JsValue::NULL
                };
                (target.connect(tls), 0, None, None)
            }
            Profile::InFleet { label } => {
                let identity = current_identity().await?;
                let peer = identity::peer_spiffe_id(&identity.svid.spiffe_id, &label)
                    .map_err(|refusal| refusal.into_error(&identity.svid.spiffe_id))?;
                let check = identity.check_for(&peer);
                let tls = object(&[
                    ("ca", text(&identity.svid.bundle_pem)),
                    ("cert", text(&identity.svid.cert_pem)),
                    ("key", text(&identity.svid.key_pem)),
                ]);
                let generation = identity.generation;
                (
                    target.connect(tls.into()),
                    generation,
                    Some(check),
                    Some(identity as Rc<dyn Any>),
                )
            }
        };

    let pairs = headers
        .iter()
        .map(|(name, value)| {
            Array::of2(
                &text(name.as_str()),
                &text(&String::from_utf8_lossy(value.as_bytes())),
            )
        })
        .collect::<Array>();
    let guard = AbortOnDrop::new()?;
    let signal = guard.controller.signal();
    let path = uri
        .path_and_query()
        .map_or("/", http::uri::PathAndQuery::as_str);
    let call = Call {
        connect,
        owner: &owner,
        generation,
        method: method.as_str(),
        path,
        headers: pairs,
        body: payload,
        signal: &signal,
        check: check.as_deref(),
    };
    let refusals_before = check.as_ref().map_or(0, |check| check.refusals.get());
    let head = call
        .send(&exchange)
        .await
        .map_err(|error| exchange_error(&error, check.as_deref(), refusals_before))?;
    guard.disarm();

    let (status, headers, body) = parse_head(&head)?;
    let mut body = FetchBody::new(body);
    if let Some(held) = held {
        body = body.holding(held);
    }
    let mut answer = Response::new(body);
    *answer.status_mut() = status;
    *answer.headers_mut() = headers;
    Ok(answer)
}

/// The Connect error of a rejected exchange: a refused peer when the call was
/// in-fleet and the host's rejection is the peer check's or a chain
/// verification failure, otherwise what the host raised.
fn exchange_error(
    error: &JsValue,
    check: Option<&PeerCheck>,
    refusals_before: u32,
) -> ConnectError {
    let Some(check) = check else {
        return js_error(error);
    };
    let code = lookup(error, &["code"]).and_then(|value| value.as_string());
    if check.refusals.get() > refusals_before
        || code
            .as_deref()
            .is_some_and(identity::is_verification_failure)
    {
        return Refusal::PeerIdentityRefused
            .into_error(&format!("the server did not present {}", check.expected));
    }
    js_error(error)
}
