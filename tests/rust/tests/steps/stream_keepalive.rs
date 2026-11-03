// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! The Rust half of `../../../features/connect_stream_keepalive.feature`.
//!
//! The scripted server follows the contract: after each full interval of
//! silence it sends one keepalive, an ordinary data message built by decoding
//! the keepalive payload as the stream's response type. The reader is the
//! shipped `skip_keepalives` over that stream.

use crate::world::StreamKeepaliveWorld;
use api_bones_contract_rust::{WatchEvent, watch_event::Body};
use api_bones_stream::{
    STREAM_KEEPALIVE_FIELD_NUMBER, STREAM_KEEPALIVE_HEADER, STREAM_KEEPALIVE_INTERVAL,
    STREAM_KEEPALIVE_PAYLOAD,
};
use buffa::{Message, UnknownField, UnknownFieldData, UnknownFields};
use connectrpc::client::CallOptions;
use cucumber::{given, then, when};
use futures_util::{StreamExt, stream};

fn hex(text: &str) -> Vec<u8> {
    assert_eq!(text.len() & 1, 0, "odd hex length in {text:?}");
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex digit"))
        .collect()
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

fn data_event(id: &str) -> WatchEvent {
    WatchEvent {
        body: Some(Body::Id(id.to_owned())),
        ..WatchEvent::default()
    }
}

fn keepalive_event() -> WatchEvent {
    WatchEvent::decode_from_slice(&STREAM_KEEPALIVE_PAYLOAD).expect("keepalive decodes")
}

fn silence(world: &mut StreamKeepaliveWorld, millis: u128) {
    let sent = millis / STREAM_KEEPALIVE_INTERVAL.as_millis();
    for _ in 0..sent {
        world.script.push(Ok(keepalive_event()));
        world.keepalives += 1;
    }
}

#[given("the keepalive payload")]
fn given_payload(world: &mut StreamKeepaliveWorld) {
    world.payload = STREAM_KEEPALIVE_PAYLOAD.to_vec();
}

#[given(expr = "the payload bytes {string}")]
fn given_bytes(world: &mut StreamKeepaliveWorld, bytes: String) {
    world.payload = hex(&bytes);
}

#[when("the payload is read as protobuf wire data")]
fn when_read_wire(world: &mut StreamKeepaliveWorld) {
    world.fields = Some(UnknownFields::decode_from_slice(&world.payload).expect("valid wire data"));
    world.verdict = Some(api_bones_stream::is_keepalive_payload(&world.payload));
}

#[when("the payload is checked for a keepalive")]
fn when_checked(world: &mut StreamKeepaliveWorld) {
    world.verdict = Some(api_bones_stream::is_keepalive_payload(&world.payload));
}

#[then(expr = "the payload is the bytes {string}")]
fn then_bytes(world: &mut StreamKeepaliveWorld, expected: String) {
    assert_eq!(to_hex(&world.payload), expected);
}

#[then(expr = "the payload holds one field, number {int}, as the varint {int}")]
fn then_one_field(world: &mut StreamKeepaliveWorld, number: u32, value: u64) {
    let fields: Vec<_> = world
        .fields
        .as_ref()
        .expect("payload not read")
        .iter()
        .collect();
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].number, number);
    assert_eq!(number, STREAM_KEEPALIVE_FIELD_NUMBER);
    assert_eq!(fields[0].data, UnknownFieldData::Varint(value));
    assert_eq!(
        world.verdict,
        Some(true),
        "the shipped check does not recognise the payload"
    );
}

#[then(expr = "the payload is a keepalive: {word}")]
fn then_verdict(world: &mut StreamKeepaliveWorld, expected: String) {
    assert_eq!(world.verdict.expect("not checked").to_string(), expected);
}

#[given(
    expr = "a server stream that delivers the event {string}, stays silent for {int} ms and delivers the event {string}"
)]
fn given_silent_stream(world: &mut StreamKeepaliveWorld, first: String, millis: u32, last: String) {
    world.script.push(Ok(data_event(&first)));
    silence(world, u128::from(millis));
    world.script.push(Ok(data_event(&last)));
}

#[given(
    expr = "a server stream that delivers the event {string}, stays silent for {int} ms and fails with {string}"
)]
fn given_failing_stream(
    world: &mut StreamKeepaliveWorld,
    first: String,
    millis: u32,
    failure: String,
) {
    world.script.push(Ok(data_event(&first)));
    silence(world, u128::from(millis));
    world.script.push(Err(failure));
}

#[given(
    expr = "a server stream that delivers the event {string} also carrying field 19999 as the varint 1"
)]
fn given_event_with_field(world: &mut StreamKeepaliveWorld, id: String) {
    let mut event = data_event(&id);
    event.__buffa_unknown_fields.push(UnknownField {
        number: STREAM_KEEPALIVE_FIELD_NUMBER,
        data: UnknownFieldData::Varint(1),
    });
    world.script.push(Ok(event));
}

#[when("the reader reads the stream")]
async fn when_read(world: &mut StreamKeepaliveWorld) {
    let items = std::mem::take(&mut world.script);
    let reader = api_bones::connect::skip_keepalives(stream::iter(items));
    let delivered: Vec<_> = reader.collect().await;
    world.received = delivered
        .iter()
        .filter_map(|item| item.as_ref().ok())
        .map(|event| match &event.body {
            Some(Body::Id(id)) => id.clone(),
            None => panic!("the reader was handed a message with no event: {event:?}"),
        })
        .collect();
    world.failure = delivered.into_iter().filter_map(Result::err).next_back();
}

#[when("the reader prepares a server-stream request")]
fn when_prepare(world: &mut StreamKeepaliveWorld) {
    world.options = Some(api_bones::connect::with_stream_keepalive(
        CallOptions::default(),
    ));
}

#[then(expr = "the server sent {int} keepalive(s) while the stream was silent")]
fn then_sent(world: &mut StreamKeepaliveWorld, expected: usize) {
    assert_eq!(world.keepalives, expected);
}

#[then(expr = "the reader received the events {string} and nothing else")]
fn then_received(world: &mut StreamKeepaliveWorld, expected: String) {
    assert_eq!(world.received.join(","), expected);
}

#[then(expr = "the reader read the failure {string}")]
fn then_failure(world: &mut StreamKeepaliveWorld, expected: String) {
    assert_eq!(world.failure.as_deref(), Some(expected.as_str()));
}

#[then(expr = "the request carries the header {string} set to {string}")]
fn then_header(world: &mut StreamKeepaliveWorld, name: String, value: String) {
    assert_eq!(name, STREAM_KEEPALIVE_HEADER);
    let options = world.options.as_ref().expect("no request prepared");
    assert_eq!(
        options
            .headers()
            .get(name.as_str())
            .expect("header missing"),
        value.as_str()
    );
}
