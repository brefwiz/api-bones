// SPDX-License-Identifier: MIT
//! The wait a refusal asks of its caller.
//!
//! A service that refuses a call because the caller must slow down says how
//! long to wait, and a client reads it back. Both halves are here so the two
//! never drift:
//!
//! - [`retry_after_seconds`] is the one rounding rule: whole seconds, rounded
//!   up, never less than one.
//! - [`with_retry_after`] stamps a refusal with the wait three ways: the
//!   `Retry-After` header, a `google.rpc.RetryInfo` detail, and
//!   `Cache-Control: no-store`, so no cache holds the refusal.
//!   [`resource_exhausted_retry_after`] is that refusal under the
//!   `resource_exhausted` code.
//! - [`retry_after`] reads the wait: the detail first, the header when there is
//!   no detail.
//!
//! The helper chooses neither the code nor the message: the caller passes the
//! error it built and the wait it computed. The message stays free text.
//!
//! `google.rpc.RetryInfo` is hand-encoded, as [`ErrorInfo`](super::ErrorInfo)
//! is, so no proto is vendored. Its `retry_delay` is a
//! `google.protobuf.Duration`; the platform writes whole `seconds` and reads
//! `seconds` and `nanos`.

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};
use connectrpc::{ConnectError, ErrorDetail};
use core::time::Duration;
use http::header::{CACHE_CONTROL, RETRY_AFTER};
use http::{HeaderMap, HeaderValue};

use super::error_info::{Wire, put_len, put_varint, wire_fields};

/// The fully-qualified protobuf name of the detail message.
pub const RETRY_INFO_TYPE: &str = "google.rpc.RetryInfo";

const TYPE_URL_PREFIX: &str = "type.googleapis.com/";

/// The most `RetryInfo` details of one error that are examined.
const MAX_RETRY_INFOS: usize = 4;

/// The whole seconds a wait is sent as: rounded up, and never less than one.
///
/// The one rounding rule of the `Retry-After` header and the `RetryInfo`
/// detail. A caller told to wait zero seconds retries at once, so a refusal
/// that names a wait always names at least one.
#[must_use]
pub fn retry_after_seconds(wait: Duration) -> u64 {
    wait.as_secs()
        .saturating_add(u64::from(wait.subsec_nanos() > 0))
        .max(1)
}

/// `google.rpc.RetryInfo`: how long a caller waits before retrying.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryInfo {
    /// The wait. [`encode`](Self::encode) writes it as whole seconds, rounded
    /// by [`retry_after_seconds`]; [`decode`](Self::decode) reads `seconds`
    /// and `nanos` as sent.
    pub retry_delay: Duration,
}

impl RetryInfo {
    /// A `RetryInfo` that asks for `retry_delay`.
    #[must_use]
    pub const fn new(retry_delay: Duration) -> Self {
        Self { retry_delay }
    }

    /// The protobuf wire bytes: `retry_delay` (1) holding a `Duration` whose
    /// `seconds` (1) is [`retry_after_seconds`] of the wait.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut duration = vec![1 << 3];
        put_varint(&mut duration, retry_after_seconds(self.retry_delay));
        let mut out = Vec::new();
        put_len(&mut out, 1, &duration);
        out
    }

    /// Decode protobuf wire bytes. Unknown fields are skipped. Malformed
    /// input, a negative wait, and a message that names no `retry_delay` yield
    /// `None`.
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let mut retry_delay = None;
        for (field, value) in wire_fields(bytes)? {
            if let (1, Wire::Len(payload)) = (field, value) {
                retry_delay = Some(decode_duration(payload)?);
            }
        }
        retry_delay.map(Self::new)
    }

    /// The Connect error detail carrying this message. Its `debug` form is the
    /// JSON a Connect reader shows, `{"retryDelay":"<seconds>s"}`.
    #[must_use]
    pub fn to_detail(&self) -> ErrorDetail {
        ErrorDetail {
            type_url: RETRY_INFO_TYPE.to_owned(),
            value: Some(STANDARD_NO_PAD.encode(self.encode())),
            debug: Some(serde_json::json!({
                "retryDelay": format!("{}s", retry_after_seconds(self.retry_delay)),
            })),
        }
    }
}

