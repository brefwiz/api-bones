// SPDX-License-Identifier: MIT
//! The stream-control signal and its control-only frame.
//!
//! A long-lived server stream declares `bones.v1.StreamControl bones_control`
//! as field [`STREAM_CONTROL_FIELD_NUMBER`] of every response message, outside
//! any oneof. A control-only frame sets that one field and nothing else, so it
//! can be built for any response message type without knowing the type:
//!
//! - proto: the field's tag, the length of the control, then the control;
//! - JSON: an object whose only member is [`CONTROL_FIELD_JSON_NAME`].
//!
//! Both are message payloads that are never compressed. A client declares
//! that it understands the frame with the request header
//! [`STREAM_CONTROL_HEADER`] set to [`STREAM_CONTROL_HEADER_VALUE`].

use std::fmt;

use serde_json::{Map, Value, json};

/// The request header with which a client declares it reads control frames.
pub const STREAM_CONTROL_HEADER: &str = "bones-stream-control";

/// The value of [`STREAM_CONTROL_HEADER`] that declares the capability.
pub const STREAM_CONTROL_HEADER_VALUE: &str = "1";

/// The field number of `bones_control` in every unbounded-stream response.
pub const STREAM_CONTROL_FIELD_NUMBER: u32 = 2047;

/// The JSON member that carries the control in a JSON frame.
pub const CONTROL_FIELD_JSON_NAME: &str = "bonesControl";

const LENGTH_DELIMITED: u64 = 2;
const VARINT: u64 = 0;
const FIXED64: u64 = 1;
const FIXED32: u64 = 5;

const DRAIN_FIELD: u64 = 1;
const CHECKPOINT_FIELD: u64 = 2;
const CUT_IN_MS_FIELD: u64 = 1;
const DOMAIN_FIELD: u64 = 1;
const LIVE_FROM_FIELD: u64 = 2;
const DELIVERED_THROUGH_FIELD: u64 = 3;
const EPOCH_FIELD: u64 = 4;

/// A frame or its control did not parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MalformedControl;

impl fmt::Display for MalformedControl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("malformed stream control")
    }
}

impl std::error::Error for MalformedControl {}

/// The wire shape of `bones.v1.StreamControl`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamControl {
    /// The stream will be cut `cut_in_ms` milliseconds after the frame is read.
    /// The delay is relative, so clock skew between the peers does not matter.
    Drain {
        /// Milliseconds from receipt until the cut.
        cut_in_ms: u32,
    },
    /// The position the stream has reached in one domain.
    Checkpoint {
        /// The domain the positions belong to.
        domain: String,
        /// The first position a stream opened now would deliver.
        live_from: u64,
        /// The contiguous position this stream has delivered: everything up to
        /// and including it arrived with no gap. Never the highest sequence
        /// seen, which may lie beyond a gap.
        delivered_through: u64,
        /// The producer's sequence epoch. It changes whenever the producer's
        /// positions restart, so positions compare only within one epoch.
        epoch: u64,
    },
}

impl StreamControl {
    /// The control-only proto frame: field 2047 and nothing else.
    #[must_use]
    pub fn to_proto_frame(&self) -> Vec<u8> {
        let body = self.encode_proto();
        let mut out = Vec::new();
        put_varint(
            &mut out,
            (u64::from(STREAM_CONTROL_FIELD_NUMBER) << 3) | LENGTH_DELIMITED,
        );
        put_varint(&mut out, body.len() as u64);
        out.extend_from_slice(&body);
        out
    }

    /// Read the control out of a proto message payload of any type.
    ///
    /// Every other field is skipped, so an ordinary message that also carries
    /// the control reads back the same. `Ok(None)` means no control is present.
    ///
    /// # Errors
    /// [`MalformedControl`] when the payload or the control inside it does not
    /// parse.
    pub fn from_proto_frame(bytes: &[u8]) -> Result<Option<Self>, MalformedControl> {
        let mut found = None;
        for field in Fields(bytes) {
            let (number, value) = field?;
            if number == u64::from(STREAM_CONTROL_FIELD_NUMBER) {
                let FieldValue::Bytes(body) = value else {
                    return Err(MalformedControl);
                };
                found = Some(Self::decode_proto(body)?);
            }
        }
        Ok(found)
    }

