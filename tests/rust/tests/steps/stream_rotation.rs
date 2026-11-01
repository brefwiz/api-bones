// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! The Rust half of `../../../features/connect_stream_rotation.feature`.
//!
//! The steps drive the shipped `RotationMachine` directly with a virtual clock
//! in milliseconds: each event names the stream it happens on and the time it
//! happens at, and the machine's actions accumulate in a log the outcome steps
//! read. Events are plain strings whose id doubles as their dedupe key.
//!
//! The driver-level steps drive the shipped `rotating_stream` over a response
//! type generated from proto and declared with `stream_controlled!`. A consumer
//! whose response type carries no control field holds the generated
//! `PlainEvent`, and the compiler, not the step, decides whether the client
//! takes it.

use std::collections::VecDeque;
use std::time::Duration;

use crate::world::{StreamRotationWorld, WatchItem, WatchStream};
use api_bones_contract_rust::{
    PlainEvent, Refused, RotatingStreamProbe, WatchEvent, control_only, watch_event::Body,
};
use api_bones_stream::{
    ControlSignal, EndCause, OpenRequest, RotationAction, RotationFrame, STREAM_CONTROL_HEADER,
    STREAM_CONTROL_HEADER_VALUE, StreamControl, StreamRole,
};
use cucumber::{given, then, when};
use futures_util::StreamExt;
use tokio::sync::mpsc;

type Log = [RotationAction<String>];

fn has(log: &Log, wanted: &RotationAction<String>) -> bool {
    log.contains(wanted)
}

fn delivered(log: &Log) -> Vec<String> {
    log.iter()
        .filter_map(|action| match action {
            RotationAction::Deliver(value) => Some(value.clone()),
            _ => None,
        })
        .collect()
}

fn fell_back(log: &Log) -> bool {
    log.iter()
        .any(|action| matches!(action, RotationAction::Fallback(_)))
}

fn closed(log: &Log, stream: StreamRole) -> bool {
    has(log, &RotationAction::Close(stream))
}

/// The log with `more` appended.
fn extended(log: &Log, more: Vec<RotationAction<String>>) -> Vec<RotationAction<String>> {
    log.iter().cloned().chain(more).collect()
}

fn checkpoint_of(what: &str) -> Option<ControlSignal> {
    let rest = what.strip_prefix("checkpoints domain \"")?;
    let (domain, rest) = rest.split_once("\" in epoch ")?;
    let (epoch, rest) = rest.split_once(" live from ")?;
    let (live_from, delivered_through) = rest.split_once(" delivered through ")?;
    Some(ControlSignal::Checkpoint {
        domain: domain.to_owned(),
        epoch: epoch.parse().ok()?,
        live_from: live_from.parse().ok()?,
        delivered_through: delivered_through.parse().ok()?,
    })
}

/// Apply one phrase of the contract's event grammar to `stream` at `now` and
/// return the actions the machine answers with.
fn happen(
    world: &mut StreamRotationWorld,
    stream: StreamRole,
    what: &str,
    now: u64,
) -> Vec<RotationAction<String>> {
    if let Some(id) = what
        .strip_prefix("delivers event \"")
        .and_then(|rest| rest.strip_suffix('"'))
    {
        let frame = RotationFrame::from_data(id.to_owned(), Some(id.to_owned()));
        world.machine.on_frame(stream, frame, now)
    } else if let Some(control) = checkpoint_of(what) {
        world
            .machine
            .on_frame(stream, RotationFrame::from_control(control), now)
    } else if let Some(cut_in_ms) = what
        .strip_prefix("drains cutting in ")
        .and_then(|rest| rest.strip_suffix(" ms"))
        .and_then(|ms| ms.parse().ok())
    {
        let control = ControlSignal::Drain { cut_in_ms };
        world
            .machine
            .on_frame(stream, RotationFrame::from_control(control), now)
    } else if what == "sends a control that cannot be read" {
        let control = ControlSignal::Unreadable;
        world
            .machine
            .on_frame(stream, RotationFrame::from_control(control), now)
    } else if what == "ends" {
        world.machine.on_end(stream, now, EndCause::Ended)
    } else if what == "fails" {
        world.machine.on_end(stream, now, EndCause::Failed)
    } else {
        panic!("the contract names an event this step cannot build: {what}")
    }
}

