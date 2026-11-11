// SPDX-License-Identifier: MIT
//! The caller's deadline as the request's timeout header states it, and the
//! header rewritten to the time remaining for a replay.
//!
//! Connect states the timeout in `connect-timeout-ms` (whole milliseconds,
//! at most 10 digits); gRPC and gRPC-Web in `grpc-timeout` (at most 8 digits
//! and a unit among `H`, `M`, `S`, `m`, `u`, `n`).

use std::time::{Duration, Instant};

use http::{HeaderMap, HeaderValue};

const CONNECT_TIMEOUT: &str = "connect-timeout-ms";
const GRPC_TIMEOUT: &str = "grpc-timeout";
const CONNECT_MAX_DIGITS: usize = 10;
const GRPC_MAX_VALUE: u128 = 99_999_999;

/// When the caller's budget for a request runs out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Deadline(Instant);

impl Deadline {
    /// The deadline the request's timeout header sets, counted from `sent`,
    /// or `None` when the request states no timeout.
    pub(crate) fn of(headers: &HeaderMap, sent: Instant) -> Option<Self> {
        timeout(headers).map(|budget| Self(sent + budget))
    }

    /// The time left at `now`, or `None` when none is left.
    pub(crate) fn remaining(self, now: Instant) -> Option<Duration> {
        let left = self.0.saturating_duration_since(now);
        (!left.is_zero()).then_some(left)
    }
}

/// The timeout the request's header states.
pub(crate) fn timeout(headers: &HeaderMap) -> Option<Duration> {
    if let Some(value) = headers.get(CONNECT_TIMEOUT) {
        return parse_connect(value.to_str().ok()?);
    }
    parse_grpc(headers.get(GRPC_TIMEOUT)?.to_str().ok()?)
}

/// Rewrites the request's timeout header to `remaining`. Answers `false`,
/// leaving the headers as they were, when `remaining` cannot be stated in the
/// header's unit (under one millisecond for Connect): no time is left to
/// replay in.
pub(crate) fn rewrite(headers: &mut HeaderMap, remaining: Duration) -> bool {
    if headers.contains_key(CONNECT_TIMEOUT) {
        let millis = remaining.as_millis();
        if millis == 0 {
            return false;
        }
        headers.insert(CONNECT_TIMEOUT, HeaderValue::from(saturating_u64(millis)));
        return true;
    }
    if headers.contains_key(GRPC_TIMEOUT) {
        let Some(value) = encode_grpc(remaining) else {
            return false;
        };
        headers.insert(GRPC_TIMEOUT, value);
    }
    true
}

fn parse_connect(text: &str) -> Option<Duration> {
    if text.is_empty()
        || text.len() > CONNECT_MAX_DIGITS
        || !text.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    text.parse().ok().map(Duration::from_millis)
}

fn parse_grpc(text: &str) -> Option<Duration> {
    let unit = text.chars().next_back()?;
    let digits = &text[..text.len() - unit.len_utf8()];
    if digits.is_empty() || digits.len() > 8 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let value: u64 = digits.parse().ok()?;
    match unit {
        'H' => Some(Duration::from_secs(value.saturating_mul(3600))),
        'M' => Some(Duration::from_secs(value.saturating_mul(60))),
        'S' => Some(Duration::from_secs(value)),
        'm' => Some(Duration::from_millis(value)),
        'u' => Some(Duration::from_micros(value)),
        'n' => Some(Duration::from_nanos(value)),
        _ => None,
    }
}

/// `remaining` in the finest `grpc-timeout` unit whose value fits 8 digits,
/// rounded down so the rewritten budget never exceeds the time left.
fn encode_grpc(remaining: Duration) -> Option<HeaderValue> {
    let candidates = [
        (remaining.as_nanos(), 'n'),
        (remaining.as_micros(), 'u'),
        (remaining.as_millis(), 'm'),
        (u128::from(remaining.as_secs()), 'S'),
        (u128::from(remaining.as_secs() / 60), 'M'),
        (u128::from(remaining.as_secs() / 3600), 'H'),
    ];
    let (value, unit) = candidates
        .into_iter()
        .find(|(value, _)| *value <= GRPC_MAX_VALUE)?;
    if value == 0 {
        return None;
    }
    HeaderValue::try_from(format!("{value}{unit}")).ok()
}

