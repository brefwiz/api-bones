// SPDX-License-Identifier: MIT
//
//! Which Connect failures may be replayed.
//!
//! [`crate::retry`] answers *how long to wait* between attempts. It does not
//! answer *whether a call may be attempted again at all*, and that half has
//! until now been left to each Rust SDK. Every consumer that wanted a retry
//! therefore wrote its own code list, which is how the set drifts: a client
//! that adds a code here changes replay behaviour for every RPC it makes,
//! without the proto surface having said anything about it.
//!
//! The TypeScript profile (`api-bones-connect-ts/src/retry.ts`) already fixed
//! this policy once. This module is the Rust half of the same decision, so the
//! two adapters agree on what "retryable" means rather than converging by
//! accident.
//!
//! Scope: this is the *transport-shape* half — did the call reach the server.
//! The TypeScript profile additionally gates eligibility on each method's
//! proto-declared `idempotency`, read from a generated policy artifact that has
//! no Rust equivalent yet. Until that artifact exists, a Rust caller still owes
//! its own judgement about whether the specific RPC is safe to replay; these
//! predicates only tell it whether the failure looked like one that never ran.

use connectrpc::{ConnectError, ErrorCode};
use core::time::Duration;
use std::time::SystemTime;

use super::Idempotency;
use crate::retry::{RetryAfter, RetryPolicy};

/// Repeats permitted after an idempotent read's first attempt.
///
/// This is the budget for blind retries only. A throttle is retried whatever
/// this returns, up to three repeats.
#[must_use]
pub const fn retries_for(idempotency: Idempotency) -> u8 {
    match idempotency {
        Idempotency::NoSideEffects | Idempotency::Idempotent => 2,
        Idempotency::NonIdempotent | Idempotency::Unspecified => 0,
    }
}

/// Repeats permitted after a throttle, whatever the method's idempotency.
///
/// A limiter refuses before the handler runs, so replaying the call cannot
/// repeat a side effect. It draws on the same attempt count as every other
/// retry, so a call is never attempted more than `max(retries, 3) + 1` times.
const THROTTLE_RETRIES: u8 = 3;

/// The longest server-named wait a call sits out. A longer retry-after
/// returns the refusal to the caller, who owns the decision to wait that long.
const MAX_THROTTLE_WAIT: Duration = Duration::from_secs(5);

/// The first backoff step; each later step doubles it.
const BACKOFF_BASE: Duration = Duration::from_millis(50);

/// Run a Connect fetch through the retry policy.
///
/// Two kinds of failure are retried, on one attempt count:
///
/// - a throttle (`ResourceExhausted` that names a `retry-after`) while the
///   attempt count is below three, whatever the method's idempotency, and
/// - an `Unavailable` while the attempt count is below `retries`.
///
/// Each wait is the server's retry-after (none for `Unavailable`) plus a
/// jittered exponential backoff from [`RetryPolicy::next_delay_jittered`],
/// slept with [`api_bones_rt::sleep`]. A wait that reaches the remaining
/// `deadline` ends the call as `DeadlineExceeded` without sleeping, and a
/// retry-after longer than five seconds returns the refusal unchanged.
///
/// The fetch callback must check any captured identity before every send.
///
/// # Errors
/// Returns the final fetch failure, any non-retryable failure immediately, or
/// `DeadlineExceeded` when the next wait would pass `deadline`.
pub async fn fetch_with_retry<Resp, F, Fut>(
    mut fetch: F,
    retries: u8,
    deadline: Option<api_bones_rt::Instant>,
) -> Result<Resp, ConnectError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<Resp, ConnectError>>,
{
    let policy = RetryPolicy::exponential(u32::from(retries.max(THROTTLE_RETRIES)), BACKOFF_BASE);
    let mut attempt = 0_u8;
    loop {
        let error = match fetch().await {
            Ok(response) => return Ok(response),
            Err(error) => error,
        };
        let server_wait = throttle_wait(&error);
        let eligible = if server_wait.is_some() {
            attempt < THROTTLE_RETRIES
        } else {
            attempt < retries && is_unprompted_retryable(error.code)
        };
        if !eligible {
            return Err(error);
        }
        let server_wait = server_wait.unwrap_or_default();
        let wait = server_wait
            .saturating_add(policy.next_delay_jittered(u32::from(attempt), random_sample()));
        if deadline
            .is_some_and(|at| wait >= at.saturating_duration_since(api_bones_rt::Instant::now()))
        {
            return Err(ConnectError::new(
                ErrorCode::DeadlineExceeded,
                "the retry wait would pass the call deadline",
            ));
        }
        if server_wait > MAX_THROTTLE_WAIT {
            return Err(error);
        }
        api_bones_rt::sleep(wait).await;
        attempt += 1;
    }
}