    /// The control-only JSON frame: an object whose only member is the control.
    #[must_use]
    pub fn to_json_frame(&self) -> Vec<u8> {
        let mut frame = Map::new();
        frame.insert(CONTROL_FIELD_JSON_NAME.to_owned(), self.to_json());
        Value::Object(frame).to_string().into_bytes()
    }

    /// Read the control out of a JSON message payload of any type. Every other
    /// member is ignored. `Ok(None)` means no control is present.
    ///
    /// # Errors
    /// [`MalformedControl`] when the payload is not a JSON object or the
    /// control inside it does not parse.
    pub fn from_json_frame(bytes: &[u8]) -> Result<Option<Self>, MalformedControl> {
        let parsed: Value = serde_json::from_slice(bytes).map_err(|_| MalformedControl)?;
        let Value::Object(members) = parsed else {
            return Err(MalformedControl);
        };
        match members.get(CONTROL_FIELD_JSON_NAME) {
            None | Some(Value::Null) => Ok(None),
            Some(control) => Self::from_json(control).map(Some),
        }
    }

    fn encode_proto(&self) -> Vec<u8> {
        let (field, inner) = match self {
            Self::Drain { cut_in_ms } => {
                let mut inner = Vec::new();
                if *cut_in_ms != 0 {
                    put_varint(&mut inner, (CUT_IN_MS_FIELD << 3) | VARINT);
                    put_varint(&mut inner, u64::from(*cut_in_ms));
                }
                (DRAIN_FIELD, inner)
            }
            Self::Checkpoint {
                domain,
                live_from,
                delivered_through,
                epoch,
            } => {
                let mut inner = Vec::new();
                if !domain.is_empty() {
                    put_varint(&mut inner, (DOMAIN_FIELD << 3) | LENGTH_DELIMITED);
                    put_varint(&mut inner, domain.len() as u64);
                    inner.extend_from_slice(domain.as_bytes());
                }
                for (number, value) in [
                    (LIVE_FROM_FIELD, *live_from),
                    (DELIVERED_THROUGH_FIELD, *delivered_through),
                    (EPOCH_FIELD, *epoch),
                ] {
                    if value != 0 {
                        put_varint(&mut inner, (number << 3) | VARINT);
                        put_varint(&mut inner, value);
                    }
                }
                (CHECKPOINT_FIELD, inner)
            }
        };
        let mut out = Vec::new();
        put_varint(&mut out, (field << 3) | LENGTH_DELIMITED);
        put_varint(&mut out, inner.len() as u64);
        out.extend_from_slice(&inner);
        out
    }

    fn decode_proto(bytes: &[u8]) -> Result<Self, MalformedControl> {
        let mut decoded = None;
        for field in Fields(bytes) {
            match field? {
                (DRAIN_FIELD, FieldValue::Bytes(body)) => {
                    let mut cut_in_ms = 0;
                    for inner in Fields(body) {
                        if let (CUT_IN_MS_FIELD, FieldValue::Varint(v)) = inner? {
                            cut_in_ms = u32::try_from(v).map_err(|_| MalformedControl)?;
                        }
                    }
                    decoded = Some(Self::Drain { cut_in_ms });
                }
                (CHECKPOINT_FIELD, FieldValue::Bytes(body)) => {
                    decoded = Some(Self::decode_checkpoint(body)?);
                }
                _ => {}
            }
        }
        decoded.ok_or(MalformedControl)
    }

    fn decode_checkpoint(bytes: &[u8]) -> Result<Self, MalformedControl> {
        let (mut domain, mut live_from, mut delivered_through, mut epoch) =
            (String::new(), 0, 0, 0);
        for field in Fields(bytes) {
            match field? {
                (DOMAIN_FIELD, FieldValue::Bytes(text)) => {
                    domain = String::from_utf8(text.to_vec()).map_err(|_| MalformedControl)?;
                }
                (LIVE_FROM_FIELD, FieldValue::Varint(v)) => live_from = v,
                (DELIVERED_THROUGH_FIELD, FieldValue::Varint(v)) => delivered_through = v,
                (EPOCH_FIELD, FieldValue::Varint(v)) => epoch = v,
                _ => {}
            }
        }
        Ok(Self::Checkpoint {
            domain,
            live_from,
            delivered_through,
            epoch,
        })
    }

