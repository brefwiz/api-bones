// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! The server stream of `../../../sdk-call/features/sdk_call.feature`: an `Events`
//! service on the lane's loopback server, and the steps that watch it through
//! the test service's Layer 2.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use api_bones_contract_rust::{Events, Tick as WireTick, WatchTicksRequest};
use api_bones_sdk::Handle;
use api_bones_sdk::call::CALL_BUDGET;
use api_bones_test_service_l2::Tick;
use connectrpc::{
    ConnectError, Encodable, RequestContext, Response, ServiceRequest, ServiceResult, ServiceStream,
};
use cucumber::{given, then, when};
use futures_util::stream;

use super::{SdkCallWorld, Shared, base_url, call_env, serve};

/// The `Events` service of the lane's server.
#[derive(Clone)]
pub(super) struct EventsService(pub(super) Arc<Shared>);

impl Events for EventsService {
    async fn watch(
        &self,
        ctx: RequestContext,
        request: ServiceRequest<'_, WatchTicksRequest>,
    ) -> ServiceResult<ServiceStream<impl Encodable<WireTick> + Send + use<>>> {
        let count = request.to_owned_message().count;
        self.0.admit(&ctx)?;
        let every = Duration::from_millis(self.0.tick_ms.load(Ordering::SeqCst));
        // The response head is returned now; each tick follows `every` later.
        Response::stream_ok(stream::unfold(0_u32, move |next| async move {
            if next >= count {
                return None;
            }
            tokio::time::sleep(every).await;
            let tick: Result<WireTick, ConnectError> = Ok(WireTick {
                sequence: next,
                ..Default::default()
            });
            Some((tick, next + 1))
        }))
    }
}

#[given(expr = "the events server streams {int} ticks")]
async fn given_ticks(world: &mut SdkCallWorld, count: u32) {
    world.shared.ticks.store(count, Ordering::SeqCst);
    serve(world).await;
}

#[given(expr = "the events server waits {int} ms before each tick")]
fn given_tick_wait(world: &mut SdkCallWorld, millis: u64) {
    world.shared.tick_ms.store(millis, Ordering::SeqCst);
}

async fn open_watch(world: &SdkCallWorld) -> Handle<Tick> {
    let count = world.shared.ticks.load(Ordering::SeqCst);
    api_bones_test_service_l2::EventsClient::new(&base_url(world), call_env(world))
        .expect("the lane's server address is a URI")
        .watch(count)
        .await
}

/// Watches the events the way a Rust caller does, reading ticks until the
/// stream ends or is refused.
async fn watch_events(world: &mut SdkCallWorld) {
    let started = Instant::now();
    let ticks = open_watch(world).await;
    loop {
        match ticks.next().await {
            Ok(Some(tick)) => world.ticks.push(tick),
            Ok(None) => {
                world.watch_over = true;
                break;
            }
            Err(error) => {
                world.refusal = Some(error);
                break;
            }
        }
    }
    world.watched = started.elapsed();
    world.watch = Some(ticks);
}

#[when("the caller watches the events through the SDK")]
async fn when_watch(world: &mut SdkCallWorld) {
    watch_events(world).await;
}

#[given("the caller has opened a watch of the events through the SDK")]
async fn given_watch_opened(world: &mut SdkCallWorld) {
    world.watch = Some(open_watch(world).await);
}

#[given("the caller has read the first tick of the watch")]
async fn given_first_tick_read(world: &mut SdkCallWorld) {
    when_next_tick(world).await;
}

#[given("the caller has read the next tick of the watch")]
async fn given_next_tick_read(world: &mut SdkCallWorld) {
    when_next_tick(world).await;
}

#[when("the caller reads the next tick of the watch")]
async fn when_next_tick(world: &mut SdkCallWorld) {
    let watch = world.watch.as_ref().expect("a Given opened the watch");
    match watch.next().await {
        Ok(Some(tick)) => world.ticks.push(tick),
        Ok(None) => world.watch_over = true,
        Err(error) => {
            world.refusal = Some(error);
            world.watch_over = matches!(watch.next().await, Ok(None));
        }
    }
}

#[when(expr = "the caller watches the events through the SDK and leaves after {int} ticks")]
async fn when_watch_and_leave(world: &mut SdkCallWorld, count: usize) {
    let ticks = open_watch(world).await;
    for _ in 0..count {
        let tick = ticks
            .next()
            .await
            .expect("a tick before the caller left")
            .expect("the watch had a tick to give");
        world.ticks.push(tick);
    }
    ticks.cancel();
    world.watch = Some(ticks);
}

#[then("the watch gives nothing after the caller left")]
async fn then_watch_left(world: &mut SdkCallWorld) {
    let watch = world.watch.as_ref().expect("the caller opened a watch");
    assert!(
        matches!(watch.next().await, Ok(None)),
        "the watch gave more after the caller left"
    );
}

#[then("the watch has ended")]
fn then_watch_ended(world: &mut SdkCallWorld) {
    assert!(world.watch_over, "the watch gave more after it ended");
}

#[then(expr = "the caller received the ticks {string}")]
fn then_ticks(world: &mut SdkCallWorld, expected: String) {
    let received: Vec<String> = world
        .ticks
        .iter()
        .map(|tick| tick.sequence.to_string())
        .collect();
    assert_eq!(received.join(","), expected);
}

#[then("the watch lasted longer than the call budget")]
fn then_watch_outlasted_budget(world: &mut SdkCallWorld) {
    assert!(
        world.watched > CALL_BUDGET,
        "the watch lasted {:?}, within the {CALL_BUDGET:?} budget",
        world.watched
    );
}

#[then("the watch was not refused")]
fn then_watch_not_refused(world: &mut SdkCallWorld) {
    assert!(
        world.refusal.is_none(),
        "the watch was refused: {:?}",
        world.refusal
    );
}
