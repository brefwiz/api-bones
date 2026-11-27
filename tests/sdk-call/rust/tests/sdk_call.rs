// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! Runs `../features/sdk_call.feature`: the call helper every Layer 2 method
//! runs through, driven as a Layer 2 consumer drives it (the test service's
//! generated Layer 2 clients: `WidgetsClient`, `NumbersClient`,
//! `EventsClient`) against a real connectrpc server on loopback that records
//! what each request carried.
//!
//! `TwoPass` owns scenario selection, concurrency and the census.

use std::collections::HashMap;
use std::fmt;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use api_bones::connect::{ErrorInfo, with_error_info};
use api_bones_batch::{Credential, Identity};
use api_bones_contract_rust::{
    BatchGetWidgetsRequest, BatchGetWidgetsResponse, EventsExt, GetWidgetRequest,
    ListNumbersRequest, ListNumbersResponse, ListWidgetPartsRequest, ListWidgetPartsResponse,
    Numbers, NumbersExt, PageResponse, Widget, Widgets, WidgetsExt,
};
use api_bones_page::{Page, RevisionCache};
use api_bones_sdk::call::CallEnv;
use api_bones_sdk::{
    CallCtx, Error, Handle, SpanEnd, SpanKind, SpanSink, SpanStart, Traceparent, Tracer,
};
use api_bones_test_service_l2::{Number, Tick, Widget as L2Widget};
use brefwiz_cucumber_steps::TwoPass;
use bytes::Bytes;
use connectrpc::{
    CodecFormat, ConnectError, ConnectRpcService, Encodable, ErrorCode, RequestContext, Response,
    Router, Server, ServiceRequest, ServiceResult,
};
use cucumber::{given, then, when};
use http::{HeaderMap, HeaderValue};

#[path = "steps/stream.rs"]
mod stream;
#[path = "../../../rust/tests/steps/support.rs"]
mod support;

use support::{batch_response, unserved_widget_methods};

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().expect("the recorder is never poisoned")
}

/// What one request carried.
#[derive(Debug, Clone)]
struct Seen {
    traceparent: String,
    /// The `authorization` header; empty when the request carried none.
    authorization: String,
    timeout_ms: u64,
    /// The page size a list request asked for; zero for any other request or
    /// none asked.
    page_size: u32,
}

/// The server's script and record, the caller's credential source and the
/// caller's span record.
#[derive(Debug, Default)]
struct Shared {
    widgets: Mutex<HashMap<String, String>>,
    /// The pages the numbers server serves; the cursor `cN` names page `N`.
    pages: Mutex<Vec<Vec<u32>>>,
    /// The pages of parts each widget has, by widget id; the cursor `cN` names
    /// page `N`.
    parts: Mutex<HashMap<String, Vec<Vec<String>>>>,
    /// How long the numbers server takes to answer each page, in milliseconds.
    page_delay_ms: AtomicU64,
    /// How many ticks the events server sends.
    ticks: AtomicU32,
    /// How long the events server waits before each tick, in milliseconds.
    tick_ms: AtomicU64,
    seen: Mutex<Vec<Seen>>,
    /// Requests the server still refuses as unavailable.
    refusals: AtomicUsize,
    /// Requests the server still refuses as unauthenticated.
    unauthenticated: AtomicUsize,
    /// Requests the server still throttles as resource exhausted.
    exhausted: AtomicUsize,
    /// The whole seconds each throttle names in its `retry-after`.
    retry_after_s: AtomicU64,
    /// How many times the host was told a server refused the caller.
    told: AtomicUsize,
    /// Whether the caller's credential source can no longer answer.
    source_broken: AtomicBool,
    /// The revision cache of the caller's env.
    revisions: Revisions,
    /// The subject the server switches the caller to while it handles a request.
    switch_to: Mutex<Option<String>>,
    /// The subject the server switches the caller to while it handles the
    /// request for the page at this zero-based index.
    switch_on_page: Mutex<Option<(usize, String)>>,
    /// The credential the caller's source answers with.
    credential: Mutex<Credential>,
    /// Whether the caller presents no credential at all.
    anonymous: AtomicBool,
    /// The `authorization` value the server gives the caller's source while it
    /// handles a request.
    refresh_to: Mutex<Option<Vec<u8>>>,
    /// How many times the caller's source was read.
    reads: AtomicUsize,
    /// The subject the source answers with from its second read on.
    change_before_send: Mutex<Option<String>>,
    /// Every span the caller's tracer opened, and every one it ended.
    starts: Mutex<Vec<SpanStart>>,
    ends: Mutex<Vec<SpanEnd>>,
    started: AtomicBool,
    /// The zero-based page whose cursor the numbers server refuses as expired.
    expire_at_page: Mutex<Option<usize>>,
    /// Whether the numbers server answers with bytes no decoder reads.
    garble: AtomicBool,
    /// Whether the caller's tracer declines to record: its sink gives no span.
    silent: AtomicBool,
}

/// The revision cache of the caller's env, shown by whether it holds a chain.
#[derive(Clone, Default)]
struct Revisions(RevisionCache);

impl fmt::Debug for Revisions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Revisions")
            .field("empty", &self.0.is_empty())
            .finish()
    }
}

/// Spends one of the refusals `owed` still counts; whether one was owed.
fn spend(owed: &AtomicUsize) -> bool {
    let mut left = owed.load(Ordering::SeqCst);
    while left > 0 {
        match owed.compare_exchange(left, left - 1, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => return true,
            Err(now) => left = now,
        }
    }
    false
}

impl Shared {
    /// Records what `ctx` carried, then answers the failure the script still
    /// owes, if any. Every service of the lane's server admits its requests
    /// through here.
    fn admit(&self, ctx: &RequestContext) -> Result<(), ConnectError> {
        self.admit_sized(ctx, 0)
    }

