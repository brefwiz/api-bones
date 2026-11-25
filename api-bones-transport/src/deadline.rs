// SPDX-License-Identifier: MIT
//! The caller's deadline.
//!
//! [`Deadline`] is the one source of a call's deadline on every target: the
//! caller's call helper puts it in the request's extensions, and the
//! transport states the time remaining in the request's timeout header and
//! gives up with `deadline_exceeded` when it passes.
//!
//! Connect states the timeout in `connect-timeout-ms` (whole milliseconds,
//! at most 10 digits); gRPC and gRPC-Web in `grpc-timeout` (at most 8 digits
//! and a unit among `H`, `M`, `S`, `m`, `u`, `n`). A request that already
//! carries a header keeps its unit; one that carries none gets
//! `connect-timeout-ms`.

use std::future::Future;
use std::time::Duration;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;

use api_bones_rt as rt;
use connectrpc::ConnectError;
use http::{Extensions, HeaderMap, HeaderValue};

const CONNECT_TIMEOUT: &str = "connect-timeout-ms";
const GRPC_TIMEOUT: &str = "grpc-timeout";
#[cfg(not(target_arch = "wasm32"))]
const CONNECT_MAX_DIGITS: usize = 10;
const GRPC_MAX_VALUE: u128 = 99_999_999;

/// When a call's budget runs out, as a request extension.
///
/// The call helper sets it from the method's default budget and wraps the
/// whole call, retries included, in [`rt::timeout`]. [`HttpTransport`] reads
/// it on every target: it states the time remaining in the request's timeout
/// header, and when the instant passes before the response head arrives it
/// stops the request and answers `deadline_exceeded`.
///
/// [`HttpTransport`]: crate::HttpTransport
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Deadline(pub rt::Instant);

impl Deadline {
    /// The time left now, or `None` when none is left.
    #[must_use]
    pub fn remaining(self) -> Option<Duration> {
        let left = self.0.saturating_duration_since(rt::Instant::now());
        (!left.is_zero()).then_some(left)
    }
}

/// Applies the request's [`Deadline`] extension, if it has one: states the
/// time remaining in the timeout header and answers how long the transport
/// may wait. `None` means the request carries no deadline.
///
/// # Errors
///
/// `deadline_exceeded` when no whole millisecond is left, so the request is
/// never sent.
pub(crate) fn bound(
    headers: &mut HeaderMap,
    extensions: &Extensions,
) -> Result<Option<Duration>, ConnectError> {
    let Some(deadline) = extensions.get::<Deadline>() else {
        return Ok(None);
    };
    let expired =
        || ConnectError::deadline_exceeded("the call's deadline passed before it was sent");
    let remaining = deadline.remaining().ok_or_else(expired)?;
    if !stamp(headers, remaining) {
        return Err(expired());
    }
    Ok(Some(remaining))
}

/// Runs the request's exchange up to the response head for at most
/// `remaining`, as [`bound`] answered it; dropping `attempt` stops the
/// request. `None` waits as long as `attempt` takes.
///
/// This bounds one transport exchange only. The call helper bounds the whole
/// call, retries included.
///
/// # Errors
///
/// `deadline_exceeded` when `remaining` passes first, otherwise whatever
/// `attempt` answers.
pub(crate) async fn within<T>(
    remaining: Option<Duration>,
    attempt: impl Future<Output = Result<T, ConnectError>>,
) -> Result<T, ConnectError> {
    let Some(remaining) = remaining else {
        return attempt.await;
    };
    rt::timeout(remaining, attempt).await.unwrap_or_else(|_| {
        Err(ConnectError::deadline_exceeded(
            "the call's deadline passed before the response arrived",
        ))
    })
}

/// Writes `remaining` to the request's timeout header, `connect-timeout-ms`
/// when the request has none. Answers `false`, leaving the headers as they
/// were, when `remaining` cannot be stated in the header's unit.
fn stamp(headers: &mut HeaderMap, remaining: Duration) -> bool {
    if headers.contains_key(CONNECT_TIMEOUT) || headers.contains_key(GRPC_TIMEOUT) {
        return rewrite(headers, remaining);
    }
    let millis = remaining.as_millis();
    if millis == 0 {
        return false;
    }
    headers.insert(CONNECT_TIMEOUT, HeaderValue::from(saturating_u64(millis)));
    true
}

/// When the caller's budget for a request runs out, as its timeout header
/// states it.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Budget(Instant);

