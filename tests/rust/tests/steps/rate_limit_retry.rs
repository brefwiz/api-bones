// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! The Rust half of `../../../features/connect_rate_limit_retry.feature`.
//!
//! A real connectrpc server on loopback refuses calls as the platform limiter
//! does: the resource-exhausted code with a `retry-after` header, before any
//! handler work. It records when every request arrives, so a step reads how
//! often a call was sent and how long the replay waited. Calls without a
//! deadline go through the generated list client of the paged call type; calls
//! with a deadline or an abort go through the batch runtime's coalesced get,
//! which owns the call's deadline and the cancellation of its wire request.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use api_bones_batch::{Batched, CallHeaders};
use api_bones_contract_rust::{
    BatchGetWidgetsRequest, BatchGetWidgetsResponse, BatchListWidgetPartsRequest,
    BatchListWidgetPartsResponse, GetWidgetRequest, ListNumbersRequest, ListNumbersResponse,
    ListWidgetPartsRequest, ListWidgetPartsResponse, Numbers, NumbersClient, NumbersExt,
    NumbersLists, PageResponse, UpdateWidgetPartsRequest, Widget, WidgetGets, WidgetResult,
    Widgets, WidgetsClient, WidgetsExt, widget_result,
};
use api_bones_page::{ListCall, PageError, PageErrorKind};
use api_bones_transport::HttpTransport;
use connectrpc::client::ClientConfig;
use connectrpc::{
    ConnectError, ConnectRpcService, ErrorCode, RequestContext, Response, Router, Server,
    ServiceRequest, ServiceResult,
};
use cucumber::{given, then, when};
use futures_util::TryStreamExt;
use futures_util::future::{Either, select};

use crate::world::RateLimitWorld;

type WidgetsBatched = Batched<WidgetsClient<HttpTransport>>;
type NumbersBatched = Batched<NumbersClient<HttpTransport>>;

/// How long a step waits for the first refusal to arrive before it gives up.
const FIRST_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(5);
/// The pause after the first refusal that puts the call inside its wait.
const INTO_THE_WAIT: Duration = Duration::from_millis(100);
/// How long past its Retry-After a step watches for a replay that must not come.
const PAST_THE_WAIT: Duration = Duration::from_millis(500);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().expect("the recorder is never poisoned")
}

/// A service that refuses its first `refusals` requests as rate limited and
/// serves the rest.
#[derive(Clone)]
struct Throttle {
    refusals: usize,
    retry_after: Option<String>,
    arrivals: Arc<Mutex<Vec<Instant>>>,
}

impl Throttle {
    /// Records the arrival, then refuses it while refusals remain.
    fn admit(&self) -> Result<(), ConnectError> {
        let attempt = {
            let mut arrivals = lock(&self.arrivals);
            arrivals.push(Instant::now());
            arrivals.len()
        };
        if attempt > self.refusals {
            return Ok(());
        }
        let mut headers = http::HeaderMap::new();
        if let Some(wait) = &self.retry_after {
            headers.insert(
                "retry-after",
                http::HeaderValue::from_str(wait).expect("the contract states a header value"),
            );
        }
        Err(ConnectError::resource_exhausted("slow down").with_headers(headers))
    }

    fn page(&self) -> Result<ListNumbersResponse, ConnectError> {
        self.admit()?;
        Ok(ListNumbersResponse {
            items: vec![1],
            page: PageResponse {
                next_cursor: String::new(),
                revision: "r1".into(),
                ..Default::default()
            }
            .into(),
            ..Default::default()
        })
    }
}

impl Numbers for Throttle {
    async fn list_no_side_effects<'a>(
        &'a self,
        _ctx: RequestContext,
        _request: ServiceRequest<'_, ListNumbersRequest>,
    ) -> ServiceResult<impl connectrpc::Encodable<ListNumbersResponse> + Send + use<'a>> {
        Response::ok(self.page()?)
    }

    async fn list_idempotent<'a>(
        &'a self,
        _ctx: RequestContext,
        _request: ServiceRequest<'_, ListNumbersRequest>,
    ) -> ServiceResult<impl connectrpc::Encodable<ListNumbersResponse> + Send + use<'a>> {
        Response::ok(self.page()?)
    }

    async fn list<'a>(
        &'a self,
        _ctx: RequestContext,
        _request: ServiceRequest<'_, ListNumbersRequest>,
    ) -> ServiceResult<impl connectrpc::Encodable<ListNumbersResponse> + Send + use<'a>> {
        Response::ok(self.page()?)
    }
}

