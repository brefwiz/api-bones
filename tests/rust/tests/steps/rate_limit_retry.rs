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
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use api_bones_batch::{Batched, CallHeaders};
use api_bones_contract_rust::{
    BatchGetWidgetsRequest, BatchGetWidgetsResponse, GetWidgetRequest, GetWidgetResponse,
    ListIdempotentRequest, ListIdempotentResponse, ListNoSideEffectsRequest,
    ListNoSideEffectsResponse, ListRequest, ListResponse, NumbersService, NumbersServiceClient,
    NumbersServiceExt, NumbersServiceLists, PageResponse, Widget, WidgetGets, WidgetsService,
    WidgetsServiceClient, WidgetsServiceExt,
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

use super::support::{batch_response, unserved_widget_methods};
use crate::world::RateLimitWorld;

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
    in_trailers: bool,
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
        let refusal = ConnectError::resource_exhausted("slow down");
        Err(if self.in_trailers {
            refusal.with_trailers(headers)
        } else {
            refusal.with_headers(headers)
        })
    }

    /// The page position of one admitted list call.
    fn page(&self) -> Result<PageResponse, ConnectError> {
        self.admit()?;
        Ok(PageResponse {
            next_cursor: String::new(),
            revision: "r1".into(),
            ..Default::default()
        })
    }
}

impl NumbersService for Throttle {
    async fn list_no_side_effects<'a>(
        &'a self,
        _ctx: RequestContext,
        _request: ServiceRequest<'_, ListNoSideEffectsRequest>,
    ) -> ServiceResult<impl connectrpc::Encodable<ListNoSideEffectsResponse> + Send + use<'a>> {
        Response::ok(ListNoSideEffectsResponse {
            items: vec![1],
            page: self.page()?.into(),
            ..Default::default()
        })
    }

    async fn list_idempotent<'a>(
        &'a self,
        _ctx: RequestContext,
        _request: ServiceRequest<'_, ListIdempotentRequest>,
    ) -> ServiceResult<impl connectrpc::Encodable<ListIdempotentResponse> + Send + use<'a>> {
        Response::ok(ListIdempotentResponse {
            items: vec![1],
            page: self.page()?.into(),
            ..Default::default()
        })
    }

    async fn list<'a>(
        &'a self,
        _ctx: RequestContext,
        _request: ServiceRequest<'_, ListRequest>,
    ) -> ServiceResult<impl connectrpc::Encodable<ListResponse> + Send + use<'a>> {
        Response::ok(ListResponse {
            items: vec![1],
            page: self.page()?.into(),
            ..Default::default()
        })
    }
}

impl WidgetsService for Throttle {
    async fn get_widget<'a>(
        &'a self,
        _ctx: RequestContext,
        request: ServiceRequest<'_, GetWidgetRequest>,
    ) -> ServiceResult<impl connectrpc::Encodable<GetWidgetResponse> + Send + use<'a>> {
        self.admit()?;
        let asked = request.to_owned_message();
        Response::ok(GetWidgetResponse {
            widget: Widget {
                id: asked.id.clone(),
                ..Default::default()
            }
            .into(),
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
        Response::ok(batch_response(&asked.ids, |id| {
            Some(Widget {
                id: id.to_owned(),
                kind: asked.kind.clone(),
                ..Default::default()
            })
        }))
    }

    unserved_widget_methods!("the throttle serves only the method under test");
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
        in_trailers: world.retry_after_in_trailers,
        arrivals: world.arrivals.clone(),
    });
    world.server = Some(tokio::spawn(async move {
        let _ = bound
            .serve_with_service(ConnectRpcService::new(router))
            .await;
    }));
    ClientConfig::new(format!("http://{address}").parse().expect("a loopback uri"))
}

/// The batch runtime's client for the widget service. It is written out, not
/// aliased, so the step reads as holding an SDK client. The batch path rides
/// `GetWidget`, so the scenario must declare what that method declares.
async fn widgets(world: &mut RateLimitWorld) -> Batched<WidgetsServiceClient<HttpTransport>> {
    assert_eq!(
        world.idempotency, "NO_SIDE_EFFECTS",
        "the batch path serves only a method declared NO_SIDE_EFFECTS"
    );
    let config = serve(world, |throttle| {
        WidgetsServiceExt::register(Arc::new(throttle), Router::new())
    })
    .await;
    Batched::anonymous(WidgetsServiceClient::new(HttpTransport::h2c(), config))
        .expect("the widget client binds")
}