    /// [`Shared::admit`] for a list request that asked for `page_size`.
    fn admit_sized(&self, ctx: &RequestContext, page_size: u32) -> Result<(), ConnectError> {
        lock(&self.seen).push(Seen {
            traceparent: header(ctx, "traceparent"),
            authorization: header(ctx, "authorization"),
            timeout_ms: header(ctx, "connect-timeout-ms")
                .parse()
                .unwrap_or_default(),
            page_size,
        });
        let refreshed = lock(&self.refresh_to).take();
        if let Some(authorization) = refreshed {
            lock(&self.credential).headers = vec![("authorization".to_owned(), authorization)];
        }
        if spend(&self.unauthenticated) {
            return Err(ConnectError::unauthenticated("the caller is not signed in"));
        }
        if spend(&self.refusals) {
            return Err(ConnectError::unavailable("the server is down"));
        }
        if spend(&self.exhausted) {
            let retry_after = HeaderValue::from(self.retry_after_s.load(Ordering::SeqCst));
            let mut headers = HeaderMap::new();
            headers.insert(http::header::RETRY_AFTER, retry_after);
            return Err(
                ConnectError::resource_exhausted("the server is throttling").with_headers(headers)
            );
        }
        Ok(())
    }

    /// Switches the caller to `subject`.
    fn switch_caller(&self, subject: &str) {
        let mut credential = lock(&self.credential);
        *credential = as_subject(&credential, subject);
    }

    /// Switches the caller when the script says to switch on any request.
    fn switch_if_scripted(&self) {
        let scripted = lock(&self.switch_to).take();
        if let Some(subject) = scripted {
            self.switch_caller(&subject);
        }
    }

    /// Switches the caller when the script says to switch on page `index`.
    fn switch_if_scripted_on_page(&self, index: usize) {
        let mut scripted = lock(&self.switch_on_page);
        if scripted.as_ref().is_some_and(|(page, _)| *page == index) {
            let subject = scripted.take().map(|(_, subject)| subject);
            drop(scripted);
            if let Some(subject) = subject {
                self.switch_caller(&subject);
            }
        }
    }
}

fn header(ctx: &RequestContext, name: &str) -> String {
    ctx.header(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned()
}

fn as_subject(credential: &Credential, subject: &str) -> Credential {
    let mut changed = credential.clone();
    changed.identity.subject = subject.to_owned();
    changed
}

/// The caller's tracer sink: records every span the core opens and ends, and
/// is recording, so each span carries an id of its own.
struct Recorder(Arc<Shared>);

impl SpanSink for Recorder {
    fn start(&self, span: &SpanStart) -> Option<String> {
        if self.0.silent.load(Ordering::SeqCst) {
            return None;
        }
        lock(&self.0.starts).push(span.clone());
        let parent = span.parent.as_deref().and_then(Traceparent::parse);
        Some(Traceparent::mint(parent.as_ref()).to_string())
    }

    fn end(&self, end: &SpanEnd) {
        if self.0.silent.load(Ordering::SeqCst) {
            return;
        }
        lock(&self.0.ends).push(end.clone());
    }
}

#[derive(Clone)]
struct Service(Arc<Shared>);

impl Widgets for Service {
    async fn get_widget<'a>(
        &'a self,
        ctx: RequestContext,
        request: ServiceRequest<'_, GetWidgetRequest>,
    ) -> ServiceResult<impl connectrpc::Encodable<Widget> + Send + use<'a>> {
        let asked = request.to_owned_message();
        self.0.admit(&ctx)?;
        self.0.switch_if_scripted();
        let name = lock(&self.0.widgets)
            .get(&asked.id)
            .cloned()
            .ok_or_else(|| ConnectError::not_found("no such widget"))?;
        Response::ok(Widget {
            id: asked.id.clone(),
            name,
            ..Default::default()
        })
    }

    async fn batch_get_widgets<'a>(
        &'a self,
        ctx: RequestContext,
        request: ServiceRequest<'_, BatchGetWidgetsRequest>,
    ) -> ServiceResult<impl connectrpc::Encodable<BatchGetWidgetsResponse> + Send + use<'a>> {
        let asked = request.to_owned_message();
        self.0.admit(&ctx)?;
        self.0.switch_if_scripted();
        let response = {
            let widgets = lock(&self.0.widgets);
            batch_response(&asked.ids, |id| {
                widgets.get(id).map(|name| Widget {
                    id: id.to_owned(),
                    name: name.clone(),
                    ..Default::default()
                })
            })
        };
        Response::ok(response)
    }

    async fn list_widget_parts<'a>(
        &'a self,
        ctx: RequestContext,
        request: ServiceRequest<'_, ListWidgetPartsRequest>,
    ) -> ServiceResult<impl connectrpc::Encodable<ListWidgetPartsResponse> + Send + use<'a>> {
        let asked = request.to_owned_message();
        self.0.admit(&ctx)?;
        if !lock(&self.0.widgets).contains_key(&asked.widget_id) {
            return Err(ConnectError::not_found("no such widget"));
        }
        let index = paged_index(&asked.page.into_option().unwrap_or_default().cursor);
        let pages = lock(&self.0.parts)
            .get(&asked.widget_id)
            .cloned()
            .unwrap_or_default();
        Response::ok(ListWidgetPartsResponse {
            items: pages.get(index).cloned().unwrap_or_default(),
            page: PageResponse {
                next_cursor: next_cursor(index, pages.len()),
                revision: REVISION.into(),
                ..Default::default()
            }
            .into(),
            ..Default::default()
        })
    }

    unserved_widget_methods!(
        "the feature serves only GetWidget, BatchGetWidgets and ListWidgetParts",
        serving list_widget_parts
    );
}

/// The revision every page of the numbers list carries.
const REVISION: &str = "r1";

/// The zero-based index of the page a cursor `cN` names; the first page for
/// an empty cursor.
fn paged_index(cursor: &str) -> usize {
    cursor
        .strip_prefix('c')
        .and_then(|index| index.parse::<usize>().ok())
        .unwrap_or(0)
}

/// The cursor of the page after `index` in a list of `pages` pages; empty at
/// the end.
fn next_cursor(index: usize, pages: usize) -> String {
    if index + 1 < pages {
        format!("c{}", index + 1)
    } else {
        String::new()
    }
}

/// What the numbers server answers a list with: a page, or bytes no decoder
/// reads.
enum NumbersBody {
    Page(ListNumbersResponse),
    Undecodable,
}

impl Encodable<ListNumbersResponse> for NumbersBody {
    fn encode(&self, codec: CodecFormat) -> Result<Bytes, ConnectError> {
        match self {
            Self::Page(page) => Encodable::<ListNumbersResponse>::encode(page, codec),
            // A varint tag whose continuation never ends.
            Self::Undecodable => Ok(Bytes::from_static(&[0xff, 0xff, 0xff])),
        }
    }
}