    fn to_json(&self) -> Value {
        match self {
            Self::Drain { cut_in_ms } => json!({ "drain": { "cutInMs": cut_in_ms } }),
            Self::Checkpoint {
                domain,
                live_from,
                delivered_through,
                epoch,
            } => json!({ "checkpoint": {
                "domain": domain,
                "liveFrom": live_from.to_string(),
                "deliveredThrough": delivered_through.to_string(),
                "epoch": epoch.to_string(),
            } }),
        }
    }

    fn from_json(control: &Value) -> Result<Self, MalformedControl> {
        let members = control.as_object().ok_or(MalformedControl)?;
        if let Some(drain) = members.get("drain") {
            let cut_in_ms = match drain.get("cutInMs") {
                None => 0,
                Some(v) => json_u64(v)
                    .and_then(|n| u32::try_from(n).ok())
                    .ok_or(MalformedControl)?,
            };
            return Ok(Self::Drain { cut_in_ms });
        }
        let checkpoint = members.get("checkpoint").ok_or(MalformedControl)?;
        let position = |name: &str| match checkpoint.get(name) {
            None => Ok(0),
            Some(v) => json_u64(v).ok_or(MalformedControl),
        };
        let domain = match checkpoint.get("domain") {
            None => String::new(),
            Some(v) => v.as_str().ok_or(MalformedControl)?.to_owned(),
        };
        Ok(Self::Checkpoint {
            domain,
            live_from: position("liveFrom")?,
            delivered_through: position("deliveredThrough")?,
            epoch: position("epoch")?,
        })
    }
}

/// A 64-bit integer as proto3 JSON writes it (a string) or as a bare number.
fn json_u64(value: &Value) -> Option<u64> {
    match value {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

enum FieldValue<'a> {
    Varint(u64),
    Bytes(&'a [u8]),
    Fixed,
}

/// The `(field number, value)` pairs of one proto message, in order.
struct Fields<'a>(&'a [u8]);

impl<'a> Iterator for Fields<'a> {
    type Item = Result<(u64, FieldValue<'a>), MalformedControl>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.0.is_empty() {
            return None;
        }
        let item = self.read();
        if item.is_err() {
            self.0 = &[];
        }
        Some(item)
    }
}

impl<'a> Fields<'a> {
    fn read(&mut self) -> Result<(u64, FieldValue<'a>), MalformedControl> {
        let (tag, rest) = take_varint(self.0).ok_or(MalformedControl)?;
        let (value, rest) = match tag & 0x7 {
            VARINT => {
                let (v, rest) = take_varint(rest).ok_or(MalformedControl)?;
                (FieldValue::Varint(v), rest)
            }
            FIXED64 => (FieldValue::Fixed, rest.get(8..).ok_or(MalformedControl)?),
            FIXED32 => (FieldValue::Fixed, rest.get(4..).ok_or(MalformedControl)?),
            LENGTH_DELIMITED => {
                let (len, rest) = take_varint(rest).ok_or(MalformedControl)?;
                let len = usize::try_from(len).map_err(|_| MalformedControl)?;
                let body = rest.get(..len).ok_or(MalformedControl)?;
                (FieldValue::Bytes(body), &rest[len..])
            }
            _ => return Err(MalformedControl),
        };
        self.0 = rest;
        Ok((tag >> 3, value))
    }
}

fn take_varint(bytes: &[u8]) -> Option<(u64, &[u8])> {
    let mut value = 0u64;
    for (index, byte) in bytes.iter().enumerate().take(10) {
        value |= u64::from(byte & 0x7f) << (7 * index);
        if byte & 0x80 == 0 {
            return Some((value, &bytes[index + 1..]));
        }
    }
    None
}

