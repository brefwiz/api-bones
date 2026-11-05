// SPDX-License-Identifier: MIT
//! The server-stream keepalive message.
//!
//! A proxy cuts an upstream read that stays idle, so a server stream with
//! nothing to send is dropped mid-flight. A keepalive keeps the connection
//! warm: an ordinary data envelope whose payload is a protobuf message holding
//! only field [`STREAM_KEEPALIVE_FIELD_NUMBER`] as the varint `1`.
//!
//! Field numbers 19000 through 19999 are reserved by protobuf and no compiler
//! lets a message declare one, so no real response message can carry the
//! field. Decoded as any response type, the payload is an empty message whose
//! only content is that unknown field.
//!
//! A server sends one only after [`STREAM_KEEPALIVE_INTERVAL`] of stream
//! silence, only when the request carried [`STREAM_KEEPALIVE_HEADER`] set to
//! [`STREAM_KEEPALIVE_HEADER_VALUE`], and only on a protobuf codec. A client
//! runtime sends the header on every server-stream call and drops keepalives
//! before the caller sees them.

use std::time::Duration;

use futures_core::Stream;
use futures_util::StreamExt;
use futures_util::future::ready;

/// The request header with which a client declares it drops keepalives.
pub const STREAM_KEEPALIVE_HEADER: &str = "bones-stream-keepalive";

/// The value of [`STREAM_KEEPALIVE_HEADER`] that declares the capability.
pub const STREAM_KEEPALIVE_HEADER_VALUE: &str = "1";

/// The reserved field number the keepalive payload sets.
pub const STREAM_KEEPALIVE_FIELD_NUMBER: u32 = 19_999;

/// How long a stream stays silent before the server sends a keepalive.
pub const STREAM_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(15);

/// The payload of a keepalive: the tag of field 19999 with wire type varint
/// (`19999 << 3` as a varint), then the value `1`.
pub const STREAM_KEEPALIVE_PAYLOAD: [u8; 4] = [0xF8, 0xE1, 0x09, 0x01];

/// Whether `payload` is exactly a keepalive.
#[must_use]
pub fn is_keepalive_payload(payload: &[u8]) -> bool {
    payload == STREAM_KEEPALIVE_PAYLOAD
}

/// Drop every `Ok` item of `stream` for which `is_keepalive` holds, and pass
/// everything else, errors included, through in order.
pub fn skip_keepalives_by<S, M, E, F>(
    stream: S,
    is_keepalive: F,
) -> impl Stream<Item = Result<M, E>>
where
    S: Stream<Item = Result<M, E>>,
    F: Fn(&M) -> bool,
{
    stream.filter(move |item| {
        ready(match item {
            Ok(message) => !is_keepalive(message),
            Err(_) => true,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::stream;

    #[test]
    fn payload_is_field_19999_varint_one() {
        assert_eq!(STREAM_KEEPALIVE_PAYLOAD, [0xF8, 0xE1, 0x09, 0x01]);
        assert_eq!(u64::from(STREAM_KEEPALIVE_FIELD_NUMBER) << 3, 159_992);
    }

    #[test]
    fn payload_bytes_are_the_tag_varint_then_one() {
        let tag = u64::from(STREAM_KEEPALIVE_FIELD_NUMBER) << 3;
        let mut expected = Vec::new();
        let mut rest = tag;
        loop {
            let low = (rest & 0x7F) as u8;
            rest >>= 7;
            if rest == 0 {
                expected.push(low);
                break;
            }
            expected.push(low | 0x80);
        }
        expected.push(1);
        assert_eq!(expected, STREAM_KEEPALIVE_PAYLOAD);
    }

    #[test]
    fn constants_are_pinned() {
        assert_eq!(STREAM_KEEPALIVE_HEADER, "bones-stream-keepalive");
        assert_eq!(STREAM_KEEPALIVE_HEADER_VALUE, "1");
        assert_eq!(STREAM_KEEPALIVE_INTERVAL, Duration::from_secs(15));
    }

    #[test]
    fn only_the_exact_payload_is_a_keepalive() {
        assert!(is_keepalive_payload(&STREAM_KEEPALIVE_PAYLOAD));
        assert!(!is_keepalive_payload(&[]));
        assert!(!is_keepalive_payload(&[0xF8, 0xE1, 0x09]));
        assert!(!is_keepalive_payload(&[0xF8, 0xE1, 0x09, 0x02]));
        assert!(!is_keepalive_payload(&[0xF8, 0xE1, 0x09, 0x01, 0x00]));
        assert!(!is_keepalive_payload(&[0xF8, 0xE1, 0x09, 0x00]));
    }

    #[tokio::test]
    async fn skip_keepalives_by_drops_matches_and_keeps_errors_in_order() {
        let items: Vec<Result<u8, &str>> = vec![Ok(1), Ok(0), Err("boom"), Ok(0), Ok(2)];
        let kept: Vec<_> = skip_keepalives_by(stream::iter(items), |m| *m == 0)
            .collect()
            .await;
        assert_eq!(kept, vec![Ok(1), Err("boom"), Ok(2)]);
    }
}