/// The `Numbers` service of the lane's server.
#[derive(Clone)]
struct NumbersService(Arc<Shared>);

impl NumbersService {
    /// The page the request asks for; its cursor `cN` names page `N`.
    async fn page(&self, request: ListNumbersRequest) -> Result<NumbersBody, ConnectError> {
        let index = paged_index(&request.page.into_option().unwrap_or_default().cursor);
        self.0.switch_if_scripted_on_page(index);
        let delay = self.0.page_delay_ms.load(Ordering::SeqCst);
        if delay > 0 {
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
        if *lock(&self.0.expire_at_page) == Some(index) {
            return Err(with_error_info(
                ConnectError::new(ErrorCode::FailedPrecondition, "the cursor expired"),
                &ErrorInfo {
                    code: "CURSOR_EXPIRED".into(),
                    emitter: "numbers".into(),
                    ..Default::default()
                },
            ));
        }
        if self.0.garble.load(Ordering::SeqCst) {
            return Ok(NumbersBody::Undecodable);
        }
        let pages = lock(&self.0.pages).clone();
        Ok(NumbersBody::Page(ListNumbersResponse {
            items: pages.get(index).cloned().unwrap_or_default(),
            page: PageResponse {
                next_cursor: next_cursor(index, pages.len()),
                revision: REVISION.into(),
                ..Default::default()
            }
            .into(),
            ..Default::default()
        }))
    }
}

impl Numbers for NumbersService {
    async fn list_no_side_effects<'a>(
        &'a self,
        ctx: RequestContext,
        request: ServiceRequest<'_, ListNumbersRequest>,
    ) -> ServiceResult<impl Encodable<ListNumbersResponse> + Send + use<'a>> {
        let request = request.to_owned_message();
        let page = request.page.clone().into_option().unwrap_or_default();
        self.0.admit_sized(&ctx, page.page_size)?;
        let first_page = page.cursor.is_empty();
        if first_page
            && header(&ctx, "bones-list-cache") == "1"
            && header(&ctx, "if-none-match") == format!("\"{REVISION}\"")
        {
            return Ok(
                Response::new(NumbersBody::Page(ListNumbersResponse::default()))
                    .with_header("bones-not-modified", "1"),
            );
        }
        Response::ok(self.page(request).await?)
    }

    async fn list_idempotent<'a>(
        &'a self,
        ctx: RequestContext,
        request: ServiceRequest<'_, ListNumbersRequest>,
    ) -> ServiceResult<impl Encodable<ListNumbersResponse> + Send + use<'a>> {
        self.0.admit(&ctx)?;
        Response::ok(self.page(request.to_owned_message()).await?)
    }

    async fn list<'a>(
        &'a self,
        ctx: RequestContext,
        request: ServiceRequest<'_, ListNumbersRequest>,
    ) -> ServiceResult<impl Encodable<ListNumbersResponse> + Send + use<'a>> {
        self.0.admit(&ctx)?;
        Response::ok(self.page(request.to_owned_message()).await?)
    }
}

#[derive(Debug, Default, cucumber::World)]
struct SdkCallWorld {
    shared: Arc<Shared>,
    address: Option<SocketAddr>,
    server: Option<tokio::task::JoinHandle<()>>,
    trace: Option<String>,
    /// The widget the caller got.
    widget: Option<L2Widget>,
    /// The per-id results of the batch the caller got.
    batch: Vec<Result<L2Widget, Error>>,
    /// The pages of numbers the caller read, in order.
    pages_read: Vec<Vec<u32>>,
    /// The pages of widget parts the caller received, by part name.
    part_pages: Vec<Vec<String>>,
    /// The `traceparent` of the span of the list walk.
    walk_span: Option<String>,
    /// Whether the walk gave nothing more after it ended.
    walk_over: bool,
    /// The numbers of the pages the caller received, in order.
    items_read: Vec<u32>,
    /// The first page the caller read on its own.
    first_page: Option<Page<Number>>,
    /// The list the caller opened and reads page by page.
    list: Option<Arc<Handle<Page<Number>>>>,
    /// The read of the next page of the list that is still waiting for its
    /// page.
    pending: Option<tokio::task::JoinHandle<Result<Option<Page<Number>>, Error>>>,
    /// The ticks the caller received.
    ticks: Vec<Tick>,
    /// The watch the caller opened and reads tick by tick.
    watch: Option<Handle<Tick>>,
    /// Whether the watch gave nothing more after it ended.
    watch_over: bool,
    /// How long the caller's watch lasted, from the call to the end of the
    /// stream.
    watched: Duration,
    /// The pages that filled the revision cache, held so the cache's weak
    /// chain stays replayable.
    held: Vec<Page<Number>>,
    /// Every caller that filled the revision cache, in order.
    cache_owners: Vec<Identity>,
    /// Whether each page the caller read came from the revision cache.
    pages_cached: Vec<bool>,
    /// Why the call, the chunk, a page or a tick was refused.
    refusal: Option<Error>,
    /// The page size the caller's numbers client asks for when a call names
    /// none.
    default_page_size: Option<u32>,
}

impl Drop for SdkCallWorld {
    fn drop(&mut self) {
        if let Some(server) = self.server.take() {
            server.abort();
        }
    }
}

impl SdkCallWorld {
    fn seen(&self) -> Vec<Seen> {
        lock(&self.shared.seen).clone()
    }
}

/// Starts the lane's server once: the `Widgets`, `Numbers` and `Events`
/// services on one loopback address.
async fn serve(world: &mut SdkCallWorld) {
    if world.shared.started.swap(true, Ordering::SeqCst) {
        return;
    }
    let bound = Server::bind("127.0.0.1:0")
        .await
        .expect("the lane's server binds");
    world.address = Some(
        bound
            .local_addr()
            .expect("the lane's server has an address"),
    );
    let shared = &world.shared;
    let router = WidgetsExt::register(Arc::new(Service(shared.clone())), Router::new());
    let router = NumbersExt::register(Arc::new(NumbersService(shared.clone())), router);
    let router = EventsExt::register(Arc::new(stream::EventsService(shared.clone())), router);
    world.server = Some(tokio::spawn(async move {
        let _ = bound
            .serve_with_service(ConnectRpcService::new(router))
            .await;
    }));
}

