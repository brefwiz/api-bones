// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! The native host boundary of the SDK, driven the way a Kotlin or Swift host
//! drives it: through the uniffi exports of the test service's Layer 2, with a
//! Rust `FfiHost` standing in for the foreign one, against a real connectrpc
//! server on loopback that records what each request carried.
//!
//! Every value that crosses is protobuf bytes, so the assertions read the
//! bytes by field number: nothing here reaches past what a foreign host sees.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use api_bones::connect::ConnectCode;
use api_bones_contract_rust::{
    BatchGetWidgetsRequest, BatchGetWidgetsResponse, EventsServiceExt, GetWidgetRequest,
    GetWidgetResponse, ListIdempotentRequest, ListIdempotentResponse, ListNoSideEffectsRequest,
    ListNoSideEffectsResponse, ListRequest, ListResponse, NumbersService, NumbersServiceExt,
    PageResponse, Widget, WidgetsService, WidgetsServiceExt,
};
use api_bones_sdk::__private::decode_call_error;
use api_bones_sdk_ffi::{FfiCallCtx, FfiError, FfiHandle, FfiHost, format_error};
use api_bones_test_service_l2 as l2;
use connectrpc::{
    ConnectError, Encodable, RequestContext, Response, Router, ServiceRequest, ServiceResult,
};
use tokio::io::AsyncReadExt;

#[path = "../../../rust/tests/steps/support.rs"]
mod support;
#[path = "steps/ticks.rs"]
mod ticks;

use support::{batch_response, serve_loopback, unserved_widget_methods};
use ticks::{EventsServer, Watching};

/// The bearer every principal in these tests carries; no output may name it.
const SENTINEL: &str = "sentinel-secret-token";

/// The value of the second credential header a principal relays; no output may
/// name it either.
const RELAYED: &str = "sentinel-relayed-header";

/// Whether `bytes` name a secret one of the credentials carries.
fn leaks(bytes: &[u8]) -> bool {
    let text = String::from_utf8_lossy(bytes);
    text.contains(SENTINEL) || text.contains(RELAYED)
}

/// The revision every numbers page carries.
const REVISION: &str = "r1";

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().expect("the recorder is never poisoned")
}

fn header(ctx: &RequestContext, name: &str) -> String {
    ctx.header(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned()
}

// ---- protobuf bytes, by field number ----------------------------------------

fn varint(mut value: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let byte = u8::try_from(value & 0x7f).expect("seven bits");
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return out;
        }
        out.push(byte | 0x80);
    }
}

/// A length-delimited field: `tag` is the field's tag byte.
fn len_field(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    out.extend(varint(body.len() as u64));
    out.extend_from_slice(body);
    out
}

/// One field of a decoded message.
#[derive(Debug, Clone, PartialEq)]
enum Field {
    Varint(u64),
    Bytes(Vec<u8>),
}

fn read_varint(bytes: &[u8], at: &mut usize) -> u64 {
    let mut value = 0_u64;
    let mut shift = 0;
    loop {
        let byte = bytes[*at];
        *at += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return value;
        }
        shift += 7;
    }
}

/// The fields of a message holding only varint and length-delimited fields.
fn fields(bytes: &[u8]) -> Vec<(u64, Field)> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let tag = read_varint(bytes, &mut at);
        match tag & 7 {
            0 => out.push((tag >> 3, Field::Varint(read_varint(bytes, &mut at)))),
            2 => {
                let len = usize::try_from(read_varint(bytes, &mut at)).expect("a length");
                out.push((tag >> 3, Field::Bytes(bytes[at..at + len].to_vec())));
                at += len;
            }
            wire => panic!("unexpected wire type {wire}"),
        }
    }
    out
}

fn bytes_of(message: &[u8], number: u64) -> Vec<Vec<u8>> {
    fields(message)
        .into_iter()
        .filter_map(|(n, field)| match field {
            Field::Bytes(bytes) if n == number => Some(bytes),
            _ => None,
        })
        .collect()
}

fn varint_of(message: &[u8], number: u64) -> Option<u64> {
    fields(message)
        .into_iter()
        .find_map(|(n, field)| match field {
            Field::Varint(value) if n == number => Some(value),
            _ => None,
        })
}

fn text_of(message: &[u8], number: u64) -> String {
    bytes_of(message, number)
        .first()
        .map(|bytes| String::from_utf8(bytes.clone()).expect("text"))
        .unwrap_or_default()
}