#[given(regex = r"^the (old stream|replacement) (.+) at (\d+) ms$")]
#[when(regex = r"^the (old stream|replacement) (.+) at (\d+) ms$")]
fn when_stream_event(world: &mut StreamRotationWorld, who: String, what: String, at: u64) {
    let stream = match who.as_str() {
        "old stream" => StreamRole::Old,
        _ => StreamRole::Replacement,
    };
    let actions = happen(world, stream, &what, at);
    world.log = extended(&world.log, actions);
}

#[given(expr = "the old stream has already received event {string}")]
#[when(expr = "the old stream has already received event {string}")]
fn given_received(world: &mut StreamRotationWorld, id: String) {
    world
        .buffered
        .lock()
        .expect("buffer")
        .push(RotationFrame::from_data(id.clone(), Some(id)));
}

#[when(expr = "the clock reaches {int} ms")]
fn when_clock(world: &mut StreamRotationWorld, at: u64) {
    let actions = world.machine.on_tick(at);
    world.log = extended(&world.log, actions);
}

#[then("the client opens a replacement")]
fn then_opens(world: &mut StreamRotationWorld) {
    let log = &world.log;
    assert!(has(log, &RotationAction::Open), "no replacement was opened");
}

#[then("the client opens no replacement")]
fn then_opens_none(world: &mut StreamRotationWorld) {
    let log = &world.log;
    assert!(!has(log, &RotationAction::Open), "a replacement was opened");
}

#[then("the client switches to the replacement")]
fn then_switches(world: &mut StreamRotationWorld) {
    let log = &world.log;
    assert!(
        has(log, &RotationAction::Switch),
        "the client never switched"
    );
}

#[then("the client does not switch")]
fn then_does_not_switch(world: &mut StreamRotationWorld) {
    let log = &world.log;
    assert!(!has(log, &RotationAction::Switch), "the client switched");
}

#[then("the client keeps the old stream")]
fn then_keeps_old(world: &mut StreamRotationWorld) {
    let log = &world.log;
    assert!(
        fell_back(log),
        "the client never fell back to the old stream"
    );
    assert!(!has(log, &RotationAction::Switch), "the client switched");
    assert!(
        !has(log, &RotationAction::End),
        "the consumer's stream ended"
    );
}

#[then("the client has not given up on the replacement")]
fn then_not_given_up(world: &mut StreamRotationWorld) {
    let log = &world.log;
    assert!(!fell_back(log), "the client already fell back");
    assert!(
        !has(log, &RotationAction::End),
        "the consumer's stream ended"
    );
}

#[then("the replacement is closed")]
fn then_replacement_closed(world: &mut StreamRotationWorld) {
    let log = &world.log;
    assert!(
        closed(log, StreamRole::Replacement),
        "the replacement was not closed"
    );
}

#[then("the consumer's stream ends")]
fn then_stream_ends(world: &mut StreamRotationWorld) {
    let log = &world.log;
    assert!(
        has(log, &RotationAction::End),
        "the consumer's stream did not end"
    );
}

#[then("the consumer has received no events")]
fn then_no_events(world: &mut StreamRotationWorld) {
    let log = &world.log;
    assert_eq!(delivered(log), Vec::<String>::new());
}

#[then(expr = "the consumer receives events {string} in order")]
fn then_receives(world: &mut StreamRotationWorld, ids: String) {
    let log = &world.log;
    let got = delivered(log);
    let wanted: Vec<&str> = ids.split(", ").collect();
    assert_eq!(got, wanted);
    let mut distinct = got.clone();
    distinct.sort();
    distinct.dedup();
    assert_eq!(
        distinct.len(),
        got.len(),
        "an event reached the consumer twice"
    );
}