/// Where the lane's server listens.
fn base_url(world: &SdkCallWorld) -> String {
    let address = world.address.expect("a Given started the lane's server");
    format!("http://{address}")
}

/// What every call of the lane's clients runs in: the caller's credential
/// source, re-read before every send, and the span record.
fn call_env(world: &SdkCallWorld) -> CallEnv {
    let shared = world.shared.clone();
    let told = world.shared.clone();
    let env = if shared.anonymous.load(Ordering::SeqCst) {
        CallEnv::anonymous()
    } else {
        CallEnv::new(move || -> Result<Credential, ConnectError> {
            if shared.source_broken.load(Ordering::SeqCst) {
                return Err(ConnectError::unavailable("the credential source is down"));
            }
            let read = shared.reads.fetch_add(1, Ordering::SeqCst);
            let credential = lock(&shared.credential).clone();
            match lock(&shared.change_before_send).as_deref() {
                Some(subject) if read >= 1 => Ok(as_subject(&credential, subject)),
                _ => Ok(credential),
            }
        })
    };
    env.with_tracer(Tracer::new(Arc::new(Recorder(world.shared.clone()))))
        .with_revision_cache(world.shared.revisions.0.clone())
        .on_unauthenticated(Arc::new(move || {
            told.told.fetch_add(1, Ordering::SeqCst);
        }))
}

/// `numbers` as the comma-separated list the feature names a page with.
fn join_numbers(numbers: &[u32]) -> String {
    numbers
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

fn numbers(list: &str) -> Vec<u32> {
    list.split(',')
        .filter(|number| !number.is_empty())
        .map(|number| number.parse().expect("a number"))
        .collect()
}

#[given(expr = "the widgets server has the widget {string} named {string}")]
async fn given_widget(world: &mut SdkCallWorld, id: String, name: String) {
    lock(&world.shared.widgets).insert(id, name);
    serve(world).await;
}

#[given(expr = "the widget {string} has the parts {string}")]
fn given_parts(world: &mut SdkCallWorld, id: String, pages: String) {
    let pages = pages
        .split('|')
        .map(|page| page.split(',').map(str::to_owned).collect())
        .collect();
    lock(&world.shared.parts).insert(id, pages);
}

#[given(expr = "the numbers server has the pages {string}")]
async fn given_pages(world: &mut SdkCallWorld, pages: String) {
    *lock(&world.shared.pages) = pages.split('|').map(numbers).collect();
    serve(world).await;
}

#[given(expr = "the numbers server takes {int} ms to answer each page")]
fn given_page_delay(world: &mut SdkCallWorld, millis: u64) {
    world.shared.page_delay_ms.store(millis, Ordering::SeqCst);
}

#[given(expr = "the widgets server refuses the first {int} requests as unavailable")]
fn given_refusals(world: &mut SdkCallWorld, refusals: usize) {
    world.shared.refusals.store(refusals, Ordering::SeqCst);
}

#[given(
    expr = "the widgets server refuses the first {int} requests as resource exhausted with a Retry-After of {int} seconds"
)]
fn given_throttled(world: &mut SdkCallWorld, refusals: usize, seconds: u64) {
    world.shared.retry_after_s.store(seconds, Ordering::SeqCst);
    world.shared.exhausted.store(refusals, Ordering::SeqCst);
}

#[given(expr = "the SDK caller is {string} of organization {string}")]
fn given_caller(world: &mut SdkCallWorld, subject: String, org: String) {
    *lock(&world.shared.credential) = Credential {
        identity: Identity { subject, org },
        headers: vec![("authorization".to_owned(), b"Bearer caller".to_vec())],
    };
}

#[given(expr = "the numbers client defaults to the page size {int}")]
fn given_default_page_size(world: &mut SdkCallWorld, size: u32) {
    world.default_page_size = Some(size);
}

#[given("the caller is anonymous")]
fn given_anonymous(world: &mut SdkCallWorld) {
    world.shared.anonymous.store(true, Ordering::SeqCst);
}

#[given(expr = "the caller switches to {string} of organization {string}")]
fn caller_switches(world: &mut SdkCallWorld, subject: String, org: String) {
    world
        .shared
        .credential
        .lock()
        .expect("the recorder is never poisoned")
        .identity = Identity { subject, org };
}

#[given("the caller's token is refreshed")]
fn when_token_refreshed(world: &mut SdkCallWorld) {
    lock(&world.shared.credential).headers =
        vec![("authorization".to_owned(), b"Bearer refreshed".to_vec())];
}

#[given(expr = "the server refreshes the caller's token to {string} while it handles a request")]
fn given_refresh_on_request(world: &mut SdkCallWorld, authorization: String) {
    *lock(&world.shared.refresh_to) = Some(authorization.into_bytes());
}

#[given(expr = "the caller is inside the trace {string} at the span {string}")]
fn given_trace(world: &mut SdkCallWorld, trace: String, span: String) {
    world.trace = Some(format!("00-{trace}-{span}-01"));
}

#[given(expr = "the caller's identity changes to {string} before the first send")]
fn given_change_before_send(world: &mut SdkCallWorld, subject: String) {
    *lock(&world.shared.change_before_send) = Some(subject);
}

#[given(expr = "the server switches the caller to {string} while it handles a request")]
fn given_switch(world: &mut SdkCallWorld, subject: String) {
    *lock(&world.shared.switch_to) = Some(subject);
}

#[given(
    expr = "the server switches the caller to {string} while it handles the request for page {int}"
)]
fn given_switch_on_page(world: &mut SdkCallWorld, subject: String, page: usize) {
    *lock(&world.shared.switch_on_page) = Some((page - 1, subject));
}

#[given(expr = "the {word} server refuses the first {int} requests as unauthenticated")]
fn given_unauthenticated(world: &mut SdkCallWorld, _service: String, refusals: usize) {
    world
        .shared
        .unauthenticated
        .store(refusals, Ordering::SeqCst);
}

#[given("the caller's credential source cannot answer")]
fn given_source_broken(world: &mut SdkCallWorld) {
    world.shared.source_broken.store(true, Ordering::SeqCst);
}

