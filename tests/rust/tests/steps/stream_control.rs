// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! The Rust half of `../../../features/connect_stream_control.feature`.
//!
//! Frames the codec writes are decoded against the shipped
//! `bones/v1/stream.proto`; frames the shipped proto writes are read by the
//! codec.

use crate::world::StreamControlWorld;
use api_bones::connect::{
    CONTROL_FIELD_JSON_NAME, MalformedControl, STREAM_CONTROL_FIELD_NUMBER, StreamControl,
};
use cucumber::{given, then, when};
use prost_reflect::prost::Message;
use prost_reflect::{DescriptorPool, DynamicMessage, Kind, Value};

use super::shipped_resolver::{CONTRACT, compiler};

fn pool(world: &StreamControlWorld) -> &DescriptorPool {
    world.pool.as_ref().expect("contract not compiled")
}

#[given("a contract whose stream response carries the stream control")]
fn given_stream_control(world: &mut StreamControlWorld) {
    world.source = "syntax = \"proto3\";\npackage contract.v1;\n\
         import \"bones/v1/stream.proto\";\n\
         message Event { oneof body { string id = 1; } bones.v1.StreamControl bones_control = 2047; }\n"
        .to_owned();
    let mut compiler = compiler(world.source.clone(), api_bones_protos::files());
    compiler
        .open_file(CONTRACT)
        .unwrap_or_else(|e| panic!("contract failed to compile: {e}"));
    world.pool = Some(compiler.descriptor_pool());
}

#[given(expr = "a drain notice cutting in {int} ms")]
fn given_drain(world: &mut StreamControlWorld, cut_in_ms: u32) {
    world.control = Some(StreamControl::Drain { cut_in_ms });
}

#[given(expr = "a checkpoint for domain {string} live from {int} delivered through {int}")]
fn given_checkpoint(
    world: &mut StreamControlWorld,
    domain: String,
    live_from: u64,
    delivered_through: u64,
) {
    world.control = Some(StreamControl::Checkpoint {
        domain,
        live_from,
        delivered_through,
    });
}

#[given(expr = "the response also sets its ordinary field {string} to {string}")]
fn given_ordinary(world: &mut StreamControlWorld, field: String, value: String) {
    assert_eq!(field, "id", "the contract's ordinary field is id");
    let event = pool(world)
        .get_message_by_name("contract.v1.Event")
        .expect("Event");
    let mut ordinary = DynamicMessage::new(event);
    ordinary.set_field_by_name("id", Value::String(value.clone()));
    world.ordinary_proto = ordinary.encode_to_vec();
    world.ordinary = Some(value);
}

/// Writes the declared control, and the ordinary field when declared, with the
/// shipped descriptor and the proto3 JSON mapping (lowerCamelCase members, u64
/// as strings), independent of the stream control codec.
#[given(expr = "the shipped proto wrote that message as a {string} frame")]
fn given_shipped_frame(world: &mut StreamControlWorld, codec: String) {
    let control = world.control.clone().expect("no control declared");
    world.json_frame = codec == "json";
    if world.json_frame {
        let body = match control {
            StreamControl::Drain { cut_in_ms } => {
                serde_json::json!({ "drain": { "cutInMs": cut_in_ms } })
            }
            StreamControl::Checkpoint {
                domain,
                live_from,
                delivered_through,
            } => serde_json::json!({ "checkpoint": {
                "domain": domain,
                "liveFrom": live_from.to_string(),
                "deliveredThrough": delivered_through.to_string(),
            } }),
        };
        let mut message = serde_json::Map::new();
        if let Some(id) = &world.ordinary {
            message.insert("id".to_owned(), serde_json::json!(id));
        }
        message.insert(CONTROL_FIELD_JSON_NAME.to_owned(), body);
        world.frame = serde_json::Value::Object(message).to_string().into_bytes();
        return;
    }
    let event = pool(world)
        .get_message_by_name("contract.v1.Event")
        .expect("Event");
    let control_field = event
        .get_field(STREAM_CONTROL_FIELD_NUMBER)
        .expect("control field");
    let Kind::Message(control_type) = control_field.kind() else {
        panic!("control field is not a message");
    };
    let nested = |name: &str| match control_type.get_field_by_name(name).expect(name).kind() {
        Kind::Message(m) => m,
        other => panic!("{name} is {other:?}"),
    };
    let mut set = DynamicMessage::new(control_type.clone());
    match control {
        StreamControl::Drain { cut_in_ms } => {
            let mut drain = DynamicMessage::new(nested("drain"));
            drain.set_field_by_name("cut_in_ms", Value::U32(cut_in_ms));
            set.set_field_by_name("drain", Value::Message(drain));
        }
        StreamControl::Checkpoint {
            domain,
            live_from,
            delivered_through,
        } => {
            let mut checkpoint = DynamicMessage::new(nested("checkpoint"));
            checkpoint.set_field_by_name("domain", Value::String(domain));
            checkpoint.set_field_by_name("live_from", Value::U64(live_from));
            checkpoint.set_field_by_name("delivered_through", Value::U64(delivered_through));
            set.set_field_by_name("checkpoint", Value::Message(checkpoint));
        }
    }
    let mut message = DynamicMessage::new(event);
    if let Some(id) = &world.ordinary {
        message.set_field_by_name("id", Value::String(id.clone()));
    }
    message.set_field(&control_field, Value::Message(set));
    world.frame = message.encode_to_vec();
}