/// A `CredentialAnswer` naming `subject`, the `authorization` header `bearer`
/// and the relayed header `x-relayed`.
fn credential(subject: &str, bearer: &str) -> Vec<u8> {
    let header = |name: &str, value: &str| {
        len_field(
            0x1a,
            &[
                len_field(0x0a, name.as_bytes()),
                len_field(0x12, value.as_bytes()),
            ]
            .concat(),
        )
    };
    let credential = [
        len_field(0x0a, subject.as_bytes()),
        header("authorization", bearer),
        header("x-relayed", RELAYED),
    ]
    .concat();
    len_field(0x0a, &credential)
}

/// A `SpanStarted` that records, under a fixed trace context.
fn span_started() -> Vec<u8> {
    let traceparent = "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01";
    [len_field(0x0a, traceparent.as_bytes()), vec![0x10, 0x01]].concat()
}

// ---- the server --------------------------------------------------------------

/// What the loopback server saw and how it is told to answer.
#[derive(Default)]
struct Shared {
    /// The `authorization` header of every request, in arrival order.
    authorizations: Mutex<Vec<String>>,
    /// Requests still refused as unauthenticated.
    unauthenticated: AtomicUsize,
}

impl Shared {
    fn admit(&self, ctx: &RequestContext) -> Result<(), ConnectError> {
        lock(&self.authorizations).push(header(ctx, "authorization"));
        let left = self.unauthenticated.load(Ordering::SeqCst);
        if left > 0 {
            self.unauthenticated.store(left - 1, Ordering::SeqCst);
            return Err(ConnectError::unauthenticated("the caller is not signed in"));
        }
        Ok(())
    }

    fn requests(&self) -> usize {
        lock(&self.authorizations).len()
    }
}

#[derive(Clone)]
struct WidgetsServer(Arc<Shared>);

impl WidgetsService for WidgetsServer {
    async fn get_widget<'a>(
        &'a self,
        ctx: RequestContext,
        request: ServiceRequest<'_, GetWidgetRequest>,
    ) -> ServiceResult<impl Encodable<GetWidgetResponse> + Send + use<'a>> {
        let asked = request.to_owned_message();
        self.0.admit(&ctx)?;
        if asked.id != "w1" {
            return Err(ConnectError::not_found("no such widget"));
        }
        Response::ok(GetWidgetResponse {
            widget: Widget {
                id: asked.id,
                name: "Sprocket".to_owned(),
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
    ) -> ServiceResult<impl Encodable<BatchGetWidgetsResponse> + Send + use<'a>> {
        let asked = request.to_owned_message();
        self.0.admit(&ctx)?;
        Response::ok(batch_response(&asked.ids, |id| {
            (id == "w1").then(|| Widget {
                id: id.to_owned(),
                name: "Sprocket".to_owned(),
                ..Default::default()
            })
        }))
    }

    unserved_widget_methods!("the lane serves only GetWidget and BatchGetWidgets");
}

#[derive(Clone)]
struct NumbersServer(Arc<Shared>);

impl NumbersService for NumbersServer {
    async fn list_no_side_effects<'a>(
        &'a self,
        ctx: RequestContext,
        request: ServiceRequest<'_, ListNoSideEffectsRequest>,
    ) -> ServiceResult<impl Encodable<ListNoSideEffectsResponse> + Send + use<'a>> {
        let page = request
            .to_owned_message()
            .page
            .into_option()
            .unwrap_or_default();
        self.0.admit(&ctx)?;
        let first = page.cursor.is_empty();
        if first
            && header(&ctx, "bones-list-cache") == "1"
            && header(&ctx, "if-none-match") == format!("\"{REVISION}\"")
        {
            return Ok(Response::new(ListNoSideEffectsResponse::default())
                .with_header("bones-not-modified", "1"));
        }
        // The principal decides the list, so a page of another principal's
        // cache can never pass for this one's. A subject ending `-b` is the
        // second principal.
        let own = header(&ctx, "authorization").contains("-b-");
        let (items, next_cursor) = match (own, first) {
            (true, _) => (vec![70], String::new()),
            (false, true) => (vec![1, 2], "c1".to_owned()),
            (false, false) => (vec![3], String::new()),
        };
        Response::ok(ListNoSideEffectsResponse {
            items,
            page: PageResponse {
                next_cursor,
                revision: REVISION.into(),
                ..Default::default()
            }
            .into(),
            ..Default::default()
        })
    }

    async fn list_idempotent<'a>(
        &'a self,
        _ctx: RequestContext,
        _request: ServiceRequest<'_, ListIdempotentRequest>,
    ) -> ServiceResult<impl Encodable<ListIdempotentResponse> + Send + use<'a>> {
        ServiceResult::<ListIdempotentResponse>::Err(ConnectError::unimplemented("not served"))
    }

    async fn list<'a>(
        &'a self,
        _ctx: RequestContext,
        _request: ServiceRequest<'_, ListRequest>,
    ) -> ServiceResult<impl Encodable<ListResponse> + Send + use<'a>> {
        ServiceResult::<ListResponse>::Err(ConnectError::unimplemented("not served"))
    }
}

