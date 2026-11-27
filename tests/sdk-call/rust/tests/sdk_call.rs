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
use api_bones_batch::{Credential, CredentialAsk, Identity};
use api_bones_contract_rust::{
    BatchGetWidgetsRequest, BatchGetWidgetsResponse, EventsServiceExt, GetWidgetRequest,
    GetWidgetResponse, ListIdempotentRequest, ListIdempotentResponse, ListNoSideEffectsRequest,
    ListNoSideEffectsResponse, ListRequest, ListResponse, ListWidgetPartsRequest,
    ListWidgetPartsResponse, NumbersService, NumbersServiceExt, PageResponse, Widget,
    WidgetsService, WidgetsServiceExt,
};
use api_bones_page::{Page, RevisionCache};
use api_bones_sdk::call::CallEnv;
use api_bones_sdk::{
    CallCtx, Error, Handle, PageKind, SpanEnd, SpanKind, SpanSink, SpanStart, SpanStarted,
    SpanStatus, Traceparent, Tracer,
};
use api_bones_test_service_l2::{ApiBonesSdk, Number, Tick, Widget as L2Widget, WidgetPart};
use brefwiz_cucumber_steps::TwoPass;
use bytes::Bytes;
use connectrpc::{
    CodecFormat, ConnectError, Encodable, ErrorCode, RequestContext, Response, Router,
    ServiceRequest, ServiceResult,
};
use cucumber::{given, then, when};
use http::{HeaderMap, HeaderValue};

#[path = "steps/stream.rs"]
mod stream;
#[path = "../../../rust/tests/steps/support.rs"]
mod support;

use support::{batch_response, serve_loopback, unserved_widget_methods};

/// The name the scenarios give a span status.
fn status_name(status: SpanStatus) -> &'static str {
    match status {
        SpanStatus::Ok => "ok",
        SpanStatus::Error => "error",
        SpanStatus::Unset => "unset",
    }
}

/// The name the scenarios give the kind of a page failure.
fn page_kind_name(kind: PageKind) -> &'static str {
    match kind {
        PageKind::CursorInvalid => "cursor_invalid",
        PageKind::CursorExpired => "cursor_expired",
        PageKind::CursorQueryMismatch => "cursor_query_mismatch",
        PageKind::Rpc => "rpc",
    }
}

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
    /// Requests the server still fails as internal.
    failing: AtomicUsize,
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
    /// How many times the caller's source was asked to refresh.
    refreshes: AtomicUsize,
    /// What the caller's source answers when it is asked to refresh.
    on_refresh: Mutex<OnRefresh>,
    /// How long the caller's source takes to answer, in milliseconds.
    credential_delay_ms: AtomicU64,
    /// Whether the events server refuses its stream as unauthenticated after
    /// its first tick.
    refuse_after_tick: AtomicBool,
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

/// What the caller's credential source answers when a server refused its
/// credential and the core asks it to refresh.
#[derive(Debug, Default, Clone)]
enum OnRefresh {
    /// The same caller with a renewed `authorization`.
    #[default]
    Renew,
    /// Nothing new: the credential it answered last still stands.
    Unchanged,
    /// Another caller.
    Another(Identity),
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