/// The list-only client of the paged call type, which needs no batch method.
async fn numbers(world: &mut RateLimitWorld) -> Batched<NumbersServiceClient<HttpTransport>> {
    let config = serve(world, |throttle| {
        NumbersServiceExt::register(Arc::new(throttle), Router::new())
    })
    .await;
    Batched::anonymous(NumbersServiceClient::new(HttpTransport::h2c(), config))
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

/// `at` as an HTTP-date, whole seconds in GMT.
fn http_date(at: SystemTime) -> String {
    const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let seconds = at
        .duration_since(UNIX_EPOCH)
        .expect("the date is after the epoch")
        .as_secs();
    let days = i64::try_from(seconds / 86_400).expect("a date within range");
    let of_day = seconds % 86_400;
    let weekday = DAYS[usize::try_from((days + 4) % 7).expect("a weekday")];
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{weekday}, {day:02} {} {year} {:02}:{:02}:{:02} GMT",
        MONTHS[usize::try_from(month - 1).expect("a month")],
        of_day / 3600,
        of_day % 3600 / 60,
        of_day % 60
    )
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

#[given(expr = "the server refuses it {int} times with a Retry-After date {int} seconds ahead")]
fn given_refuses_with_date(world: &mut RateLimitWorld, refusals: usize, ahead: u64) {
    world.refusals = refusals;
    world.retry_after = Some(http_date(SystemTime::now() + Duration::from_secs(ahead)));
}

#[given(expr = "the server refuses it {int} times with Retry-After {string} in the trailers")]
fn given_refuses_in_trailers(world: &mut RateLimitWorld, refusals: usize, retry_after: String) {
    world.refusals = refusals;
    world.retry_after = Some(retry_after);
    world.retry_after_in_trailers = true;
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
            match world.idempotency.as_str() {
                "NO_SIDE_EFFECTS" => ListCall::items(
                    client.list_no_side_effects(ListNoSideEffectsRequest::default()),
                )
                .try_collect::<Vec<_>>()
                .await
                .map(drop)
                .map_err(refusal_of),
                "IDEMPOTENT" => {
                    ListCall::items(client.list_idempotent(ListIdempotentRequest::default()))
                        .try_collect::<Vec<_>>()
                        .await
                        .map(drop)
                        .map_err(refusal_of)
                }
                "UNSPECIFIED" => ListCall::items(client.list(ListRequest::default()))
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

/// The `retry-after` a rate-limited refusal states, as the wire carried it.
fn stated_retry_after(world: &RateLimitWorld) -> &str {
    let refusal = world
        .outcome
        .as_ref()
        .expect("the call was never sent")
        .as_ref()
        .expect_err("the call was served");
    assert_eq!(refusal.code, ErrorCode::ResourceExhausted);
    refusal
        .response_headers()
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .expect("a rate limit carries its retry-after")
}

#[then(expr = "the throttled call is rejected as a rate limit after {int} ms on attempt {int}")]
fn then_rate_limited(world: &mut RateLimitWorld, wait_ms: u64, attempt: usize) {
    let stated = stated_retry_after(world);
    let stated = stated
        .parse::<u64>()
        .unwrap_or_else(|_| panic!("the retry-after {stated:?} is not whole seconds"));
    assert_eq!(stated * 1000, wait_ms);
    assert_eq!(lock(&world.arrivals).len(), attempt);
}

#[then(
    expr = "the throttled call is rejected as a rate limit naming a wait between {int} and {int} ms on attempt {int}"
)]
fn then_rate_limited_in_whole_seconds(
    world: &mut RateLimitWorld,
    least_ms: u64,
    most_ms: u64,
    attempt: usize,
) {
    let stated = stated_retry_after(world);
    let seconds = stated
        .parse::<u64>()
        .unwrap_or_else(|_| panic!("the retry-after {stated:?} is not whole seconds"));
    assert!(
        (least_ms..=most_ms).contains(&(seconds * 1000)),
        "the retry-after of {seconds} s is outside {least_ms}..{most_ms} ms"
    );
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