#[given(expr = "a frame with {string}")]
fn given_bad_frame(world: &mut StreamControlWorld, kind: String) {
    let valid = StreamControl::Drain { cut_in_ms: 1 }.to_proto_frame();
    let tag_len = valid.len() - 5;
    world.json_frame = false;
    world.frame = match kind.as_str() {
        "a truncated control" => valid[..valid.len() - 1].to_vec(),
        "a control that sets no signal" => [&valid[..tag_len], &[0]].concat(),
        "the control field as a varint" => {
            let mut frame = valid[..tag_len].to_vec();
            frame[0] &= !0x07;
            frame.push(1);
            frame
        }
        "an ordinary message without a control" => vec![0x0a, 0x01, b'x'],
        "an ordinary JSON message without a control" => {
            world.json_frame = true;
            br#"{"id":"e-1"}"#.to_vec()
        }
        "a JSON control that is null" => {
            world.json_frame = true;
            format!("{{\"{CONTROL_FIELD_JSON_NAME}\":null}}").into_bytes()
        }
        "a JSON control that sets no signal" => {
            world.json_frame = true;
            format!("{{\"{CONTROL_FIELD_JSON_NAME}\":{{}}}}").into_bytes()
        }
        other => panic!("unknown frame kind {other}"),
    };
}

#[when(expr = "the stream control is framed in the {string} codec")]
fn when_framed(world: &mut StreamControlWorld, codec: String) {
    world.frame = frame_control(world, &codec);
    world.json_frame = codec == "json";
}

fn frame_control(world: &StreamControlWorld, codec: &str) -> Vec<u8> {
    let control = world.control.as_ref().expect("no control declared");
    if codec == "json" {
        api_bones::connect::stream_control::StreamControl::to_json_frame(control)
    } else {
        api_bones::connect::stream_control::StreamControl::to_proto_frame(control)
    }
}

#[when("the frame is read by the stream control codec")]
fn when_read(world: &mut StreamControlWorld) {
    world.read_back = Some(read_frame(world));
}

fn read_frame(world: &StreamControlWorld) -> Result<Option<StreamControl>, MalformedControl> {
    if world.json_frame {
        api_bones::connect::stream_control::StreamControl::from_json_frame(&world.frame)
    } else {
        api_bones::connect::stream_control::StreamControl::from_proto_frame(&world.frame)
    }
}

/// Builds a dynamic message from a proto3 JSON object by looking every member
/// up by JSON name in the shipped descriptor, so a member the descriptor does
/// not declare fails the scenario.
fn message_from_json(
    descriptor: &prost_reflect::MessageDescriptor,
    json: &serde_json::Value,
) -> DynamicMessage {
    let mut message = DynamicMessage::new(descriptor.clone());
    for (name, member) in json.as_object().expect("JSON object") {
        let field = descriptor
            .get_field_by_json_name(name)
            .unwrap_or_else(|| panic!("{name} is not a JSON member of the shipped message"));
        let value = match field.kind() {
            Kind::String => Value::String(member.as_str().expect("string").to_owned()),
            Kind::Uint32 => Value::U32(member.as_u64().expect("number").try_into().expect("u32")),
            Kind::Uint64 => Value::U64(match member {
                serde_json::Value::String(text) => text.parse().expect("u64 string"),
                other => other.as_u64().expect("u64 number"),
            }),
            Kind::Message(nested) => Value::Message(message_from_json(&nested, member)),
            other => panic!("unsupported kind {other:?}"),
        };
        message.set_field(&field, value);
    }
    message
}

impl StreamControlWorld {
    /// The framed bytes decoded as the contract's message, by the shipped proto.
    fn decoded_event(&self) -> DynamicMessage {
        decode_event(pool(self), self.json_frame, &self.frame)
    }

    /// The framed control merged with the declared ordinary field, as one
    /// message on the wire: proto fields concatenate, JSON members join.
    fn decoded_event_with_ordinary(&self) -> DynamicMessage {
        let id = self.ordinary.as_ref().expect("no ordinary field declared");
        let combined = if self.json_frame {
            let frame = String::from_utf8(self.frame.clone()).expect("utf8");
            format!("{{\"id\":\"{id}\",{}", &frame[1..]).into_bytes()
        } else {
            [self.ordinary_proto.as_slice(), self.frame.as_slice()].concat()
        };
        decode_event(pool(self), self.json_frame, &combined)
    }