/// The answer of a method this scenario never calls.
fn unserved<T>() -> ServiceResult<T> {
    Err(ConnectError::unimplemented(
        "the throttle serves only the method under test",
    ))
}

impl Widgets for Throttle {
    async fn get_widget<'a>(
        &'a self,
        _ctx: RequestContext,
        request: ServiceRequest<'_, GetWidgetRequest>,
    ) -> ServiceResult<impl connectrpc::Encodable<Widget> + Send + use<'a>> {
        self.admit()?;
        let asked = request.to_owned_message();
        Response::ok(Widget {
            id: asked.id.clone(),
            ..Default::default()
        })
    }

    async fn batch_get_widgets<'a>(
        &'a self,
        _ctx: RequestContext,
        request: ServiceRequest<'_, BatchGetWidgetsRequest>,
    ) -> ServiceResult<impl connectrpc::Encodable<BatchGetWidgetsResponse> + Send + use<'a>> {
        self.admit()?;
        let asked = request.to_owned_message();
        let results = asked
            .ids
            .iter()
            .map(|id| WidgetResult {
                id: id.clone(),
                outcome: Some(widget_result::Outcome::Item(Box::new(Widget {
                    id: id.clone(),
                    kind: asked.kind.clone(),
                    ..Default::default()
                }))),
                ..Default::default()
            })
            .collect();
        Response::ok(BatchGetWidgetsResponse {
            results,
            ..Default::default()
        })
    }

    async fn list_widget_parts<'a>(
        &'a self,
        _ctx: RequestContext,
        _request: ServiceRequest<'_, ListWidgetPartsRequest>,
    ) -> ServiceResult<impl connectrpc::Encodable<ListWidgetPartsResponse> + Send + use<'a>> {
        unserved::<ListWidgetPartsResponse>()
    }

    async fn list_widget_parts_idempotent<'a>(
        &'a self,
        _ctx: RequestContext,
        _request: ServiceRequest<'_, ListWidgetPartsRequest>,
    ) -> ServiceResult<impl connectrpc::Encodable<ListWidgetPartsResponse> + Send + use<'a>> {
        unserved::<ListWidgetPartsResponse>()
    }

    async fn list_widget_parts_unspecified<'a>(
        &'a self,
        _ctx: RequestContext,
        _request: ServiceRequest<'_, ListWidgetPartsRequest>,
    ) -> ServiceResult<impl connectrpc::Encodable<ListWidgetPartsResponse> + Send + use<'a>> {
        unserved::<ListWidgetPartsResponse>()
    }

    async fn update_widget_parts<'a>(
        &'a self,
        _ctx: RequestContext,
        _request: ServiceRequest<'_, UpdateWidgetPartsRequest>,
    ) -> ServiceResult<impl connectrpc::Encodable<ListWidgetPartsResponse> + Send + use<'a>> {
        unserved::<ListWidgetPartsResponse>()
    }

    async fn batch_list_widget_parts<'a>(
        &'a self,
        _ctx: RequestContext,
        _request: ServiceRequest<'_, BatchListWidgetPartsRequest>,
    ) -> ServiceResult<impl connectrpc::Encodable<BatchListWidgetPartsResponse> + Send + use<'a>>
    {
        unserved::<BatchListWidgetPartsResponse>()
    }
}

/// Serves the router `register` builds over this scenario's throttle and
/// returns the client configuration that reaches it.
async fn serve(
    world: &mut RateLimitWorld,
    register: impl FnOnce(Throttle) -> Router,
) -> ClientConfig {
    let bound = Server::bind("127.0.0.1:0")
        .await
        .expect("the throttle server binds");
    let address = bound
        .local_addr()
        .expect("the throttle server has an address");
    let router = register(Throttle {
        refusals: world.refusals,
        retry_after: world.retry_after.clone(),
        arrivals: world.arrivals.clone(),
    });
    world.server = Some(tokio::spawn(async move {
        let _ = bound
            .serve_with_service(ConnectRpcService::new(router))
            .await;
    }));
    ClientConfig::new(format!("http://{address}").parse().expect("a loopback uri"))
}

