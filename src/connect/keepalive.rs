// SPDX-License-Identifier: MIT
//! Reading a server stream that may carry keepalive messages.
//!
//! The constants live in `api-bones-stream`; this module is the part that
//! needs a decoded message type. A keepalive decodes as any response type into
//! a message with no known field set and one unknown field, so it is told
//! apart by re-encoding it.

use buffa::Message;
use connectrpc::client::CallOptions;
use futures_core::Stream;

use super::stream_control::{
    STREAM_KEEPALIVE_HEADER, STREAM_KEEPALIVE_HEADER_VALUE, STREAM_KEEPALIVE_PAYLOAD,
    is_keepalive_payload,
};

/// Whether a decoded message is a keepalive.
///
/// The length is checked before anything is encoded, so an ordinary message is
/// rejected without allocating.
#[must_use]
pub fn is_keepalive_message<M: Message>(message: &M) -> bool {
    message.encoded_len() as usize == STREAM_KEEPALIVE_PAYLOAD.len()
        && is_keepalive_payload(&message.encode_to_vec())
}

/// Skip the keepalive messages of a decoded server stream.
///
/// Errors and every other message pass through in order.
pub fn skip_keepalives<S, M, E>(stream: S) -> impl Stream<Item = Result<M, E>>
where
    S: Stream<Item = Result<M, E>>,
    M: Message,
{
    api_bones_stream::skip_keepalives_by(stream, is_keepalive_message::<M>)
}

/// Add the opt-in header to the options of a server-stream request.
///
/// Without it the server never sends a keepalive.
#[must_use]
pub fn with_stream_keepalive(options: CallOptions) -> CallOptions {
    options.with_header(STREAM_KEEPALIVE_HEADER, STREAM_KEEPALIVE_HEADER_VALUE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use buffa::{UnknownFieldData, UnknownFields};
    use buffa_types::google::protobuf::{Duration, Empty};
    use futures_util::{StreamExt, stream};

    fn keepalive() -> Empty {
        Empty::decode_from_slice(&STREAM_KEEPALIVE_PAYLOAD).expect("keepalive payload decodes")
    }

    fn duration(seconds: i64, nanos: i32) -> Duration {
        Duration {
            seconds,
            nanos,
            ..Default::default()
        }
    }

    #[test]
    fn payload_decodes_as_field_19999_varint_one() {
        let fields =
            UnknownFields::decode_from_slice(&STREAM_KEEPALIVE_PAYLOAD).expect("valid wire data");
        let all: Vec<_> = fields.iter().collect();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].number, 19_999);
        assert_eq!(all[0].data, UnknownFieldData::Varint(1));
    }

    #[test]
    fn keepalive_decoded_as_a_response_type_is_recognised() {
        assert!(is_keepalive_message(&keepalive()));
    }

    #[test]
    fn an_empty_message_is_not_a_keepalive() {
        assert!(!is_keepalive_message(&Empty::default()));
    }

    #[test]
    fn an_ordinary_message_with_other_fields_is_kept() {
        assert!(!is_keepalive_message(&duration(30, 1)));
    }

    // Two small ordinary fields encode to the keepalive's length, so only the
    // byte comparison tells them apart.
    #[test]
    fn a_message_of_the_keepalive_length_with_other_bytes_is_kept() {
        let same_length = duration(1, 2);
        assert_eq!(same_length.encoded_len() as usize, 4);
        assert!(!is_keepalive_message(&same_length));
    }

    #[test]
    fn a_message_with_another_unknown_varint_is_kept() {
        let mut other = Empty::default();
        other.__buffa_unknown_fields.push(buffa::UnknownField {
            number: 19_999,
            data: UnknownFieldData::Varint(2),
        });
        assert!(!is_keepalive_message(&other));
    }

    #[tokio::test]
    async fn skip_keepalives_delivers_only_ordinary_messages_and_errors() {
        let items: Vec<Result<Duration, &str>> = vec![
            Ok(duration(5, 0)),
            Ok(Duration::decode_from_slice(&STREAM_KEEPALIVE_PAYLOAD).unwrap()),
            Err("closed"),
            Ok(duration(6, 0)),
        ];
        let seen: Vec<_> = skip_keepalives(stream::iter(items)).collect().await;
        assert_eq!(
            seen,
            vec![Ok(duration(5, 0)), Err("closed"), Ok(duration(6, 0))]
        );
    }

    #[test]
    fn the_stream_request_carries_the_opt_in_header() {
        let options = with_stream_keepalive(CallOptions::default());
        assert_eq!(
            options.headers().get("bones-stream-keepalive").unwrap(),
            "1"
        );
    }
}