/// A `google.protobuf.Duration`: `seconds` (1) and `nanos` (2), both signed. A
/// negative component is a negative wait, which is no wait to read.
fn decode_duration(bytes: &[u8]) -> Option<Duration> {
    let mut seconds = 0_u64;
    let mut nanos = 0_u32;
    for (field, value) in wire_fields(bytes)? {
        match (field, value) {
            (1, Wire::Varint(raw)) => {
                seconds = u64::try_from(i64::from_ne_bytes(raw.to_ne_bytes())).ok()?;
            }
            (2, Wire::Varint(raw)) => {
                let signed = i32::try_from(i64::from_ne_bytes(raw.to_ne_bytes())).ok()?;
                nanos = u32::try_from(signed)
                    .ok()
                    .filter(|nanos| *nanos < 1_000_000_000)?;
            }
            _ => {}
        }
    }
    Some(Duration::new(seconds, nanos))
}

fn is_retry_info(detail: &ErrorDetail) -> bool {
    detail
        .type_url
        .strip_prefix(TYPE_URL_PREFIX)
        .unwrap_or(&detail.type_url)
        == RETRY_INFO_TYPE
}

/// Stamp `err` with the wait it asks of its caller.
///
/// Sets `Retry-After` to [`retry_after_seconds`] of `wait`, `Cache-Control` to
/// `no-store`, and attaches a `google.rpc.RetryInfo` detail, replacing any
/// already there. The code, the message and every other header and detail of
/// `err` are kept.
#[must_use]
pub fn with_retry_after(mut err: ConnectError, wait: Duration) -> ConnectError {
    let headers = err.response_headers_mut();
    headers.insert(RETRY_AFTER, HeaderValue::from(retry_after_seconds(wait)));
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    err.details.retain(|detail| !is_retry_info(detail));
    err.with_detail(RetryInfo::new(wait).to_detail())
}

/// A `resource_exhausted` refusal with free-text `message` that asks the
/// caller to wait `wait`. See [`with_retry_after`].
///
/// The message names what the caller can act on, never an internal component.
#[must_use]
pub fn resource_exhausted_retry_after(message: impl Into<String>, wait: Duration) -> ConnectError {
    with_retry_after(ConnectError::resource_exhausted(message), wait)
}

/// The first `google.rpc.RetryInfo` detail of a Connect error that decodes.
///
/// Only the first few `RetryInfo` details are examined, and one that does not
/// decode is treated as absent.
#[must_use]
fn retry_info(err: &ConnectError) -> Option<RetryInfo> {
    err.details
        .iter()
        .filter(|detail| is_retry_info(detail))
        .take(MAX_RETRY_INFOS)
        .find_map(|detail| {
            let encoded = detail.value.as_deref()?;
            let bytes = STANDARD_NO_PAD
                .decode(encoded)
                .or_else(|_| STANDARD.decode(encoded))
                .ok()?;
            RetryInfo::decode(&bytes)
        })
}

/// The wait a refusal asks of its caller, if it names one.
///
/// The `google.rpc.RetryInfo` detail wins; with none, the `Retry-After`
/// response header. The header counts only as whole non-negative seconds: an HTTP-date, a negative or fractional number, and an
/// empty value read as no wait.
///
/// The value is the server's claim and is uncapped: callers must cap it before
/// sleeping.
#[must_use]
pub fn retry_after(err: &ConnectError) -> Option<Duration> {
    retry_info(err)
        .map(|info| info.retry_delay)
        .or_else(|| header_wait(err.response_headers()))
}

