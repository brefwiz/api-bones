// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! Runs `../features/sdk_call.feature`: the call helper every Layer 2 method
//! runs through, driven as a Layer 2 author drives it (`CallEnv::call(..)
//! .unary|list|batch|stream` with the test service's Layer 2 send builders)
//! against a real connectrpc server on loopback that records what each
//! request carried.
//!
//! `TwoPass` owns scenario selection, concurrency and the census.

use std::collections::HashMap;
use std::fmt;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use api_bones::connect::Idempotency;
use api_bones_batch::{Credential, Identity};
use api_bones_contract_rust::{
    BatchGetWidgetsRequest, BatchGetWidgetsResponse, EventsExt, GetWidgetRequest,
    ListNumbersRequest, ListNumbersResponse, Numbers, NumbersClient as NumbersRpc, NumbersExt,
    PageResponse, Widget, Widgets, WidgetsExt,
};
use api_bones_page::{
    ListCall as PageCall, ListResponse, MetadataFetch, Page, PageOptions, RevisionCache,
};
use api_bones_sdk::call::CallEnv;
use api_bones_sdk::{
    CallCtx, Error, Handle, SpanEnd, SpanKind, SpanSink, SpanStart, Traceparent, Tracer,
};
use api_bones_test_service_l2::{
    NUMBERS, Number, NumbersPageRequest, Tick, WIDGETS, Widget as L2Widget, batch_get_widgets_send,
    config_of, get_widget_send, list_send,
};
use api_bones_transport::HttpTransport;
use brefwiz_cucumber_steps::TwoPass;
use connectrpc::client::ClientConfig;
use connectrpc::{
    ConnectError, ConnectRpcService, RequestContext, Response, Router, Server, ServiceRequest,
    ServiceResult,
};
use cucumber::{given, then, when};
use futures_util::StreamExt as _;

#[path = "steps/stream.rs"]
mod stream;
#[path = "steps/support.rs"]
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
}

/// The server's script and record, the caller's credential source and the
/// caller's span record.
#[derive(Debug, Default)]
struct Shared {
    widgets: Mutex<HashMap<String, String>>,
    /// The pages the numbers server serves; the cursor `cN` names page `N`.
    pages: Mutex<Vec<Vec<u32>>>,
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
        lock(&self.seen).push(Seen {
            traceparent: header(ctx, "traceparent"),
            authorization: header(ctx, "authorization"),
            timeout_ms: header(ctx, "connect-timeout-ms")
                .parse()
                .unwrap_or_default(),
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
        lock(&self.0.starts).push(span.clone());
        let parent = span.parent.as_deref().and_then(Traceparent::parse);
        Some(Traceparent::mint(parent.as_ref()).to_string())
    }

    fn end(&self, end: &SpanEnd) {
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

    unserved_widget_methods!("the feature serves only GetWidget and BatchGetWidgets");
}

/// The `Numbers` service of the lane's server.
#[derive(Clone)]
struct NumbersService(Arc<Shared>);

impl NumbersService {
    /// The page the request asks for; its cursor `cN` names page `N`.
    async fn page(&self, request: ListNumbersRequest) -> ListNumbersResponse {
        let cursor = request.page.into_option().unwrap_or_default().cursor;
        let index = cursor
            .strip_prefix('c')
            .and_then(|index| index.parse::<usize>().ok())
            .unwrap_or(0);
        self.0.switch_if_scripted_on_page(index);
        let delay = self.0.page_delay_ms.load(Ordering::SeqCst);
        if delay > 0 {
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
        let pages = lock(&self.0.pages).clone();
        let next_cursor = if index + 1 < pages.len() {
            format!("c{}", index + 1)
        } else {
            String::new()
        };
        ListNumbersResponse {
            items: pages.get(index).cloned().unwrap_or_default(),
            page: PageResponse {
                next_cursor,
                revision: "r1".into(),
                ..Default::default()
            }
            .into(),
            ..Default::default()
        }
    }
}

impl Numbers for NumbersService {
    async fn list_no_side_effects<'a>(
        &'a self,
        ctx: RequestContext,
        request: ServiceRequest<'_, ListNumbersRequest>,
    ) -> ServiceResult<impl connectrpc::Encodable<ListNumbersResponse> + Send + use<'a>> {
        self.0.admit(&ctx)?;
        Response::ok(self.page(request.to_owned_message()).await)
    }

    async fn list_idempotent<'a>(
        &'a self,
        ctx: RequestContext,
        request: ServiceRequest<'_, ListNumbersRequest>,
    ) -> ServiceResult<impl connectrpc::Encodable<ListNumbersResponse> + Send + use<'a>> {
        self.0.admit(&ctx)?;
        Response::ok(self.page(request.to_owned_message()).await)
    }

    async fn list<'a>(
        &'a self,
        ctx: RequestContext,
        request: ServiceRequest<'_, ListNumbersRequest>,
    ) -> ServiceResult<impl connectrpc::Encodable<ListNumbersResponse> + Send + use<'a>> {
        self.0.admit(&ctx)?;
        Response::ok(self.page(request.to_owned_message()).await)
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
    /// The `traceparent` of the span of the list walk.
    walk_span: Option<String>,
    /// Whether the walk gave nothing more after it ended.
    walk_over: bool,
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
    held: Vec<Page<u32>>,
    /// Why the call, the chunk, a page or a tick was refused.
    refusal: Option<Error>,
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