/// The batch runtime's client for the widget service. The batch path rides
/// `GetWidget`, so the scenario must declare what that method declares.
async fn widgets(world: &mut RateLimitWorld) -> WidgetsBatched {
    assert_eq!(
        world.idempotency, "NO_SIDE_EFFECTS",
        "the batch path serves only a method declared NO_SIDE_EFFECTS"
    );
    let config = serve(world, |throttle| {
        WidgetsExt::register(Arc::new(throttle), Router::new())
    })
    .await;
    Batched::anonymous(WidgetsClient::new(HttpTransport::h2c(), config))
        .expect("the widget client binds")
}

/// The list-only client of the paged call type, which needs no batch method.
async fn numbers(world: &mut RateLimitWorld) -> NumbersBatched {
    let config = serve(world, |throttle| {
        NumbersExt::register(Arc::new(throttle), Router::new())
    })
    .await;
    Batched::anonymous(NumbersClient::new(HttpTransport::h2c(), config))
        .expect("a list-only client binds without a batch method")
}

impl RateLimitWorld {
    /// The wait the server asked for.
    fn retry_after_ms(&self) -> u64 {
        let stated = self
            .retry_after
            .as_deref()
            .expect("the scenario names no Retry-After");
        stated
            .parse::<u64>()
            .expect("the contract states whole seconds")
            * 1000
    }
}

fn widget_request() -> GetWidgetRequest {
    GetWidgetRequest {
        id: "w1".into(),
        ..Default::default()
    }
}

/// The refusal a page stream ended with; any other page failure is a defect
/// of the scenario, not an outcome it describes.
fn refusal_of(error: PageError) -> ConnectError {
    match error.kind {
        PageErrorKind::Rpc(refusal) => refusal,
        other => panic!("the list failed as something other than a refusal: {other:?}"),
    }
}

/// Resolves once the server has seen the first attempt and the call has had
/// time to settle into its wait.
async fn into_the_wait(arrivals: Arc<Mutex<Vec<Instant>>>) {
    let first = async {
        while lock(&arrivals).is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    };
    tokio::time::timeout(FIRST_ATTEMPT_TIMEOUT, first)
        .await
        .expect("the server never saw the first attempt");
    tokio::time::sleep(INTO_THE_WAIT).await;
}

#[given(expr = "a throttled method declared {string}")]
fn given_declared(world: &mut RateLimitWorld, idempotency: String) {
    world.idempotency = idempotency;
}

#[given(expr = "the throttled call has a deadline of {int} ms")]
fn given_deadline(world: &mut RateLimitWorld, millis: u64) {
    world.deadline_ms = Some(millis);
}

#[given(expr = "the server refuses it {int} times with Retry-After {string}")]
fn given_refuses_with(world: &mut RateLimitWorld, refusals: usize, retry_after: String) {
    world.refusals = refusals;
    world.retry_after = Some(retry_after);
}

#[given(expr = "the server refuses it {int} times without Retry-After")]
fn given_refuses_without(world: &mut RateLimitWorld, refusals: usize) {
    world.refusals = refusals;
    world.retry_after = None;
}

#[when("the throttled call is sent")]
async fn when_sent(world: &mut RateLimitWorld) {
    let started = Instant::now();
    world.outcome = Some(match world.deadline_ms {
        Some(millis) => {
            let client = widgets(world).await;
            let headers = CallHeaders::new().timeout(Duration::from_millis(millis));
            client
                .get_widget_with_options(widget_request(), headers)
                .await
                .map(drop)
        }
        None => {
            let client = numbers(world).await;
            let request = ListNumbersRequest::default();
            match world.idempotency.as_str() {
                "NO_SIDE_EFFECTS" => ListCall::items(client.list_no_side_effects(request))
                    .try_collect::<Vec<_>>()
                    .await
                    .map(drop)
                    .map_err(refusal_of),
                "IDEMPOTENT" => ListCall::items(client.list_idempotent(request))
                    .try_collect::<Vec<_>>()
                    .await
                    .map(drop)
                    .map_err(refusal_of),
                "UNSPECIFIED" => ListCall::items(client.list(request))
                    .try_collect::<Vec<_>>()
                    .await
                    .map(drop)
                    .map_err(refusal_of),
                other => panic!("the contract declares no method for idempotency {other}"),
            }
        }
    });
    world.elapsed = started.elapsed();
}

