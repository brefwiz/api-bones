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
//!
//! This crate depends on neither `api-bones` nor `api-bones-connect`, so a
//! consumer on any `api-bones` major can use the codec; `api-bones` re-exports
//! it as `api_bones::connect::stream_control`.

mod control;
mod controlled;
mod driver;
mod rotation;

pub use controlled::StreamControlled;
pub use driver::{OpenRequest, rotating_stream};

pub use control::{
    CONTROL_FIELD_JSON_NAME, MalformedControl, STREAM_CONTROL_FIELD_NUMBER, STREAM_CONTROL_HEADER,
    STREAM_CONTROL_HEADER_VALUE, StreamControl,
};
pub use rotation::{
    ControlSignal, EndCause, FallbackReason, RotationAction, RotationEvent, RotationFrame,
    RotationMachine, StreamRole,
};