#[cfg(not(target_arch = "wasm32"))]
impl Budget {
    /// The budget the request's timeout header sets, counted from `sent`,
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
#[cfg(not(target_arch = "wasm32"))]
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

#[cfg(not(target_arch = "wasm32"))]
fn parse_connect(text: &str) -> Option<Duration> {
    if text.is_empty()
        || text.len() > CONNECT_MAX_DIGITS
        || !text.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    text.parse().ok().map(Duration::from_millis)
}

#[cfg(not(target_arch = "wasm32"))]
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

#[cfg(all(test, not(target_arch = "wasm32")))]
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
    fn the_budget_counts_from_the_send() {
        let sent = Instant::now();
        let budget = Budget::of(&headers(CONNECT_TIMEOUT, "100"), sent).expect("budget");
        assert_eq!(budget.remaining(sent), Some(Duration::from_millis(100)));
        assert_eq!(budget.remaining(sent + Duration::from_millis(100)), None);
        assert_eq!(Budget::of(&HeaderMap::new(), sent), None);
    }

    fn with_deadline(after: Duration) -> Extensions {
        let mut extensions = Extensions::new();
        extensions.insert(Deadline(rt::Instant::now() + after));
        extensions
    }

    #[test]
    fn a_request_without_a_deadline_extension_is_left_alone() {
        let mut map = HeaderMap::new();
        assert_eq!(
            bound(&mut map, &Extensions::new()).expect("no deadline"),
            None
        );
        assert!(map.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn the_deadline_states_the_time_remaining_in_the_connect_header() {
        let extensions = with_deadline(Duration::from_millis(1500));
        tokio::time::advance(Duration::from_millis(500)).await;
        let mut map = HeaderMap::new();
        let left = bound(&mut map, &extensions)
            .expect("time is left")
            .expect("a deadline");
        assert_eq!(left, Duration::from_millis(1000));
        assert_eq!(map[CONNECT_TIMEOUT], "1000");
    }

    #[tokio::test(start_paused = true)]
    async fn the_deadline_keeps_the_unit_of_a_grpc_timeout_header() {
        let extensions = with_deadline(Duration::from_millis(250));
        let mut map = headers(GRPC_TIMEOUT, "5S");
        bound(&mut map, &extensions).expect("time is left");
        assert_eq!(map[GRPC_TIMEOUT], "250000u");
        assert!(!map.contains_key(CONNECT_TIMEOUT));
    }

    #[tokio::test(start_paused = true)]
    async fn a_passed_deadline_is_deadline_exceeded() {
        let extensions = with_deadline(Duration::from_millis(100));
        tokio::time::advance(Duration::from_millis(100)).await;
        let mut map = HeaderMap::new();
        let refusal = bound(&mut map, &extensions).expect_err("no time left");
        assert_eq!(refusal.code, connectrpc::ErrorCode::DeadlineExceeded);
        assert!(map.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn under_a_millisecond_left_is_deadline_exceeded() {
        let extensions = with_deadline(Duration::from_micros(900));
        let mut map = HeaderMap::new();
        let refusal = bound(&mut map, &extensions).expect_err("under a millisecond");
        assert_eq!(refusal.code, connectrpc::ErrorCode::DeadlineExceeded);
        assert!(map.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn the_deadline_reports_what_is_left() {
        let deadline = Deadline(rt::Instant::now() + Duration::from_millis(40));
        assert_eq!(deadline.remaining(), Some(Duration::from_millis(40)));
        tokio::time::advance(Duration::from_millis(40)).await;
        assert_eq!(deadline.remaining(), None);
    }

    #[tokio::test(start_paused = true)]
    async fn an_exchange_without_a_time_limit_runs_to_its_answer() {
        let answer = within(None, async {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            Ok(7)
        })
        .await;
        assert_eq!(answer.expect("answered"), 7);
    }

    #[tokio::test(start_paused = true)]
    async fn an_exchange_that_answers_in_time_keeps_its_answer() {
        let answered = within(Some(Duration::from_millis(100)), async { Ok(1) }).await;
        assert_eq!(answered.expect("answered"), 1);
        let refused = within(Some(Duration::from_millis(100)), async {
            Err::<(), _>(ConnectError::not_found("nothing here"))
        })
        .await
        .expect_err("refused");
        assert_eq!(refused.code, connectrpc::ErrorCode::NotFound);
    }

    #[tokio::test(start_paused = true)]
    async fn an_exchange_still_waiting_at_the_deadline_is_deadline_exceeded() {
        let started = tokio::time::Instant::now();
        let refusal = within(Some(Duration::from_millis(100)), async {
            std::future::pending::<()>().await;
            Ok(())
        })
        .await
        .expect_err("the deadline passed");
        assert_eq!(refusal.code, connectrpc::ErrorCode::DeadlineExceeded);
        assert_eq!(started.elapsed(), Duration::from_millis(100));
    }
}