/// Fills the caller env's revision cache the way a caller does: one complete
/// walk of the numbers list through the SDK, its pages still held. The walk
/// that fills it is not the call under test, so what it sent, read and
/// recorded is forgotten.
#[given("the revision cache holds the numbers list")]
async fn given_cached_numbers(world: &mut SdkCallWorld) {
    let owner = lock(&world.shared.credential).identity.clone();
    world.cache_owners.push(owner);
    let list = open_numbers(world).await;
    while let Some(page) = list.next().await.expect("every page of the filling walk") {
        world.held.push(page);
    }
    assert!(
        !world.shared.revisions.0.is_empty(),
        "the filling walk left the revision cache empty"
    );
    lock(&world.shared.seen).clear();
    lock(&world.shared.starts).clear();
    lock(&world.shared.ends).clear();
    world.shared.reads.store(0, Ordering::SeqCst);
}

#[when(expr = "the caller gets the widget {string} through the SDK")]
async fn when_get(world: &mut SdkCallWorld, id: String) {
    let client = api_bones_test_service_l2::WidgetsClient::new(&base_url(world), call_env(world))
        .expect("the lane's server address is a URI");
    let call = client.get_widget(id);
    let outcome = match world.trace.clone() {
        Some(traceparent) => {
            let ctx = CallCtx {
                traceparent: Some(traceparent),
                ..Default::default()
            };
            CallCtx::scope(ctx, call).await
        }
        None => call.await,
    };
    match outcome {
        Ok(widget) => world.widget = Some(widget),
        Err(error) => world.refusal = Some(error),
    }
}

#[when(expr = "the caller gets the widgets {string} through the SDK")]
async fn when_batch_get(world: &mut SdkCallWorld, ids: String) {
    let ids: Vec<String> = ids.split(',').map(str::to_owned).collect();
    let client = api_bones_test_service_l2::WidgetsClient::new(&base_url(world), call_env(world))
        .expect("the lane's server address is a URI");
    let batch = client.batch_get_widgets(ids).await;
    match batch.next().await {
        Ok(Some(chunk)) => world.batch = chunk,
        Ok(None) => panic!("the batch delivered no chunk"),
        Err(error) => world.refusal = Some(error),
    }
}

#[when(expr = "the caller lists the parts of the widget {string} through the SDK")]
async fn when_list_parts(world: &mut SdkCallWorld, id: String) {
    list_parts(world, id).await;
}

async fn list_parts(world: &mut SdkCallWorld, id: String) {
    let client = api_bones_test_service_l2::WidgetsClient::new(&base_url(world), call_env(world))
        .expect("the lane's server address is a URI");
    let list = client.list_widget_parts(id, None).await;
    loop {
        match list.next().await {
            Ok(Some(page)) => world
                .part_pages
                .push(page.items.iter().map(|part| part.name.clone()).collect()),
            Ok(None) => return,
            Err(error) => {
                world.refusal = Some(error);
                return;
            }
        }
    }
}

async fn open_numbers(world: &SdkCallWorld) -> Handle<Page<Number>> {
    open_numbers_at(world, &base_url(world)).await
}

async fn open_numbers_at(world: &SdkCallWorld, url: &str) -> Handle<Page<Number>> {
    api_bones_test_service_l2::NumbersClient::new(url, call_env(world))
        .expect("the lane's server address is a URI")
        .with_default_page_size(world.default_page_size)
        .list(None)
        .await
}

/// The lane's server under another address: the same server and revisions, a
/// different origin for the caller's cache.
#[given(
    "the caller has opened a list of the numbers at another address of the server through the SDK"
)]
async fn given_list_opened_elsewhere(world: &mut SdkCallWorld) {
    let port = world
        .address
        .expect("a Given started the lane's server")
        .port();
    let list = open_numbers_at(world, &format!("http://localhost:{port}")).await;
    world.walk_span = list.span();
    world.list = Some(Arc::new(list));
}

#[given("the caller has opened a list of the numbers through the SDK")]
async fn given_list_opened(world: &mut SdkCallWorld) {
    let list = open_numbers(world).await;
    world.walk_span = list.span();
    world.list = Some(Arc::new(list));
}