fn position_of_close(log: &Log) -> usize {
    log.iter()
        .position(|action| *action == RotationAction::Close(StreamRole::Old))
        .expect("the old stream was never closed")
}

fn position_of_delivery(log: &Log, id: &str) -> usize {
    log.iter()
        .position(|action| matches!(action, RotationAction::Deliver(value) if value == id))
        .unwrap_or_else(|| panic!("the consumer never received {id}"))
}

#[then(expr = "the old stream is closed after the consumer received {string}")]
fn then_closed_after(world: &mut StreamRotationWorld, id: String) {
    let log = &world.log;
    assert!(position_of_close(log) > position_of_delivery(log, &id));
}

#[then(expr = "the old stream is closed before the consumer received {string}")]
fn then_closed_before(world: &mut StreamRotationWorld, id: String) {
    let log = &world.log;
    assert!(position_of_close(log) < position_of_delivery(log, &id));
}

// The steps below drive the shipped `rotating_stream` driver, not the machine.
// The consumer's open function is scripted: it counts its calls, records the
// header each request carries and returns the first stream, then a replacement.

type Leg = WatchStream;

fn leg(mut rx: mpsc::UnboundedReceiver<WatchItem>) -> Leg {
    Box::pin(futures_util::stream::poll_fn(move |cx| rx.poll_recv(cx)))
}

fn id_of(event: &WatchEvent) -> Option<String> {
    event.body.as_ref().map(|Body::Id(id)| id.clone())
}

fn data_event(id: &str) -> WatchEvent {
    WatchEvent {
        body: Some(Body::Id(id.to_owned())),
        ..WatchEvent::default()
    }
}

fn send_on_first(world: &StreamRotationWorld, event: WatchEvent) {
    world
        .consumer
        .first
        .as_ref()
        .expect("the consumer has not opened its stream")
        .send(Ok(event))
        .expect("the first stream is open");
}

#[given(
    regex = r"^a consumer whose response type (declares|does not declare) the stream control field$"
)]
fn given_consumer(world: &mut StreamRotationWorld, declaration: String) {
    world.consumer.declared = declaration == "declares";
}

#[when("the consumer opens its stream through the client")]
async fn when_consumer_opens(world: &mut StreamRotationWorld) {
    // The consumer's scripted streams: the first, then its replacement.
    let (first_tx, first_rx) = mpsc::unbounded_channel();
    let (replacement_tx, replacement_rx) = mpsc::unbounded_channel();
    world.consumer.first = Some(first_tx);
    world.consumer.replacement = Some(replacement_tx);
    let mut legs: VecDeque<Leg> = VecDeque::from([leg(first_rx), leg(replacement_rx)]);
    let opens = opens_of(world);
    // The client takes only a response type that implements
    // `StreamControlled`, and the compiler decides which types do. A consumer
    // whose type does not is refused before any request exists; the
    // compile_fail example on `rotating_stream` proves the refusal.
    world.consumer.client_accepts = if declared(world) {
        RotatingStreamProbe::<WatchEvent>::ACCEPTS
    } else {
        RotatingStreamProbe::<PlainEvent>::ACCEPTS
    };
    let stream = if declared(world) {
        Some(
            api_bones_stream::rotating_stream(
                move |request: OpenRequest| {
                    opens.lock().expect("opens").push((
                        request.header_name.to_owned(),
                        request.header_value.to_owned(),
                    ));
                    std::future::ready(legs.pop_front().ok_or_else(|| "no more streams".to_owned()))
                },
                id_of,
            )
            .boxed(),
        )
    } else {
        None
    };
    world.consumer.stream = stream;
    if declared(world) {
        read_available(world).await;
    }
}