    fn decoded_control(&self) -> DynamicMessage {
        let message = self.decoded_event();
        assert!(message.has_field_by_number(STREAM_CONTROL_FIELD_NUMBER));
        message
            .get_field_by_number(STREAM_CONTROL_FIELD_NUMBER)
            .expect("control is set")
            .as_message()
            .expect("control is a message")
            .clone()
    }
}

fn decode_event(pool: &DescriptorPool, json_frame: bool, frame: &[u8]) -> DynamicMessage {
    let event = pool
        .get_message_by_name("contract.v1.Event")
        .expect("Event");
    if json_frame {
        let json: serde_json::Value = serde_json::from_slice(frame).expect("frame is JSON");
        message_from_json(&event, &json)
    } else {
        DynamicMessage::decode(event, frame).expect("frame decodes")
    }
}

#[then(expr = "the frame decodes against the shipped proto as a drain cutting in {int} ms")]
fn then_decoded_drain(world: &mut StreamControlWorld, cut_in_ms: u32) {
    assert!(!world.frame.is_empty(), "the codec wrote an empty frame");
    let control = world.decoded_control();
    let drain = control.get_field_by_name("drain").expect("drain is set");
    assert!(!control.has_field_by_name("checkpoint"));
    let drain = drain.as_message().expect("drain message");
    let read = drain.get_field_by_name("cut_in_ms").expect("cut_in_ms");
    assert_eq!(read.as_u32(), Some(cut_in_ms));
}

#[then(
    expr = "the frame decodes against the shipped proto as a checkpoint for domain {string} live from {int} delivered through {int}"
)]
fn then_decoded_checkpoint(
    world: &mut StreamControlWorld,
    domain: String,
    live_from: u64,
    delivered_through: u64,
) {
    assert!(!world.frame.is_empty(), "the codec wrote an empty frame");
    let control = world.decoded_control();
    assert!(!control.has_field_by_name("drain"));
    let checkpoint = control
        .get_field_by_name("checkpoint")
        .expect("checkpoint is set");
    let checkpoint = checkpoint.as_message().expect("checkpoint message");
    let field = |name: &str| checkpoint.get_field_by_name(name).expect(name).into_owned();
    assert_eq!(field("domain").as_str(), Some(domain.as_str()));
    assert_eq!(field("live_from").as_u64(), Some(live_from));
    assert_eq!(field("delivered_through").as_u64(), Some(delivered_through));
}

#[then(expr = "the same message reads its ordinary field {string} back as {string}")]
fn then_ordinary_back(world: &mut StreamControlWorld, field: String, expected: String) {
    assert!(!world.frame.is_empty(), "the codec wrote an empty frame");
    let message = world.decoded_event_with_ordinary();
    let read = message.get_field_by_name(&field).expect(&field);
    assert_eq!(read.as_str(), Some(expected.as_str()));
}

#[then(expr = "the JSON frame holds the control under {string} and nothing else")]
fn then_json_member(world: &mut StreamControlWorld, member: String) {
    let text: String = world.frame.iter().map(|b| char::from(*b)).collect();
    assert!(
        text.starts_with(&format!("{{\"{member}\":{{")) && text.ends_with("}}"),
        "{text}"
    );
    assert_eq!(member, CONTROL_FIELD_JSON_NAME);
}

#[then("the stream control read back is the one declared")]
fn then_same_control(world: &mut StreamControlWorld) {
    let outcome = world.read_back.as_ref().expect("frame not read");
    assert_eq!(outcome, &Ok(world.control.clone()));
}

#[then("the codec reads no control")]
fn then_no_control(world: &mut StreamControlWorld) {
    let outcome = world.read_back.as_ref().expect("frame not read");
    assert_eq!(outcome, &Ok(None));
}

#[then("the proto frame sets field 2047 of the shipped message")]
fn then_field_2047(world: &mut StreamControlWorld) {
    let message = world.decoded_event();
    let numbers: Vec<u32> = message.fields().map(|(field, _)| field.number()).collect();
    assert!(
        numbers.contains(&STREAM_CONTROL_FIELD_NUMBER),
        "{numbers:?}"
    );
    assert!(
        numbers
            .iter()
            .all(|n| *n == STREAM_CONTROL_FIELD_NUMBER || *n == 1),
        "{numbers:?}"
    );
}

#[then("the frame is refused as a malformed control")]
fn then_malformed(world: &mut StreamControlWorld) {
    let outcome = world.read_back.as_ref().expect("frame not read");
    assert_eq!(outcome, &Err(MalformedControl));
}