fn header_wait(headers: &HeaderMap) -> Option<Duration> {
    let value = headers.get(RETRY_AFTER)?.to_str().ok()?.trim();
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse::<u64>().ok().map(Duration::from_secs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use connectrpc::ErrorCode;

    fn secs(seconds: u64) -> Duration {
        Duration::from_secs(seconds)
    }

    fn with_header(value: &str) -> ConnectError {
        let mut err = ConnectError::new(ErrorCode::ResourceExhausted, "slow down");
        err.response_headers_mut().insert(
            RETRY_AFTER,
            HeaderValue::from_str(value).expect("a header value"),
        );
        err
    }

    fn detail_of(info_bytes: &[u8]) -> ErrorDetail {
        ErrorDetail {
            type_url: RETRY_INFO_TYPE.into(),
            value: Some(STANDARD_NO_PAD.encode(info_bytes)),
            debug: None,
        }
    }

    #[test]
    fn rounding_is_ceil_with_a_floor_of_one() {
        assert_eq!(retry_after_seconds(Duration::ZERO), 1);
        assert_eq!(retry_after_seconds(Duration::from_nanos(1)), 1);
        assert_eq!(retry_after_seconds(Duration::from_millis(1)), 1);
        assert_eq!(retry_after_seconds(Duration::from_millis(999)), 1);
        assert_eq!(retry_after_seconds(secs(1)), 1);
        assert_eq!(retry_after_seconds(Duration::from_millis(1200)), 2);
        assert_eq!(retry_after_seconds(Duration::new(2, 1)), 3);
        assert_eq!(retry_after_seconds(secs(7)), 7);
    }

    #[test]
    fn rounding_saturates_instead_of_overflowing() {
        assert_eq!(retry_after_seconds(Duration::MAX), u64::MAX);
    }

    /// The bytes the platform limiter's `retry_info_detail` writes for
    /// the same wait, fixed here so the two encoders cannot drift.
    #[test]
    fn the_wire_matches_the_platform_limiter_byte_for_byte() {
        assert_eq!(RetryInfo::new(secs(7)).encode(), [0x0a, 0x02, 0x08, 0x07]);
        assert_eq!(
            RetryInfo::new(secs(300)).encode(),
            [0x0a, 0x03, 0x08, 0xac, 0x02]
        );
        assert_eq!(
            RetryInfo::new(Duration::from_millis(1200)).encode(),
            [0x0a, 0x02, 0x08, 0x02]
        );
        let detail = RetryInfo::new(secs(7)).to_detail();
        assert_eq!(detail.type_url, "google.rpc.RetryInfo");
        assert_eq!(detail.value.as_deref(), Some("CgIIBw"));
        assert_eq!(detail.debug, Some(serde_json::json!({"retryDelay": "7s"})));
    }

    #[test]
    fn a_sub_second_wait_is_written_as_one_second() {
        let encoded = RetryInfo::new(Duration::ZERO).encode();
        assert_eq!(encoded, [0x0a, 0x02, 0x08, 0x01]);
        assert_eq!(
            RetryInfo::decode(&encoded).map(|info| info.retry_delay),
            Some(secs(1))
        );
    }

    #[test]
    fn the_wire_round_trips_whole_seconds() {
        for seconds in [1, 2, 7, 127, 128, 300, 86_400, u64::from(u32::MAX)] {
            let info = RetryInfo::new(secs(seconds));
            assert_eq!(RetryInfo::decode(&info.encode()), Some(info), "{seconds}");
        }
    }

    #[test]
    fn decode_reads_seconds_and_nanos() {
        // retry_delay { seconds: 2, nanos: 500_000_000 }
        let mut duration = vec![0x08, 0x02, 0x10];
        put_varint(&mut duration, 500_000_000);
        let mut bytes = Vec::new();
        put_len(&mut bytes, 1, &duration);
        assert_eq!(
            RetryInfo::decode(&bytes).map(|info| info.retry_delay),
            Some(Duration::new(2, 500_000_000))
        );
    }

    #[test]
    fn decode_reads_a_duration_with_the_zero_seconds_omitted() {
        assert_eq!(
            RetryInfo::decode(&[0x0a, 0x00]).map(|info| info.retry_delay),
            Some(Duration::ZERO)
        );
    }

    #[test]
    fn decode_skips_unknown_fields() {
        // field 5 (varint) before and after the retry_delay.
        let bytes = [0x28, 0x01, 0x0a, 0x02, 0x08, 0x03, 0x28, 0x01];
        assert_eq!(
            RetryInfo::decode(&bytes).map(|info| info.retry_delay),
            Some(secs(3))
        );
    }

    #[test]
    fn decode_refuses_what_names_no_usable_wait() {
        // No retry_delay at all.
        assert_eq!(RetryInfo::decode(&[]), None);
        assert_eq!(RetryInfo::decode(&[0x28, 0x01]), None);
        // A truncated message, and a truncated length.
        assert_eq!(RetryInfo::decode(&[0x0a, 0x05, 0x08]), None);
        assert_eq!(RetryInfo::decode(&[0x0a]), None);
        // A negative `seconds`: -1 sign-extended to ten bytes.
        let mut negative = vec![0x08];
        put_varint(&mut negative, u64::MAX);
        let mut bytes = Vec::new();
        put_len(&mut bytes, 1, &negative);
        assert_eq!(RetryInfo::decode(&bytes), None);
        // `nanos` out of range, and negative.
        for nanos in [1_000_000_000_u64, u64::MAX] {
            let mut bad = vec![0x08, 0x01, 0x10];
            put_varint(&mut bad, nanos);
            let mut bytes = Vec::new();
            put_len(&mut bytes, 1, &bad);
            assert_eq!(RetryInfo::decode(&bytes), None, "{nanos}");
        }
        // A malformed Duration inside a well-formed message.
        assert_eq!(RetryInfo::decode(&[0x0a, 0x01, 0x08]), None);
    }

    #[test]
    fn with_retry_after_sets_the_header_no_store_and_the_detail() {
        let err = with_retry_after(
            ConnectError::new(ErrorCode::Unavailable, "busy"),
            Duration::from_millis(1200),
        );
        let headers = err.response_headers();
        assert_eq!(headers.get(RETRY_AFTER).expect("retry-after"), "2");
        assert_eq!(
            headers.get(CACHE_CONTROL).expect("cache-control"),
            "no-store"
        );
        assert_eq!(retry_info(&err).map(|info| info.retry_delay), Some(secs(2)));
        assert_eq!(err.details.len(), 1);
        assert_eq!(err.details[0].type_url, RETRY_INFO_TYPE);
    }

    #[test]
    fn with_retry_after_keeps_the_code_the_message_and_other_headers() {
        let mut err = ConnectError::new(ErrorCode::Unavailable, "busy");
        err.response_headers_mut()
            .insert("x-trace", HeaderValue::from_static("abc"));
        let err = with_retry_after(err, secs(3));
        assert_eq!(err.code, ErrorCode::Unavailable);
        assert_eq!(err.message.as_deref(), Some("busy"));
        assert_eq!(err.response_headers().get("x-trace").expect("kept"), "abc");
    }

    #[test]
    fn with_retry_after_replaces_an_earlier_wait() {
        let err = with_retry_after(with_retry_after(with_header("9"), secs(9)), secs(4));
        assert_eq!(err.response_headers().get(RETRY_AFTER).expect("set"), "4");
        assert_eq!(err.details.len(), 1);
        assert_eq!(retry_after(&err), Some(secs(4)));
    }

    #[test]
    fn resource_exhausted_retry_after_chooses_only_the_code() {
        let err = resource_exhausted_retry_after("slow down", secs(5));
        assert_eq!(err.code, ErrorCode::ResourceExhausted);
        assert_eq!(err.message.as_deref(), Some("slow down"));
        assert_eq!(retry_after(&err), Some(secs(5)));
        assert_eq!(err.response_headers().get(RETRY_AFTER).expect("set"), "5");
    }

    #[test]
    fn the_reader_prefers_the_detail_to_the_header() {
        let mut err = with_header("9");
        err = err.with_detail(RetryInfo::new(secs(3)).to_detail());
        assert_eq!(retry_after(&err), Some(secs(3)));
    }

    #[test]
    fn the_reader_falls_back_to_the_header() {
        assert_eq!(retry_after(&with_header("9")), Some(secs(9)));
        assert_eq!(retry_after(&with_header(" 9 ")), Some(secs(9)));
        assert_eq!(retry_after(&with_header("0")), Some(Duration::ZERO));
    }

    #[test]
    fn the_reader_is_none_for_a_value_that_is_not_whole_seconds() {
        for value in [
            "Wed, 21 Oct 2015 07:28:00 GMT",
            "-1",
            "-0",
            "+5",
            "1.5",
            "1e3",
            "",
            "  ",
            "soon",
            "18446744073709551616",
        ] {
            assert_eq!(retry_after(&with_header(value)), None, "{value:?}");
        }
    }

    #[test]
    fn the_reader_is_none_without_a_wait() {
        assert_eq!(
            retry_after(&ConnectError::new(ErrorCode::ResourceExhausted, "x")),
            None
        );
    }

    #[test]
    fn an_undecodable_detail_falls_through_to_the_header() {
        let mut err = with_header("8");
        err = err.with_detail(detail_of(&[0x0a, 0x05, 0x08]));
        err = err.with_detail(ErrorDetail {
            type_url: RETRY_INFO_TYPE.into(),
            value: Some("!!!".into()),
            debug: None,
        });
        err = err.with_detail(ErrorDetail {
            type_url: RETRY_INFO_TYPE.into(),
            value: None,
            debug: None,
        });
        assert_eq!(retry_after(&err), Some(secs(8)));
    }

    #[test]
    fn the_reader_skips_a_bad_detail_for_a_good_one() {
        let err = ConnectError::new(ErrorCode::ResourceExhausted, "x")
            .with_detail(detail_of(&[0x0a, 0x05, 0x08]))
            .with_detail(RetryInfo::new(secs(2)).to_detail());
        assert_eq!(retry_after(&err), Some(secs(2)));
    }

    #[test]
    fn the_reader_accepts_a_prefixed_type_url_and_padded_base64() {
        let detail = ErrorDetail {
            type_url: format!("{TYPE_URL_PREFIX}{RETRY_INFO_TYPE}"),
            value: Some(STANDARD.encode(RetryInfo::new(secs(7)).encode())),
            debug: None,
        };
        let err = ConnectError::new(ErrorCode::ResourceExhausted, "x").with_detail(detail);
        assert_eq!(retry_after(&err), Some(secs(7)));
    }

    #[test]
    fn the_reader_ignores_other_details() {
        let other = ErrorDetail {
            type_url: "bones.v1.ErrorInfo".into(),
            value: Some(STANDARD_NO_PAD.encode(RetryInfo::new(secs(7)).encode())),
            debug: None,
        };
        let err = ConnectError::new(ErrorCode::ResourceExhausted, "x").with_detail(other);
        assert_eq!(retry_after(&err), None);
    }

    #[test]
    fn only_a_bounded_number_of_details_is_examined() {
        let mut err = ConnectError::new(ErrorCode::ResourceExhausted, "x");
        for _ in 0..MAX_RETRY_INFOS {
            err = err.with_detail(detail_of(&[0x0a, 0x05, 0x08]));
        }
        err = err.with_detail(RetryInfo::new(secs(2)).to_detail());
        assert_eq!(retry_after(&err), None);
    }
}