impl Watching for Shared {
    fn admit(&self, ctx: &RequestContext) -> Result<(), ConnectError> {
        Shared::admit(self, ctx)
    }

    fn pacing(&self) -> (Duration, bool) {
        (Duration::from_millis(40), false)
    }
}

/// The server on an ephemeral loopback port and what it saw.
async fn serve() -> (Arc<Shared>, String) {
    let shared = Arc::new(Shared::default());
    let router =
        WidgetsServiceExt::register(Arc::new(WidgetsServer(shared.clone())), Router::new());
    let router = NumbersServiceExt::register(Arc::new(NumbersServer(shared.clone())), router);
    let router = EventsServiceExt::register(Arc::new(EventsServer(shared.clone())), router);
    let (_task, address) = serve_loopback(router).await;
    (shared, format!("http://{address}"))
}

// ---- the host ------------------------------------------------------------------

/// What the host does when the core asks for its credential.
#[derive(Clone, Copy, PartialEq)]
enum Credential {
    /// Answers its credential.
    Answers,
    /// Returns an error carrying the sentinel.
    Fails,
    /// Panics with the sentinel.
    Panics,
    /// Answers bytes that are not a credential answer.
    Garbled,
}

/// The Rust stand-in of a native host.
struct TestHost {
    subject: String,
    bearer: String,
    credential: Credential,
    /// Whether a refresh read fails while a first read answers.
    refresh_fails: bool,
    reads: AtomicUsize,
    unauthenticated: AtomicUsize,
    /// `start:<name>` and `end` in the order the core reported them.
    spans: Mutex<Vec<String>>,
    /// The bytes of every span report, as the core handed them over.
    span_bytes: Mutex<Vec<Vec<u8>>>,
}

impl TestHost {
    fn new(subject: &str) -> Arc<Self> {
        Arc::new(Self {
            subject: subject.to_owned(),
            // One token: the core reads a bearer holding whitespace as a changed
            // identity and sends nothing.
            bearer: format!("Bearer {subject}-{SENTINEL}"),
            credential: Credential::Answers,
            refresh_fails: false,
            reads: AtomicUsize::new(0),
            unauthenticated: AtomicUsize::new(0),
            spans: Mutex::new(Vec::new()),
            span_bytes: Mutex::new(Vec::new()),
        })
    }

    fn scripted(credential: Credential, refresh_fails: bool) -> Arc<Self> {
        Arc::new(Self {
            credential,
            refresh_fails,
            ..Arc::into_inner(Self::new("principal-a")).expect("one owner")
        })
    }
}

impl FfiHost for TestHost {
    fn credential(&self, refresh: bool) -> Result<Vec<u8>, FfiError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        if refresh && self.refresh_fails {
            return Err(FfiError::Call {
                bytes: SENTINEL.as_bytes().to_vec(),
            });
        }
        match self.credential {
            Credential::Answers => Ok(credential(&self.subject, &self.bearer)),
            Credential::Fails => Err(FfiError::Call {
                bytes: SENTINEL.as_bytes().to_vec(),
            }),
            Credential::Panics => panic!("{SENTINEL}"),
            Credential::Garbled => Ok(vec![0x0a]),
        }
    }

    fn anonymous(&self) -> bool {
        false
    }

    fn unauthenticated(&self) {
        self.unauthenticated.fetch_add(1, Ordering::SeqCst);
    }

    fn span_start(&self, start: Vec<u8>) -> Vec<u8> {
        lock(&self.spans).push(format!("start:{}", text_of(&start, 1)));
        lock(&self.span_bytes).push(start);
        span_started()
    }

    fn span_end(&self, end: Vec<u8>) {
        lock(&self.spans).push("end".to_owned());
        lock(&self.span_bytes).push(end);
    }
}