/// The wait a server named when it throttled a call.
///
/// Only a `ResourceExhausted` that carries a parseable `retry-after` (seconds
/// or an HTTP-date, in the headers or the trailers) is a throttle. Any other
/// refusal names no moment the server will be ready, so it is not replayed.
fn throttle_wait(error: &ConnectError) -> Option<Duration> {
    if error.code != ErrorCode::ResourceExhausted {
        return None;
    }
    let value = error
        .response_headers()
        .get(http::header::RETRY_AFTER)
        .or_else(|| error.trailers().get(http::header::RETRY_AFTER))?;
    match value.to_str().ok()?.parse::<RetryAfter>().ok()? {
        RetryAfter::Delay(delay) => Some(delay),
        RetryAfter::Date(at) => Some(
            SystemTime::from(at)
                .duration_since(api_bones_rt::system_now())
                .unwrap_or_default(),
        ),
    }
}

/// A uniform draw in `[0, 1)` from the platform's secure random source.
fn random_sample() -> f64 {
    let mut bytes = [0_u8; 4];
    api_bones_rt::fill_random(&mut bytes);
    f64::from(u32::from_le_bytes(bytes)) / 4_294_967_296.0
}

/// Failures raised while writing the request onto the connection.
///
/// A keep-alive pool hands back a socket the peer has already closed and the
/// write fails before any byte of the request is delivered. Connect surfaces
/// that as `Internal` — the code it uses for "the transport itself broke" — so
/// a code check alone never sees it and the call fails outright even though
/// nothing ran on the server.
const CONNECTION_WRITE_FAILURE_SIGNATURES: &[&str] = &[
    "write epipe",
    "broken pipe",
    "econnreset",
    "connection reset",
    "socket hang up",
    "http2 stream closed",
    "received goaway",
];

/// Whether a code may be retried with no instruction from the server.
///
/// Only [`ErrorCode::Unavailable`]: it is the one code that reliably means this
/// connection did not carry the call.
///
/// [`ErrorCode::Aborted`] is deliberately absent. It signals a concurrency or
/// transaction conflict, and the correct response is to re-run the enclosing
/// transaction, not to replay one RPC inside it. Treating it as retryable is
/// also how a watch loop that reconnects on "retryable" turns a server that
/// keeps aborting into a spin.
///
/// [`ErrorCode::ResourceExhausted`] is absent too: retrying a quota rejection
/// on our own schedule is how a degraded service becomes a saturated one.
#[must_use]
pub fn is_unprompted_retryable(code: ErrorCode) -> bool {
    matches!(code, ErrorCode::Unavailable)
}

/// Whether a failure happened before the request reached the server.
///
/// Matched on the message because the underlying cause's code does not survive
/// being wrapped into a [`ConnectError`].
#[must_use]
pub fn is_connection_write_failure(error: &ConnectError) -> bool {
    if error.code != ErrorCode::Internal {
        return false;
    }
    let message = error.message.as_deref().unwrap_or_default().to_lowercase();
    CONNECTION_WRITE_FAILURE_SIGNATURES
        .iter()
        .any(|signature| message.contains(signature))
}