#[when("the throttled call is sent and abandoned during the wait")]
async fn when_abandoned(world: &mut RateLimitWorld) {
    let client = widgets(world).await;
    let started = Instant::now();
    let call = Box::pin(async { client.get_widget(widget_request()).await });
    let waited = Box::pin(into_the_wait(world.arrivals.clone()));
    world.outcome = match select(call, waited).await {
        Either::Left((finished, _)) => Some(finished.map(drop)),
        Either::Right((_, call)) => {
            drop(call);
            None
        }
    };
    world.elapsed = started.elapsed();
}

#[then(expr = "the throttled call succeeds on attempt {int}")]
fn then_succeeds(world: &mut RateLimitWorld, attempt: usize) {
    assert_eq!(
        world.outcome.as_ref().map(Result::is_ok),
        Some(true),
        "the call did not succeed: {:?}",
        world.outcome
    );
    assert_eq!(lock(&world.arrivals).len(), attempt);
}

#[then(expr = "the replay arrived at least {int} ms after the refusal")]
fn then_replay_gap(world: &mut RateLimitWorld, minimum_ms: u64) {
    let arrivals = lock(&world.arrivals);
    assert!(arrivals.len() >= 2, "the call was never replayed");
    let gap = arrivals[1].duration_since(arrivals[0]);
    assert!(
        gap >= Duration::from_millis(minimum_ms),
        "the replay came {gap:?} after the refusal, expected at least {minimum_ms} ms"
    );
}

#[then(expr = "the throttled call is rejected as a rate limit after {int} ms on attempt {int}")]
fn then_rate_limited(world: &mut RateLimitWorld, wait_ms: u64, attempt: usize) {
    let refusal = world
        .outcome
        .as_ref()
        .expect("the call was never sent")
        .as_ref()
        .expect_err("the call was served");
    assert_eq!(refusal.code, ErrorCode::ResourceExhausted);
    let stated = refusal
        .response_headers()
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .expect("a rate limit carries its retry-after");
    assert_eq!(stated * 1000, wait_ms);
    assert_eq!(lock(&world.arrivals).len(), attempt);
}

#[then(expr = "the throttled call is rejected as a plain refusal on attempt {int}")]
fn then_plain_refusal(world: &mut RateLimitWorld, attempt: usize) {
    let refusal = world
        .outcome
        .as_ref()
        .expect("the call was never sent")
        .as_ref()
        .expect_err("the call was served");
    assert_eq!(refusal.code, ErrorCode::ResourceExhausted);
    assert!(
        refusal.response_headers().get("retry-after").is_none(),
        "a plain refusal carries no retry-after"
    );
    assert_eq!(lock(&world.arrivals).len(), attempt);
}

#[then(
    expr = "the throttled call ends as deadline exceeded before its Retry-After on attempt {int}"
)]
fn then_deadline_exceeded(world: &mut RateLimitWorld, attempt: usize) {
    let refusal = world
        .outcome
        .as_ref()
        .expect("the call was never sent")
        .as_ref()
        .expect_err("the call was served");
    assert_eq!(refusal.code, ErrorCode::DeadlineExceeded);
    assert_eq!(lock(&world.arrivals).len(), attempt);
    assert!(
        world.elapsed < Duration::from_millis(world.retry_after_ms()),
        "the call ran {:?}, which waited the Retry-After out",
        world.elapsed
    );
}

#[then(expr = "the throttled call ends as canceled before its Retry-After on attempt {int}")]
async fn then_canceled(world: &mut RateLimitWorld, attempt: usize) {
    assert_eq!(
        world.outcome.as_ref().map(Result::is_ok),
        None,
        "the call ended before it was abandoned: {:?}",
        world.outcome
    );
    assert!(
        world.elapsed < Duration::from_millis(world.retry_after_ms()),
        "the call ran {:?}, which waited the Retry-After out",
        world.elapsed
    );
    // A cancelled wait never sends the replay its Retry-After scheduled.
    tokio::time::sleep(Duration::from_millis(world.retry_after_ms()) + PAST_THE_WAIT).await;
    assert_eq!(
        lock(&world.arrivals).len(),
        attempt,
        "the abandoned call was replayed after its wait"
    );
}
