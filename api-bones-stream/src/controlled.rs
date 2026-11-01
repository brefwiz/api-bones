// SPDX-License-Identifier: MIT
//! The Rust declaration that a response message carries a stream control.

use crate::{MalformedControl, StreamControl};

/// A response message that carries a `bones.v1.StreamControl` as field 2047.
///
/// Implementing this trait is the Rust declaration that the stream can be
/// rotated: only a type that implements it is accepted by
/// [`rotating_stream`](crate::rotating_stream), and only such a stream sends
/// the `bones-stream-control` request header.
///
/// Implement it for a buffa-generated response type with
/// [`stream_controlled!`](crate::stream_controlled).
pub trait StreamControlled: Default + PartialEq {
    /// Remove the control from the message and return it. `Ok(None)` means the
    /// message carried none and is left as it was.
    ///
    /// # Errors
    /// [`MalformedControl`] when the control present does not parse.
    fn take_control(&mut self) -> Result<Option<StreamControl>, MalformedControl>;

    /// True when no field is set: a message that carried only a control is
    /// empty once its control is taken.
    fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Implement [`StreamControlled`] for a buffa-generated response type.
///
/// `$field` names the type's `bones.v1.StreamControl` field (the one numbered
/// 2047, a `MessageField`). The macro expects the types the platform's Rust
/// codegen emits, and calls `::buffa::Message`, so the invoking crate needs
/// `buffa`, which its generated code already depends on.
///
/// ```text
/// api_bones_stream::stream_controlled!(WatchResponse, bones_control);
/// ```
#[macro_export]
macro_rules! stream_controlled {
    ($ty:ty, $field:ident) => {
        impl $crate::StreamControlled for $ty {
            fn take_control(
                &mut self,
            ) -> ::core::result::Result<
                ::core::option::Option<$crate::StreamControl>,
                $crate::MalformedControl,
            > {
                match self.$field.take() {
                    ::core::option::Option::None => {
                        ::core::result::Result::Ok(::core::option::Option::None)
                    }
                    ::core::option::Option::Some(control) => {
                        let bytes = ::buffa::Message::encode_to_vec(&control);
                        $crate::StreamControl::from_control_message(&bytes)
                            .map(::core::option::Option::Some)
                    }
                }
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Default, PartialEq)]
    struct Message {
        id: String,
        control: Option<Vec<u8>>,
    }

    impl StreamControlled for Message {
        fn take_control(&mut self) -> Result<Option<StreamControl>, MalformedControl> {
            self.control
                .take()
                .map(|bytes| StreamControl::from_control_message(&bytes))
                .transpose()
        }
    }

    fn drain_bytes() -> Vec<u8> {
        let frame = StreamControl::Drain { cut_in_ms: 5000 }.to_proto_frame();
        frame[3..].to_vec()
    }

    #[test]
    fn take_control_removes_and_returns_the_control() {
        let mut message = Message {
            id: "a".into(),
            control: Some(drain_bytes()),
        };
        assert_eq!(
            message.take_control(),
            Ok(Some(StreamControl::Drain { cut_in_ms: 5000 }))
        );
        assert_eq!(message.control, None);
        assert_eq!(message.id, "a");
    }

    #[test]
    fn take_control_reads_none_from_a_message_without_one() {
        let mut message = Message {
            id: "a".into(),
            control: None,
        };
        assert_eq!(message.take_control(), Ok(None));
    }

    #[test]
    fn take_control_reports_a_malformed_control() {
        let mut message = Message {
            id: String::new(),
            control: Some(vec![0x0a]),
        };
        assert_eq!(message.take_control(), Err(MalformedControl));
    }

    #[test]
    fn a_message_is_empty_once_a_control_only_message_loses_its_control() {
        let mut message = Message {
            id: String::new(),
            control: Some(drain_bytes()),
        };
        assert!(!message.is_empty());
        let _ = message.take_control();
        assert!(message.is_empty());
    }

    #[test]
    fn a_message_with_data_is_not_empty() {
        let message = Message {
            id: "a".into(),
            control: None,
        };
        assert!(!message.is_empty());
    }
}