/// Whether a failure's *shape* permits a replay.
///
/// A socket that rejected the write demonstrably carried nothing, which is the
/// safest replay there is; `Unavailable` can reach the client after the server
/// accepted the call, which is why callers of non-idempotent RPCs must still
/// decide for themselves (see the module note on the missing policy artifact).
#[must_use]
pub fn is_replayable_transport_failure(error: &ConnectError) -> bool {
    is_unprompted_retryable(error.code) || is_connection_write_failure(error)
}

/// Re-code a failure that never reached the server as `Unavailable`.
///
/// `Internal` asserts the server has a bug; a socket that rejected the write
/// asserts the opposite, that nothing server-side formed an opinion. The
/// difference is what a caller acts on -- stop and investigate, versus the hop
/// failed and may be retried -- and it is what a caller asserting on a refusal
/// sees in place of the refusal.
///
/// Applied at the boundary where the error leaves the transport, AFTER any
/// retry decision: [`is_connection_write_failure`] recognises these by their
/// `Internal` code, so re-coding earlier would make every one of them look
/// unretryable. Anything else is returned untouched.
#[must_use]
pub fn connection_failure_as_unavailable(error: ConnectError) -> ConnectError {
    if !is_connection_write_failure(&error) {
        return error;
    }
    let mut recoded = error;
    recoded.code = ErrorCode::Unavailable;
    recoded
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use api_bones_rt::Instant;
    use http::{HeaderMap, HeaderValue};

    use super::*;

    fn retry_after_headers(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::RETRY_AFTER,
            HeaderValue::from_str(value).expect("a header value"),
        );
        headers
    }

    /// A refusal from a platform limiter that names when it will be ready.
    fn throttled(retry_after: &str) -> ConnectError {
        ConnectError::new(ErrorCode::ResourceExhausted, "limit reached")
            .with_headers(retry_after_headers(retry_after))
    }

    fn unavailable() -> ConnectError {
        ConnectError::new(ErrorCode::Unavailable, "connection refused")
    }

    /// Drives `fetch_with_retry` over scripted outcomes. Returns the result,
    /// how many times the fetch ran and how long the paused clock moved.
    async fn run(
        script: Vec<Result<u32, ConnectError>>,
        retries: u8,
        deadline: Option<Instant>,
    ) -> (Result<u32, ConnectError>, usize, Duration) {
        let queue = Mutex::new(VecDeque::from(script));
        let calls = AtomicUsize::new(0);
        let started = Instant::now();
        let result = fetch_with_retry(
            || {
                calls.fetch_add(1, Ordering::SeqCst);
                let next = queue
                    .lock()
                    .expect("script")
                    .pop_front()
                    .expect("script ran out");
                async move { next }
            },
            retries,
            deadline,
        )
        .await;
        (result, calls.load(Ordering::SeqCst), started.elapsed())
    }

    fn within(elapsed: Duration, low_ms: u64, high_ms: u64) {
        assert!(
            elapsed >= Duration::from_millis(low_ms) && elapsed <= Duration::from_millis(high_ms),
            "{elapsed:?} is outside {low_ms}..={high_ms} ms"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_first_answer_is_returned_without_waiting() {
        let (result, calls, elapsed) = run(vec![Ok(1)], 2, None).await;
        assert_eq!(result.expect("answered"), 1);
        assert_eq!(calls, 1);
        assert_eq!(elapsed, Duration::ZERO);
    }

    #[tokio::test(start_paused = true)]
    async fn unavailable_backs_off_with_jitter_from_the_policy() {
        let (result, calls, elapsed) =
            run(vec![Err(unavailable()), Err(unavailable()), Ok(7)], 2, None).await;
        assert_eq!(result.expect("retried"), 7);
        assert_eq!(calls, 3);
        // 50 ms then 100 ms, each plus up to the same again.
        within(elapsed, 150, 300);
    }

    #[tokio::test(start_paused = true)]
    async fn unavailable_stops_at_the_blind_retry_budget() {
        let script = vec![Err(unavailable()), Err(unavailable()), Err(unavailable())];
        let (result, calls, _) = run(script, 1, None).await;
        assert_eq!(
            result.expect_err("budget spent").code,
            ErrorCode::Unavailable
        );
        assert_eq!(calls, 2);
    }

    #[tokio::test(start_paused = true)]
    async fn unavailable_is_not_retried_with_no_blind_budget() {
        let (result, calls, elapsed) = run(vec![Err(unavailable())], 0, None).await;
        assert_eq!(result.expect_err("no budget").code, ErrorCode::Unavailable);
        assert_eq!(calls, 1);
        assert_eq!(elapsed, Duration::ZERO);
    }

    #[tokio::test(start_paused = true)]
    async fn other_failures_are_returned_at_once() {
        for code in [
            ErrorCode::Internal,
            ErrorCode::Aborted,
            ErrorCode::DeadlineExceeded,
            ErrorCode::ResourceExhausted,
        ] {
            let (result, calls, elapsed) =
                run(vec![Err(ConnectError::new(code, "refused"))], 2, None).await;
            assert_eq!(result.expect_err("not retryable").code, code);
            assert_eq!(calls, 1, "{code:?}");
            assert_eq!(elapsed, Duration::ZERO, "{code:?}");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_throttle_is_retried_with_no_blind_budget_and_waits_its_retry_after() {
        // `retries_for(Unspecified)` is zero: the throttle still replays.
        assert_eq!(retries_for(Idempotency::Unspecified), 0);
        let (result, calls, elapsed) = run(vec![Err(throttled("2")), Ok(9)], 0, None).await;
        assert_eq!(result.expect("replayed"), 9);
        assert_eq!(calls, 2);
        // The server's 2 s plus 50..100 ms of jitter, never sooner.
        within(elapsed, 2050, 2100);
    }

    #[tokio::test(start_paused = true)]
    async fn a_throttle_stops_after_three_repeats() {
        let script = (0..5).map(|_| Err(throttled("1"))).collect();
        let (result, calls, elapsed) = run(script, 0, None).await;
        assert_eq!(
            result.expect_err("repeats spent").code,
            ErrorCode::ResourceExhausted
        );
        assert_eq!(calls, 4);
        // 1 s plus 50, 100 and 200 ms of backoff, each with up to the same
        // again.
        within(elapsed, 3350, 3700);
    }

    #[tokio::test(start_paused = true)]
    async fn blind_retries_and_throttles_draw_on_one_attempt_count() {
        // Three blind retries leave no throttle repeat.
        let script = vec![
            Err(unavailable()),
            Err(unavailable()),
            Err(unavailable()),
            Err(throttled("0")),
            Ok(1),
        ];
        let (result, calls, _) = run(script, 5, None).await;
        assert_eq!(
            result.expect_err("count spent").code,
            ErrorCode::ResourceExhausted
        );
        assert_eq!(calls, 4);

        // A blind retry spent first leaves the throttle its repeats.
        let script = vec![Err(unavailable()), Err(throttled("0")), Ok(2)];
        let (result, calls, _) = run(script, 1, None).await;
        assert_eq!(result.expect("retried twice"), 2);
        assert_eq!(calls, 3);

        // And a throttle spent first leaves the blind retries what remains.
        let script = vec![Err(throttled("0")), Err(unavailable()), Err(unavailable())];
        let (result, calls, _) = run(script, 2, None).await;
        assert_eq!(
            result.expect_err("count spent").code,
            ErrorCode::Unavailable
        );
        assert_eq!(calls, 3);
    }

    #[tokio::test(start_paused = true)]
    async fn a_retry_after_past_five_seconds_returns_the_refusal_unchanged() {
        let (result, calls, elapsed) = run(vec![Err(throttled("6")), Ok(1)], 2, None).await;
        let error = result.expect_err("too long a wait");
        assert_eq!(error.code, ErrorCode::ResourceExhausted);
        assert_eq!(error.message.as_deref(), Some("limit reached"));
        assert_eq!(
            error.response_headers().get(http::header::RETRY_AFTER),
            Some(&HeaderValue::from_static("6"))
        );
        assert_eq!(calls, 1);
        assert_eq!(elapsed, Duration::ZERO);

        // Five seconds exactly is still sat out.
        let (result, calls, elapsed) = run(vec![Err(throttled("5")), Ok(1)], 2, None).await;
        assert_eq!(result.expect("waited"), 1);
        assert_eq!(calls, 2);
        within(elapsed, 5050, 5100);
    }

    #[tokio::test(start_paused = true)]
    async fn a_retry_after_past_the_deadline_ends_the_call_without_waiting() {
        let deadline = Some(Instant::now() + Duration::from_secs(2));
        let (result, calls, elapsed) = run(vec![Err(throttled("3")), Ok(1)], 2, deadline).await;
        assert_eq!(
            result.expect_err("deadline").code,
            ErrorCode::DeadlineExceeded
        );
        assert_eq!(calls, 1);
        assert_eq!(elapsed, Duration::ZERO);
    }

    #[tokio::test(start_paused = true)]
    async fn the_deadline_bounds_every_wait_including_unavailable() {
        let deadline = Some(Instant::now() + Duration::from_millis(10));
        let (result, calls, elapsed) = run(vec![Err(unavailable()), Ok(1)], 2, deadline).await;
        assert_eq!(
            result.expect_err("deadline").code,
            ErrorCode::DeadlineExceeded
        );
        assert_eq!(calls, 1);
        assert_eq!(elapsed, Duration::ZERO);

        // A deadline already reached admits no wait, however short.
        let deadline = Some(Instant::now());
        let (result, calls, _) = run(vec![Err(throttled("0")), Ok(1)], 2, deadline).await;
        assert_eq!(
            result.expect_err("deadline").code,
            ErrorCode::DeadlineExceeded
        );
        assert_eq!(calls, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn the_deadline_is_checked_before_the_longest_wait_is_refused() {
        let deadline = Some(Instant::now() + Duration::from_secs(3));
        let (result, _, _) = run(vec![Err(throttled("10"))], 2, deadline).await;
        assert_eq!(
            result.expect_err("deadline first").code,
            ErrorCode::DeadlineExceeded
        );

        let deadline = Some(Instant::now() + Duration::from_secs(60));
        let (result, calls, _) = run(vec![Err(throttled("10")), Ok(1)], 2, deadline).await;
        assert_eq!(
            result.expect_err("too long a wait").code,
            ErrorCode::ResourceExhausted
        );
        assert_eq!(calls, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn a_wait_inside_the_deadline_is_taken() {
        let deadline = Some(Instant::now() + Duration::from_secs(60));
        let (result, calls, elapsed) = run(vec![Err(throttled("1")), Ok(5)], 0, deadline).await;
        assert_eq!(result.expect("replayed"), 5);
        assert_eq!(calls, 2);
        within(elapsed, 1050, 1100);
    }

    #[tokio::test(start_paused = true)]
    async fn a_refusal_that_names_no_usable_retry_after_is_not_replayed() {
        let bare = ConnectError::new(ErrorCode::ResourceExhausted, "quota");
        let garbled = ConnectError::new(ErrorCode::ResourceExhausted, "quota")
            .with_headers(retry_after_headers("soon"));
        let negative = ConnectError::new(ErrorCode::ResourceExhausted, "quota")
            .with_headers(retry_after_headers("-3"));
        let mut raw = HeaderMap::new();
        raw.insert(
            http::header::RETRY_AFTER,
            HeaderValue::from_bytes(&[0xff]).expect("an opaque header value"),
        );
        let opaque = ConnectError::new(ErrorCode::ResourceExhausted, "quota").with_headers(raw);
        for error in [bare, garbled, negative, opaque] {
            let (result, calls, elapsed) = run(vec![Err(error), Ok(1)], 2, None).await;
            assert_eq!(
                result.expect_err("not a throttle").code,
                ErrorCode::ResourceExhausted
            );
            assert_eq!(calls, 1);
            assert_eq!(elapsed, Duration::ZERO);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn only_a_resource_exhausted_refusal_is_a_throttle() {
        // A retry-after on another code is not waited out: the backoff alone
        // governs an Unavailable.
        let error = ConnectError::new(ErrorCode::Unavailable, "draining")
            .with_headers(retry_after_headers("4"));
        let (result, calls, elapsed) = run(vec![Err(error), Ok(3)], 1, None).await;
        assert_eq!(result.expect("retried"), 3);
        assert_eq!(calls, 2);
        within(elapsed, 50, 100);
    }

    #[tokio::test(start_paused = true)]
    async fn a_retry_after_in_the_trailers_is_honoured() {
        let error = ConnectError::new(ErrorCode::ResourceExhausted, "limit reached")
            .with_trailers(retry_after_headers("1"));
        let (result, calls, elapsed) = run(vec![Err(error), Ok(4)], 0, None).await;
        assert_eq!(result.expect("replayed"), 4);
        assert_eq!(calls, 2);
        within(elapsed, 1050, 1100);
    }

    #[tokio::test(start_paused = true)]
    async fn an_http_date_retry_after_is_measured_from_the_system_clock() {
        let ahead = chrono::DateTime::<chrono::Utc>::from(
            api_bones_rt::system_now() + Duration::from_secs(3),
        )
        .to_rfc2822();
        let (result, calls, elapsed) = run(vec![Err(throttled(&ahead)), Ok(6)], 0, None).await;
        assert_eq!(result.expect("replayed"), 6);
        assert_eq!(calls, 2);
        // The date drops its sub-second part, so up to a second is lost.
        within(elapsed, 1900, 3200);

        // A date already past leaves only the backoff.
        let (result, calls, elapsed) = run(
            vec![Err(throttled("Wed, 21 Oct 2015 07:28:00 GMT")), Ok(8)],
            0,
            None,
        )
        .await;
        assert_eq!(result.expect("replayed"), 8);
        assert_eq!(calls, 2);
        within(elapsed, 50, 100);
    }

    #[test]
    fn the_jitter_sample_is_a_fresh_draw_in_the_unit_range() {
        let draws: Vec<f64> = (0..64).map(|_| random_sample()).collect();
        assert!(draws.iter().all(|draw| (0.0..1.0).contains(draw)));
        assert!(
            draws
                .windows(2)
                .any(|pair| pair[0].to_bits() != pair[1].to_bits())
        );
    }

    #[test]
    fn only_unavailable_retries_without_server_instruction() {
        assert!(is_unprompted_retryable(ErrorCode::Unavailable));
        for code in [
            ErrorCode::Aborted,
            ErrorCode::ResourceExhausted,
            ErrorCode::DeadlineExceeded,
            ErrorCode::Internal,
            ErrorCode::FailedPrecondition,
        ] {
            assert!(!is_unprompted_retryable(code), "{code:?} must not retry");
        }
    }

    #[test]
    fn a_rejected_write_is_replayable_but_a_semantic_internal_is_not() {
        assert!(is_connection_write_failure(&ConnectError::new(
            ErrorCode::Internal,
            "write EPIPE",
        )));
        assert!(is_connection_write_failure(&ConnectError::new(
            ErrorCode::Internal,
            "received GOAWAY from peer",
        )));
        // The server ran and broke: replaying it just breaks it again.
        assert!(!is_connection_write_failure(&ConnectError::new(
            ErrorCode::Internal,
            "invariant failed",
        )));
        // Only Internal carries these signatures; the same words under another
        // code describe a server-side outcome, not a write that never left.
        assert!(!is_connection_write_failure(&ConnectError::new(
            ErrorCode::Unknown,
            "broken pipe",
        )));
    }

    #[test]
    fn aborted_is_never_a_replayable_transport_failure() {
        // The regression this module exists to prevent: a concurrency conflict
        // read as a transport failure, replayed automatically, and — in a watch
        // loop — reconnected without pause.
        assert!(!is_replayable_transport_failure(&ConnectError::new(
            ErrorCode::Aborted,
            "registry changed during reconciliation",
        )));
        assert!(is_replayable_transport_failure(&ConnectError::new(
            ErrorCode::Unavailable,
            "connection refused",
        )));
    }
}