/// Starts a read of the next page of the list that the caller does not wait
/// for, and returns once the numbers server has the request: the list is then
/// busy with that read.
#[given("a read of the next page of the list is pending")]
async fn given_read_pending(world: &mut SdkCallWorld) {
    let list = Arc::clone(world.list.as_ref().expect("a Given opened the list"));
    world.pending = Some(tokio::spawn(async move { list.next().await }));
    for _ in 0..500 {
        if !world.seen().is_empty() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the pending read never reached the numbers server");
}

/// Cancels the list; shared by the Given and the When that name it.
fn cancel_list(world: &SdkCallWorld) {
    world
        .list
        .as_ref()
        .expect("a Given opened the list")
        .cancel();
}

#[given("the caller has cancelled the list")]
fn given_list_cancelled(world: &mut SdkCallWorld) {
    cancel_list(world);
}

#[when("the caller cancels the list")]
fn when_list_cancelled(world: &mut SdkCallWorld) {
    cancel_list(world);
}

#[given("the caller has read the first page of the list")]
async fn given_first_page_read(world: &mut SdkCallWorld) {
    when_next_page(world).await;
}

#[given("the caller has read the next page of the list")]
async fn given_next_page_read(world: &mut SdkCallWorld) {
    when_next_page(world).await;
}

#[when("the caller reads the next page of the list")]
async fn when_next_page(world: &mut SdkCallWorld) {
    match world
        .list
        .as_ref()
        .expect("a Given opened the list")
        .next()
        .await
    {
        Ok(Some(page)) => {
            world.pages_cached.push(page.from_cache);
            world
                .pages_read
                .push(page.items.iter().map(|number| number.value).collect());
        }
        Ok(None) => world.walk_over = true,
        Err(error) => world.refusal = Some(error),
    }
}

/// The page after the one read is fetched ahead while the caller still reads
/// the first: the step waits until the call helper has ended the client spans
/// of both pages, which it does once each page's answer has arrived.
#[given("the page after it has been fetched ahead")]
async fn when_page_fetched_ahead(world: &mut SdkCallWorld) {
    for _ in 0..500 {
        if lock(&world.shared.ends).len() >= 2 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the page after the first was never fetched ahead");
}

#[then(expr = "the response is the widget {string} named {string}")]
fn then_widget(world: &mut SdkCallWorld, id: String, name: String) {
    let widget = world.widget.as_ref().expect("the call succeeded");
    assert_eq!(widget.id, id);
    assert_eq!(widget.name, name);
}

#[then(expr = "the batch delivered the widgets {string} in request order")]
fn then_batch(world: &mut SdkCallWorld, expected: String) {
    let delivered: Vec<String> = world
        .batch
        .iter()
        .map(|result| {
            let widget = result.as_ref().expect("the widget was delivered");
            format!("{}={}", widget.id, widget.name)
        })
        .collect();
    assert_eq!(delivered.join(","), expected);
}

#[then(expr = "the caller received the pages {string}")]
fn then_pages(world: &mut SdkCallWorld, expected: String) {
    let received: Vec<String> = world
        .pages_read
        .iter()
        .map(|page| join_numbers(page))
        .collect();
    assert_eq!(received.join("|"), expected);
}

#[then(expr = "the caller received the part pages {string}")]
fn then_part_pages(world: &mut SdkCallWorld, expected: String) {
    let received: Vec<String> = world.part_pages.iter().map(|page| page.join(",")).collect();
    assert_eq!(received.join("|"), expected);
}

#[then("every page the caller received came from the revision cache")]
fn then_pages_cached(world: &mut SdkCallWorld) {
    assert!(
        !world.pages_cached.is_empty(),
        "the caller received no page"
    );
    assert!(
        world.pages_cached.iter().all(|cached| *cached),
        "pages from the cache: {:?}",
        world.pages_cached
    );
}

#[then("no page the caller received came from the revision cache")]
fn then_no_page_cached(world: &mut SdkCallWorld) {
    assert!(
        !world.pages_cached.is_empty(),
        "the caller received no page"
    );
    assert!(
        world.pages_cached.iter().all(|cached| !*cached),
        "pages from the cache: {:?}",
        world.pages_cached
    );
}

#[then("the walk is over")]
fn then_walk_over(world: &mut SdkCallWorld) {
    assert!(world.walk_over, "the walk gave more after it ended");
}

#[then(expr = "the tracer recorded {int} client spans named {string} under the span of the walk")]
fn then_client_spans(world: &mut SdkCallWorld, count: usize, name: String) {
    let walk = world
        .walk_span
        .as_deref()
        .and_then(Traceparent::parse)
        .expect("the walk has a recorded span");
    let starts = lock(&world.shared.starts).clone();
    let clients: Vec<&SpanStart> = starts
        .iter()
        .filter(|span| span.kind == SpanKind::Client && span.name == name)
        .collect();
    assert_eq!(clients.len(), count, "client spans: {starts:?}");
    for span in clients {
        let parent = span.parent.as_deref().and_then(Traceparent::parse);
        assert_eq!(
            parent,
            Some(walk),
            "the span {span:?} is not under the walk"
        );
    }
}

#[then("every span the tracer recorded ended")]
fn then_spans_ended(world: &mut SdkCallWorld) {
    let started = lock(&world.shared.starts).len();
    let ended = lock(&world.shared.ends).len();
    assert!(started > 0, "the tracer recorded no span");
    assert_eq!(ended, started);
}

#[then(expr = "the call is refused with the code {string}")]
fn then_refused(world: &mut SdkCallWorld, code: String) {
    let error = world.refusal.as_ref().expect("the call was refused");
    assert_eq!(error.code, code);
}

#[then(expr = "the call is refused with the platform code {string}")]
fn then_refused_with_platform(world: &mut SdkCallWorld, platform: String) {
    let error = world.refusal.as_ref().expect("the call was refused");
    assert_eq!(error.platform, platform, "refused with {error:?}");
}

#[then("the call is refused as unauthenticated")]
fn then_refused_unauthenticated(world: &mut SdkCallWorld) {
    let error = world.refusal.as_ref().expect("the call was refused");
    assert_eq!(error.platform, "UNAUTHORIZED", "refused with {error:?}");
}

/// The read the caller left pending, once it has finished.
async fn finished_read(world: &mut SdkCallWorld) -> Result<Option<Page<Number>>, Error> {
    let pending = world.pending.take().expect("a Given left a read pending");
    pending.await.expect("the pending read ran to its end")
}

#[then(expr = "the pending read is refused with the code {string}")]
async fn then_pending_refused(world: &mut SdkCallWorld, code: String) {
    let error = finished_read(world)
        .await
        .expect_err("the pending read was not refused");
    assert_eq!(error.code, code);
}

#[then(expr = "the pending read completes with the page {string}")]
async fn then_pending_completes(world: &mut SdkCallWorld, expected: String) {
    let page = finished_read(world)
        .await
        .expect("the pending read was refused")
        .expect("the list had a page");
    let numbers: Vec<u32> = page.items.iter().map(|number| number.value).collect();
    assert_eq!(join_numbers(&numbers), expected);
}

#[then(expr = "the span of the walk ended once with the status {string}")]
fn then_walk_span_status(world: &mut SdkCallWorld, status: String) {
    let walk = world
        .walk_span
        .as_deref()
        .expect("the walk has a recorded span");
    let ends = lock(&world.shared.ends).clone();
    let walk_ends: Vec<&SpanEnd> = ends.iter().filter(|end| end.traceparent == walk).collect();
    assert_eq!(walk_ends.len(), 1, "spans ended: {ends:?}");
    assert_eq!(walk_ends[0].status.wire_name(), status);
}

#[then(
    expr = "the host was told {int} time\\(s\\) that a server refused the caller as unauthenticated"
)]
fn then_host_told(world: &mut SdkCallWorld, times: usize) {
    assert_eq!(world.shared.told.load(Ordering::SeqCst), times);
}

#[then("the revision cache still holds the numbers list of the other caller")]
async fn then_cache_kept(world: &mut SdkCallWorld) {
    let owner = world
        .cache_owners
        .first()
        .expect("a Given filled the revision cache")
        .clone();
    lock(&world.shared.credential).identity = owner;
    let list = open_numbers(world).await;
    let first = list
        .next()
        .await
        .expect("the first page of the other caller's list")
        .expect("the list had a page");
    assert!(
        first.from_cache,
        "the refusal emptied the other caller's cache"
    );
}