fn put_varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = u8::try_from(value & 0x7f).unwrap_or_default();
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain() -> StreamControl {
        StreamControl::Drain { cut_in_ms: 30_000 }
    }

    fn checkpoint() -> StreamControl {
        StreamControl::Checkpoint {
            domain: "revocations".into(),
            live_from: 41,
            delivered_through: 1 << 40,
            epoch: 3,
        }
    }

    #[test]
    fn proto_frame_starts_with_the_field_2047_tag() {
        // (2047 << 3) | 2 = 16378, a two-byte varint.
        let frame = drain().to_proto_frame();
        assert_eq!(&frame[..2], &[0xfa, 0x7f]);
    }

    #[test]
    fn proto_round_trips_a_drain() {
        let frame = drain().to_proto_frame();
        assert_eq!(StreamControl::from_proto_frame(&frame), Ok(Some(drain())));
    }

    #[test]
    fn proto_round_trips_a_checkpoint() {
        let frame = checkpoint().to_proto_frame();
        assert_eq!(
            StreamControl::from_proto_frame(&frame),
            Ok(Some(checkpoint()))
        );
    }

    #[test]
    fn proto_round_trips_zero_valued_members() {
        for control in [
            StreamControl::Drain { cut_in_ms: 0 },
            StreamControl::Checkpoint {
                domain: String::new(),
                live_from: 0,
                delivered_through: 0,
                epoch: 0,
            },
        ] {
            let frame = control.to_proto_frame();
            assert_eq!(StreamControl::from_proto_frame(&frame), Ok(Some(control)));
        }
    }

    #[test]
    fn proto_control_is_found_among_ordinary_fields() {
        let mut message = vec![0x0a, 0x01, b'x']; // field 1 string
        message.extend(checkpoint().to_proto_frame());
        message.extend([0x18, 0x05]); // field 3 varint
        message.extend([0x25, 1, 2, 3, 4]); // field 4 fixed32
        message.extend([0x29, 1, 2, 3, 4, 5, 6, 7, 8]); // field 5 fixed64
        assert_eq!(
            StreamControl::from_proto_frame(&message),
            Ok(Some(checkpoint()))
        );
    }

    #[test]
    fn proto_without_a_control_reads_none() {
        assert_eq!(
            StreamControl::from_proto_frame(&[0x0a, 0x01, b'x']),
            Ok(None)
        );
        assert_eq!(StreamControl::from_proto_frame(&[]), Ok(None));
    }

    #[test]
    fn proto_rejects_malformed_frames() {
        let good = drain().to_proto_frame();
        assert_eq!(
            StreamControl::from_proto_frame(&good[..good.len() - 1]),
            Err(MalformedControl),
            "truncated"
        );
        assert_eq!(
            StreamControl::from_proto_frame(&[0x0b]),
            Err(MalformedControl),
            "group wire type"
        );
        let mut empty_control = Vec::new();
        put_varint(
            &mut empty_control,
            (u64::from(STREAM_CONTROL_FIELD_NUMBER) << 3) | LENGTH_DELIMITED,
        );
        empty_control.push(0);
        assert_eq!(
            StreamControl::from_proto_frame(&empty_control),
            Err(MalformedControl),
            "a control that sets no signal"
        );
        let mut as_varint = Vec::new();
        put_varint(&mut as_varint, u64::from(STREAM_CONTROL_FIELD_NUMBER) << 3);
        as_varint.push(1);
        assert_eq!(
            StreamControl::from_proto_frame(&as_varint),
            Err(MalformedControl),
            "the control field with the wrong wire type"
        );
    }

    #[test]
    fn json_frame_has_the_control_as_its_only_member() {
        let frame = drain().to_json_frame();
        let value: Value = serde_json::from_slice(&frame).expect("json");
        assert_eq!(
            value,
            json!({ CONTROL_FIELD_JSON_NAME: { "drain": { "cutInMs": 30_000 } } })
        );
    }

    #[test]
    fn json_round_trips_a_drain() {
        let frame = drain().to_json_frame();
        assert_eq!(StreamControl::from_json_frame(&frame), Ok(Some(drain())));
    }

    #[test]
    fn json_round_trips_a_checkpoint_with_positions_as_strings() {
        let frame = checkpoint().to_json_frame();
        let value: Value = serde_json::from_slice(&frame).expect("json");
        assert_eq!(
            value[CONTROL_FIELD_JSON_NAME]["checkpoint"]["deliveredThrough"],
            json!((1u64 << 40).to_string())
        );
        assert_eq!(
            StreamControl::from_json_frame(&frame),
            Ok(Some(checkpoint()))
        );
    }

    #[test]
    fn json_control_is_found_among_ordinary_members() {
        let message = json!({
            "id": "a",
            CONTROL_FIELD_JSON_NAME: { "checkpoint": {
                "domain": "d", "liveFrom": 3, "deliveredThrough": "7", "epoch": 9
            } },
            "n": 1,
        });
        assert_eq!(
            StreamControl::from_json_frame(message.to_string().as_bytes()),
            Ok(Some(StreamControl::Checkpoint {
                domain: "d".into(),
                live_from: 3,
                delivered_through: 7,
                epoch: 9,
            }))
        );
    }

    #[test]
    fn proto_round_trips_a_non_zero_epoch() {
        let control = StreamControl::Checkpoint {
            domain: "d".into(),
            live_from: 1,
            delivered_through: 0,
            epoch: u64::MAX,
        };
        let frame = control.to_proto_frame();
        assert_eq!(StreamControl::from_proto_frame(&frame), Ok(Some(control)));
    }

    #[test]
    fn proto_checkpoint_without_field_4_has_epoch_zero() {
        // checkpoint { live_from = 5 }, with no epoch field.
        let mut frame = Vec::new();
        put_varint(
            &mut frame,
            (u64::from(STREAM_CONTROL_FIELD_NUMBER) << 3) | LENGTH_DELIMITED,
        );
        frame.extend([4, 0x12, 2, 0x10, 5]);
        assert_eq!(
            StreamControl::from_proto_frame(&frame),
            Ok(Some(StreamControl::Checkpoint {
                domain: String::new(),
                live_from: 5,
                delivered_through: 0,
                epoch: 0,
            }))
        );
    }

    #[test]
    fn json_round_trips_a_non_zero_epoch_as_a_string() {
        let control = StreamControl::Checkpoint {
            domain: "d".into(),
            live_from: 1,
            delivered_through: 0,
            epoch: u64::MAX,
        };
        let frame = control.to_json_frame();
        let value: Value = serde_json::from_slice(&frame).expect("json");
        assert_eq!(
            value[CONTROL_FIELD_JSON_NAME]["checkpoint"]["epoch"],
            json!(u64::MAX.to_string())
        );
        assert_eq!(StreamControl::from_json_frame(&frame), Ok(Some(control)));
    }

    #[test]
    fn json_checkpoint_without_epoch_has_epoch_zero() {
        let message = json!({ CONTROL_FIELD_JSON_NAME: { "checkpoint": { "liveFrom": "5" } } });
        assert_eq!(
            StreamControl::from_json_frame(message.to_string().as_bytes()),
            Ok(Some(StreamControl::Checkpoint {
                domain: String::new(),
                live_from: 5,
                delivered_through: 0,
                epoch: 0,
            }))
        );
    }

    #[test]
    fn json_without_a_control_reads_none() {
        assert_eq!(StreamControl::from_json_frame(br#"{"id":"a"}"#), Ok(None));
        let null = json!({ CONTROL_FIELD_JSON_NAME: null }).to_string();
        assert_eq!(StreamControl::from_json_frame(null.as_bytes()), Ok(None));
    }

    #[test]
    fn json_rejects_malformed_frames() {
        for bad in [
            "not json".to_owned(),
            "[]".to_owned(),
            json!({ CONTROL_FIELD_JSON_NAME: 5 }).to_string(),
            json!({ CONTROL_FIELD_JSON_NAME: {} }).to_string(),
            json!({ CONTROL_FIELD_JSON_NAME: { "drain": { "cutInMs": "soon" } } }).to_string(),
            json!({ CONTROL_FIELD_JSON_NAME: { "drain": { "cutInMs": 4_294_967_296u64 } } })
                .to_string(),
            json!({ CONTROL_FIELD_JSON_NAME: { "checkpoint": { "domain": 1 } } }).to_string(),
            json!({ CONTROL_FIELD_JSON_NAME: { "checkpoint": { "liveFrom": "x" } } }).to_string(),
        ] {
            assert_eq!(
                StreamControl::from_json_frame(bad.as_bytes()),
                Err(MalformedControl),
                "{bad}"
            );
        }
    }

    #[test]
    fn the_header_declares_capability_one() {
        assert_eq!(STREAM_CONTROL_HEADER, "bones-stream-control");
        assert_eq!(STREAM_CONTROL_HEADER_VALUE, "1");
        assert_eq!(STREAM_CONTROL_FIELD_NUMBER, 2047);
        assert_eq!(MalformedControl.to_string(), "malformed stream control");
    }
}