// ---- helpers over the exports --------------------------------------------------

async fn widgets(url: &str, host: &Arc<TestHost>) -> Arc<l2::WidgetsClientFfi> {
    l2::WidgetsClientFfi::new(url.to_owned(), host.clone(), Vec::new()).expect("a client")
}

async fn numbers(url: &str, host: &Arc<TestHost>) -> Arc<l2::NumbersClientFfi> {
    l2::NumbersClientFfi::new(url.to_owned(), host.clone(), Vec::new()).expect("a client")
}

async fn get_widget(client: &Arc<l2::WidgetsClientFfi>, id: &str) -> Result<Vec<u8>, FfiError> {
    get_widget_in(client, id, FfiCallCtx::default()).await
}

async fn get_widget_in(
    client: &Arc<l2::WidgetsClientFfi>,
    id: &str,
    ctx: FfiCallCtx,
) -> Result<Vec<u8>, FfiError> {
    l2::api_bones_widgetsclient_get_widget(client.clone(), len_field(0x0a, id.as_bytes()), ctx)
        .await
}

fn list(client: &Arc<l2::NumbersClientFfi>) -> Arc<FfiHandle> {
    list_in(client, FfiCallCtx::default())
}

fn list_in(client: &Arc<l2::NumbersClientFfi>, ctx: FfiCallCtx) -> Arc<FfiHandle> {
    l2::api_bones_numbersclient_list(client.clone(), Vec::new(), ctx).expect("a handle")
}

/// Every page of a list: the items of each and whether the page came from the
/// cache.
async fn pages(handle: &FfiHandle) -> Vec<(Vec<Vec<u8>>, bool)> {
    let mut out = Vec::new();
    while let Some(page) = handle.next().await.expect("a page") {
        out.push((
            bytes_of(&page, 1),
            varint_of(&page, 3).unwrap_or_default() == 1,
        ));
    }
    out
}

fn call_error(error: &FfiError) -> api_bones_sdk::Error {
    let FfiError::Call { bytes } = error;
    decode_call_error(bytes).expect("a CallError")
}

/// Nothing a host or the app above it can read names a sentinel.
fn assert_clean(error: &FfiError) {
    let FfiError::Call { bytes } = error;
    assert!(!leaks(bytes));
    assert!(!leaks(&format_error(bytes.clone(), "en", None)));
    assert!(!leaks(format!("{error:?}").as_bytes()));
    assert!(!leaks(error.to_string().as_bytes()));
}

/// No span report the host received, raw bytes included, names a sentinel.
fn assert_spans_clean(host: &TestHost) {
    let reports = lock(&host.span_bytes);
    assert!(!reports.is_empty(), "the core reported at least one span");
    for report in reports.iter() {
        assert!(!leaks(report), "a span report names a credential value");
    }
}