#[then("the revision cache no longer holds the numbers list of the caller who was replaying")]
async fn then_replaying_cache_gone(world: &mut SdkCallWorld) {
    let owner = world
        .cache_owners
        .last()
        .expect("a Given filled the revision cache")
        .clone();
    lock(&world.shared.credential).identity = owner;
    let list = open_numbers(world).await;
    let first = list
        .next()
        .await
        .expect("the first page of the replaying caller's list")
        .expect("the list had a page");
    assert!(
        !first.from_cache,
        "the cache kept a chain of the caller whose walk was refused"
    );
}

#[then("the caller's revision cache is empty")]
fn then_cache_empty(world: &mut SdkCallWorld) {
    assert!(
        world.shared.revisions.0.is_empty(),
        "the cache kept a chain"
    );
}

#[then(expr = "the tracer started {int} client span(s) named {string}")]
fn then_span_started(world: &mut SdkCallWorld, count: usize, name: String) {
    let starts = lock(&world.shared.starts).clone();
    assert_eq!(starts.len(), count, "spans started: {starts:?}");
    for span in &starts {
        assert_eq!(span.kind, SpanKind::Client, "span: {span:?}");
        assert_eq!(span.name, name);
    }
}

#[then(expr = "the tracer ended every span it started with the status {string}")]
fn then_span_status(world: &mut SdkCallWorld, status: String) {
    let started = lock(&world.shared.starts).len();
    let ends = lock(&world.shared.ends).clone();
    assert!(started > 0, "the tracer started no span");
    assert_eq!(ends.len(), started, "spans ended: {ends:?}");
    for end in &ends {
        assert_eq!(end.status.wire_name(), status, "span: {end:?}");
    }
}

#[then("every request the server saw carried the traceparent of a span the tracer ended")]
fn then_request_carried_span(world: &mut SdkCallWorld) {
    let seen = world.seen();
    assert!(!seen.is_empty(), "the server saw no request");
    let ends = lock(&world.shared.ends).clone();
    for request in seen {
        assert!(
            ends.iter()
                .any(|end| end.traceparent == request.traceparent),
            "no ended span is {:?}: {ends:?}",
            request.traceparent
        );
    }
}

#[then(expr = "the server saw {int} request\\(s\\)")]
fn then_count(world: &mut SdkCallWorld, count: usize) {
    assert_eq!(world.seen().len(), count);
}

#[then(expr = "every request the server saw asked for the page size {int}")]
fn then_page_size(world: &mut SdkCallWorld, size: u32) {
    let page = world
        .first_page
        .as_ref()
        .expect("a When read the first page");
    let asked: Vec<u32> = world.seen().iter().map(|seen| seen.page_size).collect();
    assert!(!asked.is_empty(), "the server saw no request");
    assert_eq!(asked, vec![size; asked.len()]);
    assert_eq!(page.items.len(), size as usize);
}

#[then("every request the server saw carried a valid traceparent")]
fn then_valid_traceparent(world: &mut SdkCallWorld) {
    let seen = world.seen();
    assert!(!seen.is_empty(), "the server saw no request");
    for request in seen {
        assert!(
            Traceparent::parse(&request.traceparent).is_some(),
            "not a traceparent: {:?}",
            request.traceparent
        );
    }
}

/// The `authorization` value of each request the server saw, in arrival order.
fn authorizations(world: &SdkCallWorld) -> Vec<String> {
    let seen = world.seen();
    assert!(!seen.is_empty(), "the server saw no request");
    seen.into_iter()
        .map(|request| request.authorization)
        .collect()
}

#[then("every request the server saw carried the caller's credential")]
fn then_request_carried_credential(world: &mut SdkCallWorld) {
    let credential = lock(&world.shared.credential).clone();
    let (_, expected) = credential
        .headers
        .iter()
        .find(|(name, _)| name == "authorization")
        .expect("the caller has an authorization header");
    let expected = String::from_utf8_lossy(expected);
    for authorization in authorizations(world) {
        assert_eq!(authorization, expected);
    }
}

#[then(expr = "the requests the server saw carried the credentials {string} in order")]
fn then_credentials_in_order(world: &mut SdkCallWorld, expected: String) {
    assert_eq!(authorizations(world).join(","), expected);
}

#[then("no request the server saw carried a credential")]
fn then_no_credential(world: &mut SdkCallWorld) {
    for authorization in authorizations(world) {
        assert_eq!(authorization, "", "the request carried a credential");
    }
}

#[then(
    expr = "every request the server saw carried a traceparent in the trace {string} under a span other than {string}"
)]
fn then_child_traceparent(world: &mut SdkCallWorld, trace: String, span: String) {
    let seen = world.seen();
    assert!(!seen.is_empty(), "the server saw no request");
    for request in seen {
        let parts: Vec<&str> = request.traceparent.split('-').collect();
        assert!(
            Traceparent::parse(&request.traceparent).is_some(),
            "not a traceparent: {:?}",
            request.traceparent
        );
        assert_eq!(parts[1], trace);
        assert_ne!(parts[2], span);
    }
}

#[then(
    expr = "every request the server saw stated a call budget above {int} ms and at most {int} ms"
)]
fn then_budget(world: &mut SdkCallWorld, above: u64, at_most: u64) {
    let seen = world.seen();
    assert!(!seen.is_empty(), "the server saw no request");
    for request in seen {
        assert!(
            request.timeout_ms > above && request.timeout_ms <= at_most,
            "connect-timeout-ms was {}",
            request.timeout_ms
        );
    }
}

#[then("the request the server saw stated no call budget")]
fn then_no_budget(world: &mut SdkCallWorld) {
    let seen = world.seen();
    assert_eq!(seen.len(), 1, "the server saw {} requests", seen.len());
    assert_eq!(
        seen[0].timeout_ms, 0,
        "connect-timeout-ms was {}",
        seen[0].timeout_ms
    );
}

#[then("the call budget the server saw shrank from each request to the next")]
fn then_budget_shrank(world: &mut SdkCallWorld) {
    let seen = world.seen();
    assert!(seen.len() > 1, "the call was not retried");
    // A retry waits at least 50 ms, so the time left strictly drops.
    for pair in seen.windows(2) {
        assert!(
            pair[1].timeout_ms < pair[0].timeout_ms,
            "the budget did not shrink from {} to {}",
            pair[0].timeout_ms,
            pair[1].timeout_ms
        );
    }
}