fn saturating_u64(value: u128) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(name: &'static str, value: &'static str) -> HeaderMap {
        let mut map = HeaderMap::new();
        map.insert(name, HeaderValue::from_static(value));
        map
    }

    #[test]
    fn reads_the_connect_timeout_in_milliseconds() {
        assert_eq!(
            timeout(&headers(CONNECT_TIMEOUT, "1500")),
            Some(Duration::from_millis(1500))
        );
        assert_eq!(timeout(&headers(CONNECT_TIMEOUT, "12345678901")), None);
        assert_eq!(timeout(&headers(CONNECT_TIMEOUT, "-5")), None);
        assert_eq!(timeout(&HeaderMap::new()), None);
    }

    #[test]
    fn reads_every_grpc_timeout_unit() {
        assert_eq!(
            timeout(&headers(GRPC_TIMEOUT, "2H")),
            Some(Duration::from_secs(7200))
        );
        assert_eq!(
            timeout(&headers(GRPC_TIMEOUT, "3M")),
            Some(Duration::from_secs(180))
        );
        assert_eq!(
            timeout(&headers(GRPC_TIMEOUT, "4S")),
            Some(Duration::from_secs(4))
        );
        assert_eq!(
            timeout(&headers(GRPC_TIMEOUT, "250m")),
            Some(Duration::from_millis(250))
        );
        assert_eq!(
            timeout(&headers(GRPC_TIMEOUT, "7u")),
            Some(Duration::from_micros(7))
        );
        assert_eq!(
            timeout(&headers(GRPC_TIMEOUT, "9n")),
            Some(Duration::from_nanos(9))
        );
        assert_eq!(timeout(&headers(GRPC_TIMEOUT, "123456789m")), None);
        assert_eq!(timeout(&headers(GRPC_TIMEOUT, "5x")), None);
        assert_eq!(timeout(&headers(GRPC_TIMEOUT, "m")), None);
    }

    #[test]
    fn rewrites_the_connect_timeout_to_the_time_left() {
        let mut map = headers(CONNECT_TIMEOUT, "1000");
        assert!(rewrite(&mut map, Duration::from_micros(400_900)));
        assert_eq!(map[CONNECT_TIMEOUT], "400");
        assert!(!rewrite(&mut map, Duration::from_micros(900)));
        assert_eq!(map[CONNECT_TIMEOUT], "400");
    }

    #[test]
    fn rewrites_the_grpc_timeout_in_the_finest_unit_that_fits() {
        let mut map = headers(GRPC_TIMEOUT, "1S");
        assert!(rewrite(&mut map, Duration::from_millis(250)));
        assert_eq!(map[GRPC_TIMEOUT], "250000u");
        assert!(rewrite(&mut map, Duration::from_secs(3600 * 30_000)));
        assert_eq!(map[GRPC_TIMEOUT], "1800000M");
        assert!(!rewrite(&mut map, Duration::ZERO));
    }

    #[test]
    fn a_request_without_a_timeout_header_keeps_its_headers() {
        let mut map = HeaderMap::new();
        assert!(rewrite(&mut map, Duration::from_millis(5)));
        assert!(map.is_empty());
    }

    #[test]
    fn the_deadline_counts_from_the_send() {
        let sent = Instant::now();
        let deadline = Deadline::of(&headers(CONNECT_TIMEOUT, "100"), sent).expect("deadline");
        assert_eq!(deadline.remaining(sent), Some(Duration::from_millis(100)));
        assert_eq!(deadline.remaining(sent + Duration::from_millis(100)), None);
        assert_eq!(Deadline::of(&HeaderMap::new(), sent), None);
    }
}