// ---- the base set -------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn a_unary_call_returns_the_widget_and_carries_the_hosts_credential() {
    let (shared, url) = serve().await;
    let host = TestHost::new("principal-a");
    let client = widgets(&url, &host).await;

    let widget = get_widget(&client, "w1").await.expect("the widget");

    assert_eq!(text_of(&widget, 1), "w1");
    assert_eq!(text_of(&widget, 2), "Sprocket");
    assert_eq!(
        lock(&shared.authorizations).as_slice(),
        [format!("Bearer principal-a-{SENTINEL}")]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_error_round_trips_as_call_error_bytes_and_formats_for_a_viewer() {
    let (_shared, url) = serve().await;
    let client = widgets(&url, &TestHost::new("principal-a")).await;

    let error = get_widget(&client, "missing").await.expect_err("not found");

    assert_eq!(call_error(&error).connect_code, ConnectCode::NotFound);
    let FfiError::Call { bytes } = &error;
    let formatted = format_error(bytes.clone(), "en", Some("Europe/Paris".to_owned()));
    assert!(!bytes_of(&formatted, 1).is_empty(), "a formatted message");
    let refused = format_error(bytes.clone(), "en", Some("Not/AZone".to_owned()));
    assert_eq!(text_of(&refused, 2), "unknown_time_zone");
    let malformed = format_error(vec![0xff], "en", None);
    assert_eq!(text_of(&malformed, 2), "malformed_error");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_list_arrives_one_page_per_next_and_ends_with_none() {
    let (_shared, url) = serve().await;
    let client = numbers(&url, &TestHost::new("list-a")).await;

    let handle = list(&client);
    let read = pages(&handle).await;

    assert_eq!(read.len(), 2);
    assert_eq!(read[0].0.len(), 2, "two numbers on the first page");
    assert_eq!(read[1].0.len(), 1, "one number on the second page");
    assert!(read.iter().all(|(_, cached)| !cached));
    assert!(handle.next().await.expect("still ended").is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_batch_answers_one_outcome_per_id_in_request_order() {
    let (_shared, url) = serve().await;
    let client = widgets(&url, &TestHost::new("principal-a")).await;
    let ids = [len_field(0x0a, b"w1"), len_field(0x0a, b"nope")].concat();

    let handle = l2::api_bones_widgetsclient_batch_get_widgets(client, ids, FfiCallCtx::default())
        .expect("a handle");
    let chunk = handle.next().await.expect("a chunk").expect("one chunk");

    let results = bytes_of(&chunk, 1);
    assert_eq!(results.len(), 2);
    assert_eq!(text_of(&bytes_of(&results[0], 1)[0], 2), "Sprocket");
    let failed = &bytes_of(&results[1], 2)[0];
    assert_eq!(
        decode_call_error(failed).expect("a CallError").connect_code,
        ConnectCode::NotFound
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stream_cancelled_mid_flight_ends_with_none() {
    let (_shared, url) = serve().await;
    let client =
        l2::EventsClientFfi::new(url, TestHost::new("principal-a"), Vec::new()).expect("a client");
    let ticks = [0x08, 5].to_vec();

    let handle = l2::api_bones_eventsclient_watch(client, ticks, FfiCallCtx::default())
        .await
        .expect("a handle");
    let first = handle
        .next()
        .await
        .expect("a tick")
        .expect("the first tick");
    assert_eq!(varint_of(&first, 1).unwrap_or_default(), 0);
    handle.cancel();

    assert!(handle.next().await.expect("a clean end").is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_host_is_told_of_an_unauthenticated_refusal() {
    let (shared, url) = serve().await;
    shared.unauthenticated.store(8, Ordering::SeqCst);
    let host = TestHost::new("principal-a");
    let client = widgets(&url, &host).await;

    let error = get_widget(&client, "w1").await.expect_err("refused");

    assert_eq!(
        call_error(&error).connect_code,
        ConnectCode::Unauthenticated
    );
    assert!(host.unauthenticated.load(Ordering::SeqCst) >= 1);
    assert_clean(&error);
    assert_spans_clean(&host);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_spans_reach_the_host_with_every_start_before_its_end() {
    let (_shared, url) = serve().await;
    let host = TestHost::new("principal-a");
    let client = widgets(&url, &host).await;

    get_widget(&client, "w1").await.expect("the widget");

    let spans = lock(&host.spans).clone();
    assert!(spans.first().is_some_and(|s| s.starts_with("start:")));
    assert_eq!(spans.last().map(String::as_str), Some("end"));
    let starts = spans.iter().filter(|s| s.starts_with("start:")).count();
    assert_eq!(starts, spans.len() - starts, "every span ends once");
    assert_spans_clean(&host);
}

// ---- the call context ----------------------------------------------------------

const FIRST_TRACE: &str = "00-11111111111111111111111111111111-aaaaaaaaaaaaaaaa-01";
const SECOND_TRACE: &str = "00-22222222222222222222222222222222-bbbbbbbbbbbbbbbb-01";

/// A context that continues `traceparent`.
fn tracing(traceparent: &str) -> FfiCallCtx {
    FfiCallCtx {
        traceparent: Some(traceparent.to_owned()),
        tracestate: Some("vendor=state".to_owned()),
        ..FfiCallCtx::default()
    }
}

/// The parents the span starts among `reports` name, and their tracestates.
fn parents(reports: &[Vec<u8>]) -> Vec<(String, String)> {
    reports
        .iter()
        .flat_map(|report| {
            bytes_of(report, 4).into_iter().map(|parent| {
                (
                    String::from_utf8_lossy(&parent).into_owned(),
                    String::from_utf8_lossy(
                        bytes_of(report, 5)
                            .first()
                            .map_or(&[][..], |state| state.as_slice()),
                    )
                    .into_owned(),
                )
            })
        })
        .collect()
}

/// Waits until the server has seen `count` requests.
async fn requests_reach(shared: &Shared, count: usize) {
    for _ in 0..500 {
        if shared.requests() >= count {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the server never saw {count} requests");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_call_continues_the_trace_of_its_own_context_and_no_other() {
    let (_shared, url) = serve().await;
    let host = TestHost::new("trace-a");
    let client = widgets(&url, &host).await;

    get_widget_in(&client, "w1", tracing(FIRST_TRACE))
        .await
        .expect("the widget");
    let first_calls = lock(&host.span_bytes).len();
    get_widget_in(&client, "w1", tracing(SECOND_TRACE))
        .await
        .expect("the widget");

    let reports = lock(&host.span_bytes).clone();
    let (first, second) = reports.split_at(first_calls);
    let state = "vendor=state".to_owned();
    assert!(parents(first).contains(&(FIRST_TRACE.to_owned(), state.clone())));
    assert!(parents(second).contains(&(SECOND_TRACE.to_owned(), state)));
    assert!(
        parents(first)
            .iter()
            .all(|(parent, _)| parent != SECOND_TRACE)
    );
    assert!(
        parents(second)
            .iter()
            .all(|(parent, _)| parent != FIRST_TRACE)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_call_with_the_default_context_starts_a_new_trace() {
    let (_shared, url) = serve().await;
    let host = TestHost::new("trace-b");
    let client = widgets(&url, &host).await;

    get_widget(&client, "w1").await.expect("the widget");

    let reports = lock(&host.span_bytes).clone();
    let root = reports.first().expect("the root span started");
    assert!(bytes_of(root, 4).is_empty(), "the root span has no parent");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_list_and_a_stream_continue_the_trace_of_their_own_context() {
    let (_shared, url) = serve().await;
    let host = TestHost::new("trace-c");
    let client = numbers(&url, &host).await;
    pages(&list_in(&client, tracing(FIRST_TRACE))).await;

    let events = l2::EventsClientFfi::new(url, host.clone(), Vec::new()).expect("a client");
    let stream =
        l2::api_bones_eventsclient_watch(events, [0x08, 1].to_vec(), tracing(SECOND_TRACE))
            .await
            .expect("a handle");
    stream.next().await.expect("a tick");
    stream.cancel();

    let named: Vec<String> = parents(&lock(&host.span_bytes))
        .into_iter()
        .map(|(parent, _)| parent)
        .collect();
    assert!(
        named.iter().any(|parent| parent == FIRST_TRACE),
        "{named:?}"
    );
    assert!(
        named.iter().any(|parent| parent == SECOND_TRACE),
        "{named:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_list_asked_for_its_first_page_only_reads_one_page_with_one_request() {
    let (shared, url) = serve().await;
    let client = numbers(&url, &TestHost::new("first-only-a")).await;
    let ctx = FfiCallCtx {
        first_only: true,
        ..FfiCallCtx::default()
    };

    let handle = list_in(&client, ctx);
    let read = pages(&handle).await;

    assert_eq!(read.len(), 1, "the first page alone");
    assert_eq!(read[0].0.len(), 2, "the two numbers of the first page");
    assert_eq!(shared.requests(), 1, "next_cursor is never followed");
    assert!(handle.next().await.expect("still ended").is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_list_fetches_ahead_unless_its_context_says_none() {
    let (shared, url) = serve().await;
    let ahead = numbers(&url, &TestHost::new("prefetch-a")).await;
    let handle = list_in(&ahead, FfiCallCtx::default());
    handle
        .next()
        .await
        .expect("a page")
        .expect("the first page");
    requests_reach(&shared, 2).await;
    assert_eq!(pages(&handle).await.len(), 1, "the page fetched ahead");

    let (shared, url) = serve().await;
    let alone = numbers(&url, &TestHost::new("prefetch-none")).await;
    let ctx = FfiCallCtx {
        prefetch: Some(0),
        ..FfiCallCtx::default()
    };
    let handle = list_in(&alone, ctx);
    handle
        .next()
        .await
        .expect("a page")
        .expect("the first page");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(shared.requests(), 1, "nothing is fetched ahead");
    let second = handle
        .next()
        .await
        .expect("a page")
        .expect("the second page");
    assert_eq!(bytes_of(&second, 1).len(), 1);
    assert_eq!(
        shared.requests(),
        2,
        "the second page, when it is asked for"
    );
}

// ---- the transport (SEC-1) ------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn cleartext_to_a_public_host_is_refused_before_any_credential_is_read() {
    let host = TestHost::new("principal-a");

    let error =
        l2::WidgetsClientFfi::new("http://example.com".to_owned(), host.clone(), Vec::new())
            .err()
            .expect("refused");

    assert_eq!(
        call_error(&error).connect_code,
        ConnectCode::InvalidArgument
    );
    assert_eq!(host.reads.load(Ordering::SeqCst), 0);
}

/// A TLS server whose certificate is self-signed, so it chains to no root the
/// client trusts, and what it saw.
struct UntrustedTls {
    address: std::net::SocketAddr,
    /// Handshakes the client refused.
    rejected: Arc<AtomicUsize>,
    /// Handshakes that completed.
    completed: Arc<AtomicUsize>,
    /// Bytes read from a completed handshake's connection: the requests a
    /// handler would have been handed.
    requests: Arc<Mutex<Vec<u8>>>,
}

async fn untrusted_tls_server() -> UntrustedTls {
    use connectrpc::rustls::crypto::aws_lc_rs::default_provider;
    use connectrpc::rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

    let certified =
        rcgen::generate_simple_self_signed(vec!["localhost".to_owned(), "127.0.0.1".to_owned()])
            .expect("a self-signed certificate");
    let cert: CertificateDer<'static> = certified.cert.der().clone();
    let key: PrivateKeyDer<'static> =
        PrivatePkcs8KeyDer::from(certified.key_pair.serialize_der()).into();
    let mut config =
        connectrpc::rustls::ServerConfig::builder_with_provider(Arc::new(default_provider()))
            .with_safe_default_protocol_versions()
            .expect("protocol versions")
            .with_no_client_auth()
            .with_single_cert(vec![cert], key)
            .expect("a server config");
    config.alpn_protocols = vec![b"h2".to_vec()];
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("binds");
    let address = listener.local_addr().expect("an address");
    let server = UntrustedTls {
        address,
        rejected: Arc::default(),
        completed: Arc::default(),
        requests: Arc::default(),
    };
    let (rejected, completed, requests) = (
        server.rejected.clone(),
        server.completed.clone(),
        server.requests.clone(),
    );
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let acceptor = acceptor.clone();
            let (rejected, completed, requests) =
                (rejected.clone(), completed.clone(), requests.clone());
            tokio::spawn(async move {
                match acceptor.accept(tcp).await {
                    Err(_) => {
                        rejected.fetch_add(1, Ordering::SeqCst);
                    }
                    Ok(mut tls) => {
                        completed.fetch_add(1, Ordering::SeqCst);
                        let mut buffer = vec![0_u8; 4096];
                        if let Ok(read) = tls.read(&mut buffer).await {
                            lock(&requests).extend_from_slice(&buffer[..read]);
                        }
                    }
                }
            });
        }
    });
    server
}

#[tokio::test(flavor = "multi_thread")]
async fn an_https_server_whose_certificate_chains_to_no_trusted_root_receives_nothing() {
    let server = untrusted_tls_server().await;
    let host = TestHost::new("principal-a");
    let client = widgets(&format!("https://{}", server.address), &host).await;

    let error = get_widget(&client, "w1").await.expect_err("fails closed");

    assert_clean(&error);
    // The client refuses the certificate during the handshake, so no handler
    // is ever handed a request.
    let waited = tokio::time::timeout(Duration::from_secs(10), async {
        while server.rejected.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    assert!(waited.is_ok(), "the client rejected the handshake");
    assert_eq!(server.completed.load(Ordering::SeqCst), 0);
    assert!(lock(&server.requests).is_empty(), "no request was served");
    assert!(
        host.reads.load(Ordering::SeqCst) >= 1,
        "the host was asked for its credential, which never left the process"
    );
    assert_spans_clean(&host);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unreachable_https_host_leaves_no_address_in_the_error() {
    // A port nothing listens on, reached by name so the resolved addresses are
    // the only place an IP could come from.
    let port = {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("binds");
        listener.local_addr().expect("an address").port()
    };
    let host = TestHost::new("principal-a");
    let client = widgets(&format!("https://localhost:{port}"), &host).await;

    let error = get_widget(&client, "w1").await.expect_err("unreachable");

    let FfiError::Call { bytes } = &error;
    let formatted = format_error(bytes.clone(), "en", None);
    for output in [bytes.clone(), formatted, format!("{error:?}").into_bytes()] {
        let text = String::from_utf8_lossy(&output);
        assert!(!text.contains("127.0.0.1"), "{text}");
        assert!(!text.contains("::1"), "{text}");
        assert!(!text.contains(&port.to_string()), "{text}");
    }
    assert_clean(&error);
}

// ---- the host's failures (SEC-2b, SEC-5) -----------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn a_host_that_fails_or_panics_answers_the_fixed_refusal_and_no_request_leaves() {
    for credential in [Credential::Fails, Credential::Panics, Credential::Garbled] {
        let (shared, url) = serve().await;
        let host = TestHost::scripted(credential, false);
        let client = widgets(&url, &host).await;

        let error = get_widget(&client, "w1").await.expect_err("fails closed");

        assert_eq!(shared.requests(), 0, "no request without a credential");
        assert!(host.reads.load(Ordering::SeqCst) >= 1);
        assert_ne!(call_error(&error).connect_code, ConnectCode::NotFound);
        assert_clean(&error);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_refresh_leaks_nothing() {
    let (shared, url) = serve().await;
    shared.unauthenticated.store(8, Ordering::SeqCst);
    let host = TestHost::scripted(Credential::Answers, true);
    let client = widgets(&url, &host).await;

    let error = get_widget(&client, "w1").await.expect_err("refused");

    assert_clean(&error);
    assert_spans_clean(&host);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cancelled_stream_leaks_nothing_through_its_span() {
    let (_shared, url) = serve().await;
    let host = TestHost::new("principal-a");
    let client = l2::EventsClientFfi::new(url, host.clone(), Vec::new()).expect("a client");

    let handle =
        l2::api_bones_eventsclient_watch(client, [0x08, 5].to_vec(), FfiCallCtx::default())
            .await
            .expect("a handle");
    handle.cancel();
    let _ = handle.next().await;

    assert!(!leaks(handle.span().unwrap_or_default().as_bytes()));
    assert_spans_clean(&host);
}

// ---- the second principal (SEC-2) -------------------------------------------------

// The revision cache is process-wide and these tests run in parallel, so each
// names principals of its own: a refusal or a cached list of one never meets
// another test's walk.

#[tokio::test(flavor = "multi_thread")]
async fn a_second_principal_never_reads_the_first_ones_cached_list() {
    let (_shared, url) = serve().await;
    let a = numbers(&url, &TestHost::new("second-a")).await;
    let b = numbers(&url, &TestHost::new("second-b")).await;

    let first = pages(&list(&a)).await;
    let again = pages(&list(&a)).await;
    let other = pages(&list(&b)).await;

    assert!(!first[0].1, "the first list is read from the server");
    assert!(again[0].1, "the same principal's second list is cached");
    assert_eq!(other.len(), 1, "only the second principal's own page");
    assert!(!other[0].1, "never served from the first principal's slot");
    assert_eq!(other[0].0.len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refusal_clears_the_refused_principals_cached_list_and_no_other() {
    let (shared, url) = serve().await;
    let a = numbers(&url, &TestHost::new("refusal-a")).await;
    let b = numbers(&url, &TestHost::new("refusal-b")).await;
    pages(&list(&a)).await;
    pages(&list(&b)).await;
    assert!(
        pages(&list(&a)).await[0].1,
        "a is cached before the refusal"
    );
    assert!(
        pages(&list(&b)).await[0].1,
        "b is cached before the refusal"
    );

    shared.unauthenticated.store(8, Ordering::SeqCst);
    let refused = list(&a).next().await.expect_err("a is refused");
    shared.unauthenticated.store(0, Ordering::SeqCst);

    assert_eq!(
        call_error(&refused).connect_code,
        ConnectCode::Unauthenticated
    );
    let a_again = pages(&list(&a)).await;
    assert!(!a_again[0].1, "a reads from the server after its refusal");
    assert_eq!(a_again[0].0.len(), 2, "and gets a's own first page");
    let b_again = pages(&list(&b)).await;
    assert!(b_again[0].1, "b's slot is untouched by a's refusal");
    assert_eq!(b_again[0].0.len(), 1, "and is b's own page, never a's");
}