#[given(expr = "the numbers server refuses the cursor of page {int} as expired")]
fn given_cursor_expired(world: &mut SdkCallWorld, page: usize) {
    *lock(&world.shared.expire_at_page) = Some(page - 1);
}

#[given("the numbers server answers pages that cannot be decoded")]
async fn given_undecodable(world: &mut SdkCallWorld) {
    world.shared.garble.store(true, Ordering::SeqCst);
    serve(world).await;
}

#[given("the tracer does not record")]
fn given_tracer_silent(world: &mut SdkCallWorld) {
    world.shared.silent.store(true, Ordering::SeqCst);
}

/// Walks the numbers list the way a Rust caller does, page by page, taking the
/// items of each page in order; leaves it, as a caller who stops reading does,
/// once `leave_after` items were taken.
async fn walk_numbers(world: &mut SdkCallWorld, leave_after: Option<usize>) {
    let list = open_numbers(world).await;
    loop {
        match list.next().await {
            Ok(Some(page)) => {
                let numbers: Vec<u32> = page.items.iter().map(|number| number.value).collect();
                let Some(limit) = leave_after else {
                    world.items_read.extend(&numbers);
                    world.pages_read.push(numbers);
                    continue;
                };
                let room = limit - world.items_read.len();
                world.items_read.extend(numbers.iter().take(room));
                if world.items_read.len() == limit {
                    list.cancel();
                    break;
                }
            }
            Ok(None) => {
                world.walk_over = true;
                break;
            }
            Err(error) => {
                world.refusal = Some(error);
                break;
            }
        }
    }
    world.list = Some(Arc::new(list));
}

#[when("the caller walks the numbers list by pages through the SDK")]
async fn when_walk_pages(world: &mut SdkCallWorld) {
    walk_numbers(world, None).await;
}

#[when("the caller walks the numbers list by items through the SDK")]
async fn when_walk_items(world: &mut SdkCallWorld) {
    walk_numbers(world, None).await;
}

#[when(expr = "the caller walks the numbers list by items and leaves after {int} items")]
async fn when_walk_items_and_leave(world: &mut SdkCallWorld, items: usize) {
    walk_numbers(world, Some(items)).await;
}

#[when("the caller reads the first page of the numbers list through the SDK")]
async fn when_first_page(world: &mut SdkCallWorld) {
    let list = open_numbers(world).await;
    world.first_page = Some(
        list.next()
            .await
            .expect("the first page of the list")
            .expect("the list had a first page"),
    );
    list.cancel();
}

#[then(expr = "the caller received the items {string}")]
fn then_items(world: &mut SdkCallWorld, expected: String) {
    assert_eq!(join_numbers(&world.items_read), expected);
}

#[then("the walk ended without refusal")]
fn then_walk_clean(world: &mut SdkCallWorld) {
    assert!(
        world.refusal.is_none(),
        "the walk was refused: {:?}",
        world.refusal
    );
    assert!(world.walk_over, "the walk gave more after it ended");
}

#[then("the list gives nothing after the caller left")]
async fn then_list_left(world: &mut SdkCallWorld) {
    let list = world.list.as_ref().expect("the caller walked the list");
    assert!(
        matches!(list.next().await, Ok(None)),
        "the list gave more after the caller left"
    );
}

#[then("the walk is refused")]
fn then_walk_refused(world: &mut SdkCallWorld) {
    assert!(world.refusal.is_some(), "the walk was not refused");
}

#[then(
    expr = "the walk is refused with a PageError of kind {string} after {int} pages and {int} items"
)]
fn then_walk_page_error(world: &mut SdkCallWorld, kind: String, pages: u32, items: u64) {
    let error = world.refusal.as_ref().expect("the walk was refused");
    let detail = error
        .page
        .as_ref()
        .unwrap_or_else(|| panic!("the refusal names no page failure: {error:?}"));
    assert_eq!(detail.kind.wire_name(), kind);
    assert_eq!(detail.delivered_pages, pages);
    assert_eq!(detail.delivered_items, items);
}

#[then(expr = "the first page holds the numbers {string}")]
fn then_first_page_numbers(world: &mut SdkCallWorld, expected: String) {
    let page = world
        .first_page
        .as_ref()
        .expect("a When read the first page");
    let numbers: Vec<u32> = page.items.iter().map(|number| number.value).collect();
    assert_eq!(join_numbers(&numbers), expected);
}

#[then("the first page says a next page follows")]
fn then_first_page_has_next(world: &mut SdkCallWorld) {
    let page = world
        .first_page
        .as_ref()
        .expect("a When read the first page");
    assert!(page.has_next);
}

#[then("the first page says no next page follows")]
fn then_first_page_has_no_next(world: &mut SdkCallWorld) {
    let page = world
        .first_page
        .as_ref()
        .expect("a When read the first page");
    assert!(!page.has_next);
}

/// The span of the open list: the `traceparent` of a span the tracer started
/// and has not ended, under which the list's pages are read.
#[then("the numbers list exposes the span of its walk")]
fn then_list_span(world: &mut SdkCallWorld) {
    let walk = world
        .list
        .as_ref()
        .expect("a Given opened the list")
        .span()
        .expect("the open list exposes no span");
    let ends = lock(&world.shared.ends).clone();
    assert!(
        ends.iter().all(|end| end.traceparent != walk),
        "the span of the walk already ended: {ends:?}"
    );
    let walk = Traceparent::parse(&walk).expect("the span of the walk is a traceparent");
    let starts = lock(&world.shared.starts).clone();
    assert!(
        starts
            .iter()
            .any(|span| span.parent.as_deref().and_then(Traceparent::parse) == Some(walk)),
        "no span the tracer started is under the span of the walk: {starts:?}"
    );
}

#[then("the numbers list exposes no span")]
fn then_list_no_span(world: &mut SdkCallWorld) {
    let list = world.list.as_ref().expect("a Given opened the list");
    assert_eq!(list.span(), None);
}

#[tokio::main]
async fn main() {
    let features = format!(
        "{}/../features/sdk_call.feature",
        env!("CARGO_MANIFEST_DIR")
    );
    let two_pass = TwoPass {
        skip_tags: vec!["wip".into()],
        isolated_tag: "isolated".into(),
        parallel: 1,
    };
    two_pass.run::<SdkCallWorld>(features).await;
}
