// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! Step-lane crate: the steps live in `tests/`; this target exists so cargo
//! has something to hang them on. It owns the generated stream response types
//! the rotation steps use: `WatchEvent`, declared with `stream_controlled!`,
//! and `PlainEvent`, which carries no stream control.

use std::marker::PhantomData;

use api_bones_stream::{StreamControl, StreamControlled};
use buffa::Message;

/// Rust types generated at build time from `proto/contract/v1/watch.proto`.
#[expect(
    clippy::elidable_lifetime_names,
    clippy::missing_fields_in_debug,
    clippy::pub_underscore_fields,
    reason = "connectrpc and buffa generate this module; a fix here dies at the next codegen run"
)]
mod generated {
    connectrpc::include_generated!();
}

pub use generated::contract::v1::{PlainEvent, WatchEvent, watch_event};

api_bones_stream::stream_controlled!(WatchEvent, bones_control);

/// The compiler's answer to whether a response type opens through
/// [`api_bones_stream::rotating_stream`], which bounds its response type by
/// [`StreamControlled`].
///
/// The inherent `ACCEPTS` exists only where `T` implements that trait, and an
/// inherent constant is found before a trait's. So
/// `RotatingStreamProbe::<T>::ACCEPTS` reads `true` there and falls back to
/// [`Refused::ACCEPTS`], `false`, everywhere else.
pub struct RotatingStreamProbe<T>(PhantomData<T>);

impl<T: StreamControlled> RotatingStreamProbe<T> {
    /// `rotating_stream` accepts `T`.
    pub const ACCEPTS: bool = true;
}

/// The answer for a response type `rotating_stream` does not accept.
pub trait Refused {
    /// `rotating_stream` does not accept the type.
    const ACCEPTS: bool = false;
}

impl<T> Refused for RotatingStreamProbe<T> {}

/// The generated message a server sends for `control` alone: its control-only
/// proto frame, decoded.
///
/// # Panics
/// When the codec's frame is not a valid `WatchEvent`.
#[must_use]
pub fn control_only(control: &StreamControl) -> WatchEvent {
    WatchEvent::decode_from_slice(&control.to_proto_frame()).expect("a control-only frame decodes")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_control_removes_the_control_from_a_generated_message() {
        let drain = StreamControl::Drain { cut_in_ms: 5000 };
        let mut message = control_only(&drain);
        assert!(!message.is_empty());
        assert_eq!(message.take_control(), Ok(Some(drain)));
        assert!(message.is_empty());
        assert_eq!(message.take_control(), Ok(None));
    }

    #[test]
    fn a_generated_message_without_a_control_yields_none() {
        let mut message = WatchEvent::default();
        assert_eq!(message.take_control(), Ok(None));
    }
}