    /// Refuses the cursor of the page the scripted expiry names.
    fn refuse_expired(&self, index: usize, emitter: &str) -> Result<(), ConnectError> {
        if *lock(&self.expire_at_page) != Some(index) {
            return Ok(());
        }
        Err(with_error_info(
            ConnectError::new(ErrorCode::FailedPrecondition, "the cursor expired"),
            &ErrorInfo {
                code: "CURSOR_EXPIRED".into(),
                emitter: emitter.into(),
                ..Default::default()
            },
        ))
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
        if spend(&self.failing) {
            return Err(ConnectError::internal("the server failed"));
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
    fn start(&self, span: &SpanStart) -> SpanStarted {
        if self.0.silent.load(Ordering::SeqCst) {
            return SpanStarted::default();
        }
        lock(&self.0.starts).push(span.clone());
        let parent = span.parent.as_deref().and_then(Traceparent::parse);
        SpanStarted::recording(Traceparent::mint(parent.as_ref()).to_string())
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

impl WidgetsService for Service {
    async fn get_widget<'a>(
        &'a self,
        ctx: RequestContext,
        request: ServiceRequest<'_, GetWidgetRequest>,
    ) -> ServiceResult<impl connectrpc::Encodable<GetWidgetResponse> + Send + use<'a>> {
        let asked = request.to_owned_message();
        self.0.admit(&ctx)?;
        self.0.switch_if_scripted();
        let name = lock(&self.0.widgets)
            .get(&asked.id)
            .cloned()
            .ok_or_else(|| ConnectError::not_found("no such widget"))?;
        Response::ok(GetWidgetResponse {
            widget: Widget {
                id: asked.id.clone(),
                name,
                ..Default::default()
            }
            .into(),
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
        let page = asked.page.clone().into_option().unwrap_or_default();
        self.0.admit_sized(&ctx, page.page_size)?;
        if !lock(&self.0.widgets).contains_key(&asked.widget_id) {
            return Err(ConnectError::not_found("no such widget"));
        }
        if unchanged(&ctx, &page.cursor) {
            return Ok(Response::new(ListWidgetPartsResponse::default())
                .with_header("bones-not-modified", "1"));
        }
        let index = paged_index(&page.cursor);
        self.0.refuse_expired(index, "widgets")?;
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

/// Whether a first-page read carries the current revision's validator, so the
/// answer is "not modified".
fn unchanged(ctx: &RequestContext, cursor: &str) -> bool {
    cursor.is_empty()
        && header(ctx, "bones-list-cache") == "1"
        && header(ctx, "if-none-match") == format!("\"{REVISION}\"")
}

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

/// What the numbers server answers a list with: the page as the response
/// message `R` of the RPC that was called, or bytes no decoder reads.
enum NumbersBody<R> {
    Page(R),
    Undecodable,
}

impl<R: Encodable<R>> Encodable<R> for NumbersBody<R> {
    fn encode(&self, codec: CodecFormat) -> Result<Bytes, ConnectError> {
        match self {
            Self::Page(page) => Encodable::<R>::encode(page, codec),
            // A varint tag whose continuation never ends.
            Self::Undecodable => Ok(Bytes::from_static(&[0xff, 0xff, 0xff])),
        }
    }
}

/// The numbers of one page and the page metadata that goes with them.
struct Numbered {
    items: Vec<u32>,
    page: PageResponse,
}

/// The numbers body for a list RPC, the page built into the response
/// message by `page`; a `None` page is the bytes no decoder reads.
fn numbers_body<R>(numbered: Option<Numbered>, page: impl FnOnce(Numbered) -> R) -> NumbersBody<R> {
    numbered.map_or(NumbersBody::Undecodable, |n| NumbersBody::Page(page(n)))
}

/// The `NumbersService` of the lane's server.
#[derive(Clone)]
struct NumbersServer(Arc<Shared>);

impl NumbersServer {
    /// The page the cursor `cN` names, page `N`; `None` when the server is
    /// set to garble its answers.
    async fn page(&self, cursor: &str) -> Result<Option<Numbered>, ConnectError> {
        let index = paged_index(cursor);
        self.0.switch_if_scripted_on_page(index);
        let delay = self.0.page_delay_ms.load(Ordering::SeqCst);
        if delay > 0 {
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
        self.0.refuse_expired(index, "numbers")?;
        if self.0.garble.load(Ordering::SeqCst) {
            return Ok(None);
        }
        let pages = lock(&self.0.pages).clone();
        Ok(Some(Numbered {
            items: pages.get(index).cloned().unwrap_or_default(),
            page: PageResponse {
                next_cursor: next_cursor(index, pages.len()),
                revision: REVISION.into(),
                ..Default::default()
            },
        }))
    }
}

impl NumbersService for NumbersServer {
    async fn list_no_side_effects<'a>(
        &'a self,
        ctx: RequestContext,
        request: ServiceRequest<'_, ListNoSideEffectsRequest>,
    ) -> ServiceResult<impl Encodable<ListNoSideEffectsResponse> + Send + use<'a>> {
        let request = request.to_owned_message();
        let page = request.page.clone().into_option().unwrap_or_default();
        self.0.admit_sized(&ctx, page.page_size)?;
        if unchanged(&ctx, &page.cursor) {
            return Ok(
                Response::new(NumbersBody::Page(ListNoSideEffectsResponse::default()))
                    .with_header("bones-not-modified", "1"),
            );
        }
        Response::ok(numbers_body(self.page(&page.cursor).await?, |n| {
            ListNoSideEffectsResponse {
                items: n.items,
                page: n.page.into(),
                ..Default::default()
            }
        }))
    }

    async fn list_idempotent<'a>(
        &'a self,
        ctx: RequestContext,
        request: ServiceRequest<'_, ListIdempotentRequest>,
    ) -> ServiceResult<impl Encodable<ListIdempotentResponse> + Send + use<'a>> {
        self.0.admit(&ctx)?;
        let page = request
            .to_owned_message()
            .page
            .into_option()
            .unwrap_or_default();
        Response::ok(numbers_body(self.page(&page.cursor).await?, |n| {
            ListIdempotentResponse {
                items: n.items,
                page: n.page.into(),
                ..Default::default()
            }
        }))
    }

    async fn list<'a>(
        &'a self,
        ctx: RequestContext,
        request: ServiceRequest<'_, ListRequest>,
    ) -> ServiceResult<impl Encodable<ListResponse> + Send + use<'a>> {
        self.0.admit(&ctx)?;
        let page = request
            .to_owned_message()
            .page
            .into_option()
            .unwrap_or_default();
        Response::ok(numbers_body(self.page(&page.cursor).await?, |n| {
            ListResponse {
                items: n.items,
                page: n.page.into(),
                ..Default::default()
            }
        }))
    }
}

/// The widgets client of the SDK the caller opened, shown by what it is.
struct OpenedWidgets {
    client: api_bones_test_service_l2::WidgetsClient,
}

impl fmt::Debug for OpenedWidgets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("OpenedWidgets(WidgetsClient)")
    }
}