fn opens_of(
    world: &StreamRotationWorld,
) -> std::sync::Arc<std::sync::Mutex<Vec<(String, String)>>> {
    std::sync::Arc::clone(&world.consumer.opens)
}

fn declared(world: &StreamRotationWorld) -> bool {
    world.consumer.declared
}

#[when(expr = "the first stream drains cutting in {int} ms")]
async fn when_first_drains(world: &mut StreamRotationWorld, cut_in_ms: u32) {
    send_on_first(world, control_only(&StreamControl::Drain { cut_in_ms }));
    read_available(world).await;
}

#[when(expr = "the first stream delivers event {string}, a checkpoint and a drain notice")]
async fn when_first_delivers(world: &mut StreamRotationWorld, id: String) {
    send_on_first(world, data_event(&id));
    let checkpoint = StreamControl::Checkpoint {
        domain: "orders".to_owned(),
        live_from: 90,
        delivered_through: 99,
        epoch: 7,
    };
    send_on_first(world, control_only(&checkpoint));
    let drain = StreamControl::Drain { cut_in_ms: 30_000 };
    send_on_first(world, control_only(&drain));
    read_available(world).await;
}

#[then(regex = r#"^the open request (carries|does not carry) the "([^"]+)" header$"#)]
fn then_open_request_header(world: &mut StreamRotationWorld, verb: String, name: String) {
    assert_eq!(name, STREAM_CONTROL_HEADER, "the contract names the header");
    let carries = verb == "carries";
    let declaration = (name, STREAM_CONTROL_HEADER_VALUE.to_owned());
    let opens = world.consumer.opens.lock().expect("opens");
    // A declared type opens through the client with the header. A type the
    // client refuses never opens through it at all.
    assert_eq!(
        world.consumer.client_accepts, carries,
        "the client's acceptance of the response type does not match the declaration"
    );
    assert_eq!(
        opens.first(),
        carries.then_some(&declaration),
        "the open request does not match the declaration"
    );
}

#[then("the consumer's open function has been called once for each stream")]
fn then_open_called_per_stream(world: &mut StreamRotationWorld) {
    let declaration = (
        STREAM_CONTROL_HEADER.to_owned(),
        STREAM_CONTROL_HEADER_VALUE.to_owned(),
    );
    let opens = world.consumer.opens.lock().expect("opens");
    assert_eq!(
        *opens,
        vec![declaration.clone(), declaration],
        "one call for the first stream and one for its replacement, each declaring stream control"
    );
}

#[then(expr = "the consumer receives only event {string}")]
async fn then_receives_only(world: &mut StreamRotationWorld, id: String) {
    assert_eq!(world.consumer.received, vec![id]);
    let stream = world
        .consumer
        .stream
        .as_mut()
        .expect("the consumer has not opened its stream");
    // Still pending: consuming the control frames did not end the stream, and
    // nothing else is ready.
    assert!(
        tokio::time::timeout(Duration::ZERO, stream.next())
            .await
            .is_err(),
        "the consumer's stream ended or delivered more after the control frames"
    );
}

/// Read what the consumer's stream has ready, without waiting for more.
async fn read_available(world: &mut StreamRotationWorld) {
    let held = &mut world.consumer.stream;
    let mut taken = held.take().expect("the consumer has not opened its stream");
    let mut reading = true;
    while reading {
        match tokio::time::timeout(Duration::ZERO, taken.next()).await {
            Ok(Some(Ok(event))) => {
                assert!(
                    !event.bones_control.is_set(),
                    "a control frame reached the consumer"
                );
                let id = id_of(&event).expect("the consumer received an event with no id");
                world.consumer.received.push(id);
            }
            Ok(Some(Err(error))) => panic!("the consumer's stream failed: {error}"),
            Ok(None) | Err(_) => reading = false,
        }
    }
    world.consumer.stream = Some(taken);
}