#[given(expr = "the SDK caller is {string} of organization {string}")]
fn given_caller(world: &mut SdkCallWorld, subject: String, org: String) {
    *lock(&world.shared.credential) = Credential {
        identity: Identity { subject, org },
        headers: vec![("authorization".to_owned(), b"Bearer caller".to_vec())],
    };
}

#[given("the caller is anonymous")]
fn given_anonymous(world: &mut SdkCallWorld) {
    world.shared.anonymous.store(true, Ordering::SeqCst);
}

#[given(expr = "the caller switches to {string} of organization {string}")]
fn caller_switches(world: &mut SdkCallWorld, subject: String, org: String) {
    lock(&world.shared.credential).identity = Identity { subject, org };
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

/// Fills the caller env's revision cache the way a generated list client
/// does: one complete walk over the numbers server, its pages still held. The
/// requests that fill it are not the call under test, so they are not counted
/// among the ones the server saw.
#[given("the revision cache holds the numbers list")]
async fn given_cached_numbers(world: &mut SdkCallWorld) {
    let config = ClientConfig::new(base_url(world).parse().expect("a loopback uri"));
    let rpc = Arc::new(NumbersRpc::new(HttpTransport::h2c(), config));
    let fetch: MetadataFetch<ListNumbersRequest, ListNumbersResponse> =
        Arc::new(move |request, _metadata| {
            let rpc = Arc::clone(&rpc);
            Box::pin(async move {
                let response = rpc.list_no_side_effects(request).await?;
                Ok::<_, ConnectError>(ListResponse {
                    body: response.into_owned(),
                    not_modified: false,
                })
            })
        });
    let walk = PageCall::generated(
        "contract.v1.Numbers/ListNoSideEffects",
        Idempotency::NoSideEffects,
        fetch,
        ListNumbersRequest::default(),
        world.shared.revisions.0.clone(),
        Arc::new(|| "ada:acme".to_owned()),
        b"numbers",
    );
    let pages: Vec<_> = walk.pages().collect().await;
    world.held = pages
        .into_iter()
        .map(|page| page.expect("every page of the filling walk is delivered"))
        .collect();
    assert!(
        !world.shared.revisions.0.is_empty(),
        "the filling walk left the revision cache empty"
    );
    lock(&world.shared.seen).clear();
}

#[when(expr = "the caller gets the widget {string} through the SDK")]
async fn when_get(world: &mut SdkCallWorld, id: String) {
    let env = call_env(world);
    let config = config_of(&base_url(world)).expect("a loopback uri");
    let call = async {
        env.call(WIDGETS, "GetWidget")
            .unary(Idempotency::NoSideEffects, move |attempt| {
                get_widget_send(config.clone(), id.clone(), attempt)
            })
            .await
    };
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
    let env = call_env(world);
    let config = config_of(&base_url(world)).expect("a loopback uri");
    let ids: Vec<String> = ids.split(',').map(str::to_owned).collect();
    let batch = env
        .call(WIDGETS, "BatchGetWidgets")
        .batch(Idempotency::NoSideEffects, move |attempt| {
            batch_get_widgets_send(config.clone(), ids.clone(), attempt)
        });
    match batch.next().await {
        Ok(Some(chunk)) => world.batch = chunk,
        Ok(None) => panic!("the batch delivered no chunk"),
        Err(error) => world.refusal = Some(error),
    }
}

fn open_numbers(world: &SdkCallWorld) -> Handle<Page<Number>> {
    let env = call_env(world);
    let config = config_of(&base_url(world)).expect("a loopback uri");
    env.call(NUMBERS, "ListNoSideEffects").list(
        Idempotency::NoSideEffects,
        NumbersPageRequest::default(),
        list_send(config),
        PageOptions::default(),
    )
}

#[given("the caller has opened a list of the numbers through the SDK")]
async fn given_list_opened(world: &mut SdkCallWorld) {
    let list = open_numbers(world);
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
    let list = world.list.as_ref().expect("a Given opened the list");
    match list.next().await {
        Ok(Some(page)) => world
            .pages_read
            .push(page.items.iter().map(|number| number.value).collect()),
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