/// The numbers client of the SDK the caller opened, shown by what it is.
struct OpenedNumbers {
    client: api_bones_test_service_l2::NumbersClient,
}

impl fmt::Debug for OpenedNumbers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("OpenedNumbers(NumbersClient)")
    }
}

/// The numbers client of the caller's one session, shown by what it is.
struct Session<T>(T);

impl<T> fmt::Debug for Session<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Session(NumbersClient)")
    }
}

#[derive(Debug, Default, cucumber::World)]
struct SdkCallWorld {
    /// The client whose session the scenario ends.
    session: Option<Session<api_bones_test_service_l2::NumbersClient>>,
    /// The widgets client of the SDK the scenario opened from the caller's
    /// session.
    opened_widgets: Option<OpenedWidgets>,
    /// The numbers client of the SDK the scenario opened from the caller's
    /// session.
    opened_numbers: Option<OpenedNumbers>,
    shared: Arc<Shared>,
    address: Option<SocketAddr>,
    server: Option<tokio::task::JoinHandle<()>>,
    trace: Option<String>,
    /// The widget the caller got.
    widget: Option<L2Widget>,
    /// The per-id results of the batch the caller got.
    batch: Vec<Result<L2Widget, Error>>,
    /// The results of the widget calls the caller made at once.
    concurrent: Vec<Result<L2Widget, Error>>,
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
    /// Whether the list the first page came from gave nothing after it.
    ended_after_first: Option<bool>,
    /// The list the caller opened and reads page by page.
    list: Option<Arc<Handle<Page<Number>>>>,
    /// The read of the next page of the list that is still waiting for its
    /// page.
    pending: Option<tokio::task::JoinHandle<Result<Option<Page<Number>>, Error>>>,
    /// What the pending read returned once it finished.
    pending_read: Option<Result<Option<Page<Number>>, Error>>,
    /// What the first page of a numbers list opened after the scenario's
    /// calls returned.
    probe: Option<Result<Option<Page<Number>>, Error>>,
    /// The ticks the caller received.
    ticks: Vec<Tick>,
    /// The watch the caller opened and reads tick by tick.
    watch: Option<Handle<Tick>>,
    /// Whether the watch gave nothing more after it ended.
    watch_over: bool,
    /// How long the caller's watch lasted, from the call to the end of the
    /// stream.
    watched: Duration,
    /// The pages that filled the revision cache, as the caller still holds
    /// them.
    held: Vec<Page<Number>>,
    /// The pages of the widget's parts that filled the revision cache, as the
    /// caller still holds them.
    held_parts: Vec<Page<WidgetPart>>,
    /// Every caller that filled the revision cache, in order.
    cache_owners: Vec<Identity>,
    /// Whether each page the caller read came from the revision cache.
    pages_cached: Vec<bool>,
    /// Why the call, the chunk, a page or a tick was refused.
    refusal: Option<Error>,
    /// What the last static function or constant the caller used answered.
    static_answer: Option<u32>,
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

/// Starts the lane's server once: the `WidgetsService`, `NumbersService` and
/// `EventsService` services on one loopback address.
async fn serve(world: &mut SdkCallWorld) {
    if world.shared.started.swap(true, Ordering::SeqCst) {
        return;
    }
    let shared = &world.shared;
    let router = WidgetsServiceExt::register(Arc::new(Service(shared.clone())), Router::new());
    let router = NumbersServiceExt::register(Arc::new(NumbersServer(shared.clone())), router);
    let router = EventsServiceExt::register(Arc::new(stream::EventsServer(shared.clone())), router);
    let (server, address) = serve_loopback(router).await;
    world.address = Some(address);
    world.server = Some(server);
}

/// Where the lane's server listens.
fn base_url(world: &SdkCallWorld) -> String {
    let address = world.address.expect("a Given started the lane's server");
    format!("http://{address}")
}

/// What the caller's credential source answers to one read.
async fn read_credential(shared: &Shared, ask: CredentialAsk) -> Result<Credential, ConnectError> {
    let delay = shared.credential_delay_ms.load(Ordering::SeqCst);
    if delay > 0 {
        tokio::time::sleep(Duration::from_millis(delay)).await;
    }
    if shared.source_broken.load(Ordering::SeqCst) {
        return Err(ConnectError::unavailable("the credential source is down"));
    }
    if ask.refresh {
        shared.refreshes.fetch_add(1, Ordering::SeqCst);
        let renewed = match lock(&shared.on_refresh).clone() {
            OnRefresh::Unchanged => return Ok(lock(&shared.credential).clone()),
            OnRefresh::Renew => None,
            OnRefresh::Another(identity) => Some(identity),
        };
        let mut credential = lock(&shared.credential);
        credential.headers = vec![("authorization".to_owned(), b"Bearer refreshed".to_vec())];
        if let Some(identity) = renewed {
            credential.identity = identity;
        }
        return Ok(credential.clone());
    }
    let read = shared.reads.fetch_add(1, Ordering::SeqCst);
    let credential = lock(&shared.credential).clone();
    Ok(match lock(&shared.change_before_send).as_deref() {
        Some(subject) if read >= 1 => as_subject(&credential, subject),
        _ => credential,
    })
}

/// What every call of the lane's clients runs in: the caller's credential
/// source, re-read before every send, and the span record.
fn call_env(world: &SdkCallWorld) -> CallEnv {
    let shared = world.shared.clone();
    let told = world.shared.clone();
    let env = if shared.anonymous.load(Ordering::SeqCst) {
        CallEnv::anonymous()
    } else {
        CallEnv::new(move |ask: CredentialAsk| {
            let shared = shared.clone();
            async move { read_credential(&shared, ask).await }
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

#[given(expr = "the widgets server fails the first {int} requests as internal")]
fn given_failing(world: &mut SdkCallWorld, failures: usize) {
    world.shared.failing.store(failures, Ordering::SeqCst);
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

/// A source that rejects and one that cannot answer are one failure to the
/// core: it answers nothing.
#[given("the caller's credential source rejects")]
fn given_source_rejects(world: &mut SdkCallWorld) {
    world.shared.source_broken.store(true, Ordering::SeqCst);
}

#[given(expr = "the caller's credential source answers after {int} ms")]
fn given_source_slow(world: &mut SdkCallWorld, millis: u64) {
    world
        .shared
        .credential_delay_ms
        .store(millis, Ordering::SeqCst);
}

#[given("the caller's credential source answers the same credential when asked to refresh")]
fn given_refresh_unchanged(world: &mut SdkCallWorld) {
    *lock(&world.shared.on_refresh) = OnRefresh::Unchanged;
}

#[given(
    expr = "the caller's credential source answers {string} of organization {string} when asked to refresh"
)]
fn given_refresh_another(world: &mut SdkCallWorld, subject: String, org: String) {
    *lock(&world.shared.on_refresh) = OnRefresh::Another(Identity { subject, org });
}

#[given(expr = "the numbers server has {int} pages of {int} numbers each")]
async fn given_sized_pages(world: &mut SdkCallWorld, pages: u32, size: u32) {
    *lock(&world.shared.pages) = (0..pages)
        .map(|page| (page * size + 1..=(page + 1) * size).collect())
        .collect();
    serve(world).await;
}

/// One complete walk of the numbers list that is not the call under test and
/// need not leave anything in the revision cache.
#[given("the caller has walked the numbers list through the SDK")]
async fn given_walked_numbers(world: &mut SdkCallWorld) {
    let list = open_numbers(world).await;
    hold_filling_walk(world, &list, false).await;
}

/// What the caller held of the walk is gone; the revision cache alone keeps
/// the chain.
#[given("the caller dropped every page and the host collected its garbage")]
fn given_pages_dropped(world: &mut SdkCallWorld) {
    world.held.clear();
    world.held_parts.clear();
}

/// Walks `list` to its end, keeping every page, then forgets what the walk
/// sent, read and recorded: it fills the revision cache and is not the call
/// under test.
async fn hold_filling_walk<T: Send + Sync + 'static>(
    world: &SdkCallWorld,
    list: &Handle<Page<T>>,
    must_fill: bool,
) -> Vec<Page<T>> {
    let mut held = Vec::new();
    while let Some(page) = list.next().await.expect("every page of the filling walk") {
        held.push(page);
    }
    assert!(
        !must_fill || !world.shared.revisions.0.is_empty(),
        "the filling walk left the revision cache empty"
    );
    lock(&world.shared.seen).clear();
    lock(&world.shared.starts).clear();
    lock(&world.shared.ends).clear();
    world.shared.reads.store(0, Ordering::SeqCst);
    world.shared.refreshes.store(0, Ordering::SeqCst);
    held
}

/// One complete walk of the widget's parts fills the revision cache, its pages
/// still held; the walk that fills it is not the call under test.
#[given(expr = "the revision cache holds the parts of the widget {string}")]
async fn given_cached_parts(world: &mut SdkCallWorld, id: String) {
    let owner = lock(&world.shared.credential).identity.clone();
    world.cache_owners.push(owner);
    let client = api_bones_test_service_l2::WidgetsClient::new(&base_url(world), call_env(world))
        .expect("the lane's server address is a URI");
    let list = client.list_widget_parts(id, None);
    let pages = hold_filling_walk(world, &list, true).await;
    world.held_parts.extend(pages);
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
    let pages = hold_filling_walk(world, &list, true).await;
    world.held.extend(pages);
}

/// The same walk as the one that fills the revision cache, through one client
/// that stays the caller's session: the steps after it end that session.
#[given("the caller's session holds the numbers list")]
async fn given_session_holds_numbers(world: &mut SdkCallWorld) {
    let owner = lock(&world.shared.credential).identity.clone();
    world.cache_owners.push(owner);
    let client = api_bones_test_service_l2::NumbersClient::new(&base_url(world), call_env(world))
        .expect("the lane's server address is a URI")
        .with_default_page_size(world.default_page_size);
    let list = client.list(None);
    let pages = hold_filling_walk(world, &list, true).await;
    world.held.extend(pages);
    world.session = Some(Session(client));
}

#[given("the caller signs out")]
fn given_signed_out(world: &mut SdkCallWorld) {
    world.shared.source_broken.store(true, Ordering::SeqCst);
}

#[when("the caller signs in again")]
fn when_signed_in(world: &mut SdkCallWorld) {
    world.shared.source_broken.store(false, Ordering::SeqCst);
}

#[when("the caller reads the numbers list through the same session")]
async fn when_read_through_session(world: &mut SdkCallWorld) {
    let client = world
        .session
        .as_ref()
        .expect("a Given opened the caller's session")
        .0
        .clone();
    let list = client.list(None);
    world.refusal = list.next().await.err();
}

#[when(expr = "the caller gets the widget {string} through the SDK")]
async fn when_get(world: &mut SdkCallWorld, id: String) {
    let client = api_bones_test_service_l2::WidgetsClient::new(&base_url(world), call_env(world))
        .expect("the lane's server address is a URI");
    get_widget_through(world, &client, id).await;
}

#[when(expr = "the caller gets the widget {string} through the SDK in {int} concurrent calls")]
async fn when_get_concurrently(world: &mut SdkCallWorld, id: String, calls: usize) {
    let client = api_bones_test_service_l2::WidgetsClient::new(&base_url(world), call_env(world))
        .expect("the lane's server address is a URI");
    world.concurrent =
        futures_util::future::join_all((0..calls).map(|_| client.get_widget(id.clone()))).await;
}

/// Runs the widget call of `client` as the caller, in its trace when it has
/// one, and keeps the widget or the refusal.
async fn get_widget_through(
    world: &mut SdkCallWorld,
    client: &api_bones_test_service_l2::WidgetsClient,
    id: String,
) {
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

/// Opens the SDK from the caller's env at `base`, keeping it or the refusal.
fn open_sdk(world: &mut SdkCallWorld, base: Option<&str>) {
    match ApiBonesSdk::open(&call_env(world), base) {
        Ok(sdk) => {
            world.opened_widgets = Some(OpenedWidgets {
                client: sdk.widgets_client.clone(),
            });
            world.opened_numbers = Some(OpenedNumbers {
                client: sdk.numbers_client.clone(),
            });
        }
        Err(error) => world.refusal = Some(error),
    }
}

#[when("the caller opens the Api Bones SDK from the caller's session at the address of the server")]
fn when_open_at_server(world: &mut SdkCallWorld) {
    let url = base_url(world);
    open_sdk(world, Some(&url));
}

#[when("the caller opens the Api Bones SDK from the caller's session without a base address")]
fn when_open_without_base(world: &mut SdkCallWorld) {
    open_sdk(world, None);
}

/// The widgets client of the SDK the caller opened.
fn opened_widgets_client(world: &SdkCallWorld) -> api_bones_test_service_l2::WidgetsClient {
    world
        .opened_widgets
        .as_ref()
        .expect("a When opened the SDK")
        .client
        .clone()
}

/// The numbers client of the SDK the caller opened.
fn opened_numbers_client(world: &SdkCallWorld) -> api_bones_test_service_l2::NumbersClient {
    world
        .opened_numbers
        .as_ref()
        .expect("a When opened the SDK")
        .client
        .clone()
}

#[when(expr = "the caller gets the widget {string} through the opened SDK")]
async fn when_get_opened(world: &mut SdkCallWorld, id: String) {
    let client = opened_widgets_client(world);
    get_widget_through(world, &client, id).await;
}

#[when("the caller lists the numbers through the opened SDK")]
async fn when_list_opened(world: &mut SdkCallWorld) {
    let size = world.default_page_size;
    let client = opened_numbers_client(world).with_default_page_size(size);
    list_and_read(world, &client, None).await;
}

/// Lists the numbers at `size` and reads every page.
async fn list_and_read(
    world: &mut SdkCallWorld,
    client: &api_bones_test_service_l2::NumbersClient,
    size: Option<u32>,
) {
    let list = client.list(size);
    read_every_page(world, &list).await;
}

/// Reads `list` to its end, recording each page's numbers, or the refusal
/// that stopped it.
async fn read_every_page(world: &mut SdkCallWorld, list: &Handle<Page<Number>>) {
    loop {
        match list.next().await {
            Ok(Some(page)) => world
                .pages_read
                .push(page.items.iter().map(|number| number.value).collect()),
            Ok(None) => return,
            Err(error) => {
                world.refusal = Some(error);
                return;
            }
        }
    }
}

#[then(expr = "the refusal says {string}")]
fn then_refusal_says(world: &mut SdkCallWorld, text: String) {
    let error = world.refusal.as_ref().expect("the call was refused");
    assert!(error.message.contains(&text), "refused with {error:?}");
}

#[when(expr = "the caller gets the widgets {string} through the SDK")]
async fn when_batch_get(world: &mut SdkCallWorld, ids: String) {
    let ids: Vec<String> = ids.split(',').map(str::to_owned).collect();
    let client = api_bones_test_service_l2::WidgetsClient::new(&base_url(world), call_env(world))
        .expect("the lane's server address is a URI");
    let batch = client.batch_get_widgets(ids);
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

#[when(
    expr = "the caller lists the parts of the widget {string} with the page size {int} through the SDK"
)]
async fn when_list_parts_sized(world: &mut SdkCallWorld, id: String, size: u32) {
    list_parts_sized(world, id, Some(size)).await;
}

async fn list_parts(world: &mut SdkCallWorld, id: String) {
    list_parts_sized(world, id, None).await;
}

async fn list_parts_sized(world: &mut SdkCallWorld, id: String, size: Option<u32>) {
    let client = api_bones_test_service_l2::WidgetsClient::new(&base_url(world), call_env(world))
        .expect("the lane's server address is a URI");
    let list = client.list_widget_parts(id, size);
    loop {
        match list.next().await {
            Ok(Some(page)) => {
                world.pages_cached.push(page.from_cache);
                world
                    .part_pages
                    .push(page.items.iter().map(|part| part.name.clone()).collect());
            }
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
    open_numbers_ahead(world, url, None).await
}

/// Opens the list with the pages fetched ahead the caller chose; `None` leaves
/// the SDK's own choice.
async fn open_numbers_ahead(
    world: &SdkCallWorld,
    url: &str,
    prefetch: Option<u8>,
) -> Handle<Page<Number>> {
    let ctx = CallCtx {
        prefetch,
        ..CallCtx::current()
    };
    CallCtx::scope(ctx, async {
        api_bones_test_service_l2::NumbersClient::new(url, call_env(world))
            .expect("the lane's server address is a URI")
            .with_default_page_size(world.default_page_size)
            .list(None)
    })
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

#[given("the caller has opened a list of the numbers without fetching ahead through the SDK")]
async fn given_list_opened_without_prefetch(world: &mut SdkCallWorld) {
    let url = base_url(world);
    let list = open_numbers_ahead(world, &url, Some(0)).await;
    world.walk_span = list.span();
    world.list = Some(Arc::new(list));
}

/// The list's own choice to fetch ahead replaces what the call chose: the
/// call said none, the list then asks for one page ahead.
#[given(
    "the caller has opened a list of the numbers without fetching ahead and then asked it to fetch ahead through the SDK"
)]
async fn given_list_opened_then_asked_ahead(world: &mut SdkCallWorld) {
    let url = base_url(world);
    let opened = open_numbers_ahead(world, &url, Some(0)).await;
    let list = opened.with_prefetch(1);
    world.walk_span = list.span();
    world.list = Some(Arc::new(list));
}

#[when(expr = "the caller waits {int} ms")]
async fn when_caller_waits(_world: &mut SdkCallWorld, millis: u64) {
    tokio::time::sleep(Duration::from_millis(millis)).await;
}

/// Starts a read of the next page of the list that the caller does not wait
/// for, and returns once the numbers server has the request: the list is then
/// busy with that read.
#[given("a read of the next page of the list is pending")]
async fn given_read_pending(world: &mut SdkCallWorld) {
    let list = world.list.clone().expect("a Given opened the list");
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
async fn when_list_cancelled(world: &mut SdkCallWorld) {
    cancel_list(world);
    settle_pending_read(world).await;
}

/// Waits for the read the caller left pending, if one was, and keeps what it
/// returned.
async fn settle_pending_read(world: &mut SdkCallWorld) {
    world.pending_read = match world.pending.as_mut() {
        Some(pending) => Some(pending.await.expect("the pending read ran to its end")),
        None => world.pending_read.take(),
    };
    world.pending = None;
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
    settle_pending_read(world).await;
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

#[then(expr = "every concurrent call answered the widget {string} named {string}")]
fn then_every_concurrent_widget(world: &mut SdkCallWorld, id: String, name: String) {
    assert!(!world.concurrent.is_empty(), "a When made the calls");
    for outcome in &world.concurrent {
        let widget = outcome.as_ref().expect("the call succeeded");
        assert_eq!(widget.id, id);
        assert_eq!(widget.name, name);
    }
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

#[then(expr = "the refusal names a wait of {int} ms")]
fn then_refusal_names_wait(world: &mut SdkCallWorld, millis: u64) {
    let error = world.refusal.as_ref().expect("the call was refused");
    assert_eq!(
        error.retry_after(),
        Some(Duration::from_millis(millis)),
        "refused with {error:?}"
    );
}

#[then("the call is refused as unauthenticated")]
fn then_refused_unauthenticated(world: &mut SdkCallWorld) {
    let error = world.refusal.as_ref().expect("the call was refused");
    assert_eq!(error.platform, "UNAUTHORIZED", "refused with {error:?}");
}

/// What the read the caller left pending returned, once a When waited for it.
fn finished_read(world: &SdkCallWorld) -> &Result<Option<Page<Number>>, Error> {
    world
        .pending_read
        .as_ref()
        .expect("a When waited for the pending read")
}

#[then(expr = "the pending read is refused with the code {string}")]
fn then_pending_refused(world: &mut SdkCallWorld, code: String) {
    let error = world
        .pending_read
        .as_ref()
        .expect("a When waited for the pending read")
        .as_ref()
        .expect_err("the pending read was not refused");
    assert_eq!(error.code, code);
}

#[then(expr = "the pending read completes with the page {string}")]
fn then_pending_completes(world: &mut SdkCallWorld, expected: String) {
    let page = finished_read(world)
        .as_ref()
        .expect("the pending read was refused")
        .as_ref()
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
    assert_eq!(status_name(walk_ends[0].status), status);
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

/// The caller's next list of the numbers starts from the server: its first
/// page is not replayed from the cache, and the cache holds no chain. The
/// probe's spans and requests are forgotten before the assertions run.
#[then("the caller's revision cache is empty")]
async fn then_cache_empty(world: &mut SdkCallWorld) {
    let (seen, starts, ends) = (
        lock(&world.shared.seen).len(),
        lock(&world.shared.starts).len(),
        lock(&world.shared.ends).len(),
    );
    let url = base_url(world);
    let list = open_numbers_ahead(world, &url, Some(0)).await;
    world.probe = Some(list.next().await);
    drop(list);
    lock(&world.shared.seen).truncate(seen);
    lock(&world.shared.starts).truncate(starts);
    lock(&world.shared.ends).truncate(ends);
    let first = world
        .probe
        .as_ref()
        .expect("the caller listed the numbers")
        .as_ref()
        .expect("the caller's list was refused")
        .as_ref()
        .expect("the list had a page");
    assert!(!first.from_cache, "the cache kept a chain");
    assert!(
        world.shared.revisions.0.is_empty(),
        "the cache kept a chain"
    );
}

#[then(expr = "the credential source was asked to refresh {int} time\\(s\\)")]
fn then_refreshed(world: &mut SdkCallWorld, times: usize) {
    assert_eq!(world.shared.refreshes.load(Ordering::SeqCst), times);
}

#[then("the revision cache still holds the numbers list")]
async fn then_cache_holds_numbers(world: &mut SdkCallWorld) {
    let list = open_numbers(world).await;
    let first = list
        .next()
        .await
        .expect("the first page of the caller's list")
        .expect("the list had a page");
    assert!(
        first.from_cache,
        "the revision cache lost the caller's chain"
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
        assert_eq!(status_name(end.status), status, "span: {end:?}");
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
    let asked: Vec<u32> = world.seen().iter().map(|seen| seen.page_size).collect();
    assert!(!asked.is_empty(), "the server saw no request");
    assert_eq!(asked, vec![size; asked.len()]);
    // A caller that read only the first page also holds it: it has the size
    // the server was asked for.
    if let Some(page) = world.first_page.as_ref() {
        assert_eq!(page.items.len(), size as usize);
    }
}

#[then(expr = "every request the server saw asked for the part page size {int}")]
fn then_part_page_size(world: &mut SdkCallWorld, size: u32) {
    let asked: Vec<u32> = world.seen().iter().map(|seen| seen.page_size).collect();
    assert!(!asked.is_empty(), "the server saw no request");
    assert_eq!(asked, vec![size; asked.len()]);
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

#[given(expr = "the widgets server refuses the cursor of part page {int} as expired")]
fn given_part_cursor_expired(world: &mut SdkCallWorld, page: usize) {
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

#[when(expr = "the caller lists the numbers with the page size {int} through the SDK")]
async fn when_list_numbers_sized(world: &mut SdkCallWorld, size: u32) {
    let url = base_url(world);
    let client = api_bones_test_service_l2::NumbersClient::new(&url, call_env(world))
        .expect("the lane's server address is a URI");
    list_and_read(world, &client, Some(size)).await;
}

/// The read of the first page starts and waits for the caller's credential,
/// which has not been answered: nothing has reached the server.
#[given("a read of the first page of the list is waiting for the caller's credential")]
async fn given_read_awaits_credential(world: &mut SdkCallWorld) {
    let list = world.list.clone().expect("a Given opened the list");
    world.pending = Some(tokio::spawn(async move { list.next().await }));
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        world.seen().is_empty(),
        "a request reached the server before the credential was answered"
    );
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
    // The Rust surface has no `first_page`: reading one page alone is a list
    // opened first-only, which fetches nothing ahead and ends after its page.
    read_first_page_alone(world, None).await;
}

async fn read_first_page_alone(world: &mut SdkCallWorld, prefetch: Option<u8>) {
    let ctx = CallCtx {
        prefetch,
        first_only: true,
        ..CallCtx::current()
    };
    let url = base_url(world);
    let list = CallCtx::scope(ctx, async {
        api_bones_test_service_l2::NumbersClient::new(&url, call_env(world))
            .expect("the lane's server address is a URI")
            .with_default_page_size(world.default_page_size)
            .list(None)
    })
    .await;
    world.first_page = Some(
        list.next()
            .await
            .expect("the first page of the list")
            .expect("the list had a first page"),
    );
    world.ended_after_first = Some(matches!(list.next().await, Ok(None)));
    list.cancel();
}

/// Reading one page alone wins over a choice to fetch ahead.
#[when(
    "the caller reads the first page of the numbers list through the SDK having asked to fetch ahead"
)]
async fn when_first_page_asked_ahead(world: &mut SdkCallWorld) {
    read_first_page_alone(world, Some(1)).await;
}

#[then("the list ends after its first page")]
fn then_list_ends_after_first(world: &mut SdkCallWorld) {
    assert_eq!(world.ended_after_first, Some(true));
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
    let next = world
        .list
        .as_ref()
        .expect("the caller walked the list")
        .next()
        .await;
    assert!(
        matches!(next, Ok(None)),
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
        .page()
        .unwrap_or_else(|| panic!("the refusal names no page failure: {error:?}"));
    assert_eq!(page_kind_name(detail.kind), kind);
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
    let span = world.list.as_ref().expect("a Given opened the list").span();
    assert_eq!(span, None);
}

#[when(expr = "the caller clamps {int} between {int} and {int} through the SDK")]
fn when_clamp(world: &mut SdkCallWorld, value: u32, low: u32, high: u32) {
    world.static_answer = Some(api_bones_test_service_l2::NumbersClient::clamp(
        value, low, high,
    ));
}

#[when("the caller reads the largest page size through the SDK")]
fn when_read_largest_page_size(world: &mut SdkCallWorld) {
    // The constant is read through the SDK's clamp, with the widest bounds, so
    // the answer is the constant itself.
    let largest = api_bones_test_service_l2::NumbersClient::clamp(
        api_bones_test_service_l2::NumbersClient::MAX_PAGE_SIZE,
        0,
        u32::MAX,
    );
    world.static_answer = Some(largest);
}

#[then(expr = "the static answer is {int}")]
fn then_static_answer(world: &mut SdkCallWorld, expected: u32) {
    assert_eq!(world.static_answer, Some(expected));
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
