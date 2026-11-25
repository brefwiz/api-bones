// SPDX-License-Identifier: MIT
//! One client's wire: credential checks, admission, the chunk send and the
//! explicit batch call.

use std::sync::Arc;

use api_bones::connect::{Idempotency, fetch_with_retry, retries_for};
use api_bones_rt::{BoxFuture, Instant};
use connectrpc::ConnectError;
use connectrpc::client::CallOptions;
use tracing::Instrument as _;

use crate::admit::Admission;
use crate::chunk::{self, element_bytes};
use crate::item_error;
use crate::key::{CallHeaders, Credential, CredentialGuard, CredentialSource, QueueKey};
use crate::span::{self, BatchRecord, Observer, Outcome};

/// A boxed future: sendable natively, not `Send` on `wasm32`.
pub type Boxed<T> = BoxFuture<'static, T>;

/// What a batch RPC answered: each requested id with its own outcome, in the
/// order the server returned them.
pub type WireResults<R> = Vec<(String, Result<R, ConnectError>)>;

/// Sends one batch RPC: the template, the ids to put in it, the call options.
#[cfg(not(target_arch = "wasm32"))]
pub type BatchFn<Tmpl, R> = Arc<
    dyn Fn(Tmpl, Vec<String>, CallOptions) -> Boxed<Result<WireResults<R>, ConnectError>>
        + Send
        + Sync,
>;
/// Sends one batch RPC: the template, the ids to put in it, the call options.
#[cfg(target_arch = "wasm32")]
pub type BatchFn<Tmpl, R> =
    Arc<dyn Fn(Tmpl, Vec<String>, CallOptions) -> Boxed<Result<WireResults<R>, ConnectError>>>;

/// Everything one client instance shares across its methods.
pub struct Wire {
    pub(crate) admission: Admission,
    pub(crate) credentials: Arc<dyn CredentialSource>,
    pub(crate) observer: Option<Observer>,
}

/// One batch request about to be sent.
pub(crate) struct ChunkCall<'a> {
    pub guard: &'a CredentialGuard,
    pub queue: &'a QueueKey,
    pub rpc: &'static str,
    pub single: &'static str,
    pub requested: usize,
    pub calls: usize,
    pub chunk_index: usize,
    pub chunks: usize,
    pub bytes: usize,
    pub opened: Instant,
}

/// How an explicit batch is sent.
pub(crate) struct Explicit<'a, Tmpl, R> {
    pub rpc: &'static str,
    pub single: &'static str,
    pub batch: &'a BatchFn<Tmpl, R>,
    pub template_bytes: usize,
}

/// Completes every started wire span exactly once, including future drop.
struct Completion {
    span: tracing::Span,
    observer: Option<Observer>,
    record: BatchRecord,
    finished: bool,
}

impl Completion {
    fn finish(&mut self, item_errors: usize, outcome: Outcome) {
        self.record.item_errors = item_errors;
        self.record.outcome = outcome;
        self.finished = true;
        span::finish(&self.span, self.observer.as_ref(), &self.record);
    }
}

impl Drop for Completion {
    fn drop(&mut self) {
        if !self.finished {
            self.finish(0, Outcome::Cancelled);
        }
    }
}

impl Wire {
    pub(crate) fn new(credentials: Arc<dyn CredentialSource>, observer: Option<Observer>) -> Self {
        Self {
            admission: Admission::default(),
            credentials,
            observer,
        }
    }

    /// The current credential, when its captured identity and required headers remain valid.
    pub(crate) fn verify(&self, guard: &CredentialGuard) -> Result<Credential, ConnectError> {
        let credential = self
            .credentials
            .current()
            .map_err(|_| item_error::identity_changed())?;
        guard.verify(&credential)?;
        Ok(credential)
    }

    /// Send one batch request under a wire slot. The credential is re-read and
    /// re-checked before every send or retry and before the answer is delivered.
    ///
    /// The result holds one outcome per id, in `ids` order; an `Err` fails
    /// every position of the chunk.
    pub(crate) async fn send_chunk<Tmpl: Clone, R>(
        &self,
        call: &ChunkCall<'_>,
        batch: &BatchFn<Tmpl, R>,
        template: Tmpl,
        ids: Vec<String>,
        headers: &CallHeaders,
    ) -> Result<Vec<Result<R, ConnectError>>, ConnectError> {
        let _slot = self.admission.acquire(call.queue).await?;
        let credential = self.verify(call.guard)?;
        headers.validate(&credential.headers)?;
        let begun = record(call, ids.len(), 0, Outcome::Ok);
        let span = span::open(&begun);
        let mut completion = Completion {
            span: span.clone(),
            observer: self.observer.clone(),
            record: begun,
            finished: false,
        };
        let retry_ids = ids.clone();
        // The call's own timeout bounds every retry wait: the batch ends as
        // `deadline_exceeded` rather than sleeping past it.
        let deadline = headers
            .timeout_budget()
            .and_then(|timeout| Instant::now().checked_add(timeout));
        let answer = fetch_with_retry(
            move || {
                let options = self
                    .verify(call.guard)
                    .and_then(|credential| headers.call_options(&credential.headers));
                let template = template.clone();
                let ids = retry_ids.clone();
                async move { batch(template, ids, options?).await }
            },
            retries_for(Idempotency::NoSideEffects),
            deadline,
        )
        .instrument(span.clone())
        .await;
        if self.verify(call.guard).is_err() {
            completion.finish(0, Outcome::Error);
            return Err(item_error::identity_changed());
        }
        let results = match answer {
            Ok(results) => results,
            Err(error) => {
                completion.finish(0, Outcome::Error);
                return Err(error);
            }
        };
        let answers_ids = results.len() == ids.len()
            && results
                .iter()
                .zip(&ids)
                .all(|((answered, _), asked)| answered == asked);
        if !answers_ids {
            completion.finish(0, Outcome::Mismatch);
            return Err(item_error::response_mismatch());
        }
        let outcomes: Vec<Result<R, ConnectError>> =
            results.into_iter().map(|(_, outcome)| outcome).collect();
        let item_errors = outcomes.iter().filter(|o| o.is_err()).count();
        completion.finish(item_errors, Outcome::Ok);
        Ok(outcomes)
    }

    /// An explicit batch: one outcome per input id, in input order.
    ///
    /// Ids are deduplicated, split into chunks and sent one chunk after the
    /// other. The identity is checked before every chunk: once it changed,
    /// every unsent chunk fails without being sent, and answers already
    /// delivered stay.
    pub(crate) async fn run_explicit<Tmpl: Clone, R: Clone>(
        &self,
        explicit: &Explicit<'_, Tmpl, R>,
        template: Tmpl,
        ids: &[String],
        headers: &CallHeaders,
    ) -> (Vec<Result<R, ConnectError>>, Option<CredentialGuard>) {
        if ids.is_empty() {
            return (Vec::new(), None);
        }
        let (distinct, positions) = chunk::dedup(ids);
        let Ok(credential) = self.credentials.current() else {
            return (
                ids.iter()
                    .map(|_| Err(item_error::identity_changed()))
                    .collect(),
                None,
            );
        };
        let guard = match CredentialGuard::capture(&credential) {
            Ok(guard) => guard,
            Err(error) => return (ids.iter().map(|_| Err(error.clone())).collect(), None),
        };
        if let Err(error) = headers.validate(&credential.headers) {
            return (ids.iter().map(|_| Err(error.clone())).collect(), None);
        }
        let queue = QueueKey::of(&credential);
        let plan = chunk::plan(explicit.template_bytes, &distinct);
        let mut slots: Vec<Option<Result<R, ConnectError>>> =
            distinct.iter().map(|_| None).collect();
        for &index in &plan.too_large {
            slots[index] = Some(Err(item_error::item_too_large(distinct[index].len())));
        }
        let opened = Instant::now();
        let chunks = plan.chunks.len();
        let mut stopped: Option<ConnectError> = None;
        for (chunk_index, members) in plan.chunks.iter().enumerate() {
            let outcome = if let Some(error) = &stopped {
                Err(error.clone())
            } else {
                let sent: Vec<String> = members.iter().map(|&i| distinct[i].clone()).collect();
                let bytes = explicit.template_bytes
                    + sent.iter().map(|id| element_bytes(id)).sum::<usize>();
                let call = ChunkCall {
                    guard: &guard,
                    queue: &queue,
                    rpc: explicit.rpc,
                    single: explicit.single,
                    requested: ids.len(),
                    calls: 1,
                    chunk_index,
                    chunks,
                    bytes,
                    opened,
                };
                self.send_chunk(&call, explicit.batch, template.clone(), sent, headers)
                    .await
            };
            match outcome {
                Ok(results) => {
                    for (&index, result) in members.iter().zip(results) {
                        slots[index] = Some(result);
                    }
                }
                Err(error) => {
                    if item_error::is_identity_changed(&error) {
                        stopped = Some(error.clone());
                    }
                    for &index in members {
                        slots[index] = Some(Err(error.clone()));
                    }
                }
            }
        }
        let outcomes = positions
            .iter()
            .map(|&p| {
                slots[p].clone().unwrap_or_else(|| {
                    Err(ConnectError::internal("a batch position was not answered"))
                })
            })
            .collect();
        (outcomes, Some(guard))
    }
}

fn record(call: &ChunkCall<'_>, items: usize, item_errors: usize, outcome: Outcome) -> BatchRecord {
    BatchRecord {
        rpc_method: call.rpc,
        single_method: call.single,
        requested: call.requested,
        items,
        calls: call.calls,
        chunk_index: call.chunk_index,
        chunks: call.chunks,
        bytes: call.bytes,
        wait_ms: u64::try_from(call.opened.elapsed().as_millis()).unwrap_or(u64::MAX),
        item_errors,
        outcome,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use connectrpc::ErrorCode;

    use super::*;
    use crate::key::Anonymous;

    #[tokio::test(start_paused = true)]
    async fn batch_chunk_uses_existing_retry_policy_and_one_span() {
        for (code, expected_attempts) in [(ErrorCode::Unavailable, 2), (ErrorCode::Internal, 1)] {
            let (attempts, batch) = fails_once(ConnectError::new(code, "chunk failed"));
            let records = Arc::new(Mutex::new(Vec::new()));
            let sink = records.clone();
            let wire = Wire::new(
                Arc::new(Anonymous),
                Some(Arc::new(move |record| {
                    sink.lock().expect("records").push(record.clone());
                })),
            );
            let explicit = Explicit {
                rpc: "/contract.Widgets/BatchGetWidgets",
                single: "/contract.Widgets/GetWidget",
                batch: &batch,
                template_bytes: 0,
            };
            let started = Instant::now();
            let (results, _) = wire
                .run_explicit(
                    &explicit,
                    (),
                    &["w1".into(), "w2".into()],
                    &CallHeaders::new(),
                )
                .await;
            assert_eq!(attempts.load(Ordering::SeqCst), expected_attempts);
            let records = records.lock().expect("records");
            assert_eq!(records.len(), 1);
            if code == ErrorCode::Unavailable {
                assert_eq!(
                    results
                        .into_iter()
                        .collect::<Result<Vec<_>, _>>()
                        .expect("retried"),
                    [7, 7]
                );
                // One backoff of 50 ms plus jitter of up to the same again.
                let elapsed = started.elapsed();
                assert!(
                    (Duration::from_millis(50)..Duration::from_millis(150)).contains(&elapsed),
                    "{elapsed:?} is outside the jittered backoff bounds"
                );
                assert_eq!(records[0].outcome, Outcome::Ok);
            } else {
                assert!(
                    results
                        .iter()
                        .all(|result| result.as_ref().expect_err("failed chunk").code == code)
                );
                assert_eq!(started.elapsed(), Duration::ZERO);
                assert_eq!(records[0].outcome, Outcome::Error);
            }
        }
    }

    #[tokio::test(start_paused = true)]
    async fn identity_is_checked_before_a_chunk_retry() {
        let current = Arc::new(Mutex::new(Credential::default()));
        let source = current.clone();
        let wire = Wire::new(
            Arc::new(move || Ok(source.lock().expect("credential").clone())),
            None,
        );
        let attempts = Arc::new(AtomicUsize::new(0));
        let observed = attempts.clone();
        let batch: BatchFn<(), u32> = Arc::new(move |(), _, _| {
            observed.fetch_add(1, Ordering::SeqCst);
            current.lock().expect("credential").identity.subject = "different".into();
            Box::pin(async { Err(ConnectError::unavailable("retry after identity change")) })
        });
        let explicit = Explicit {
            rpc: "/contract.Widgets/BatchGetWidgets",
            single: "/contract.Widgets/GetWidget",
            batch: &batch,
            template_bytes: 0,
        };
        let (results, _) = wire
            .run_explicit(&explicit, (), &["w1".into()], &CallHeaders::new())
            .await;
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        assert!(item_error::is_identity_changed(
            results[0].as_ref().expect_err("identity changed")
        ));
    }

    /// A refusal from a platform limiter that names when it will be ready.
    fn throttled(retry_after: &'static str) -> ConnectError {
        let mut headers = http::HeaderMap::new();
        headers.insert(
            http::header::RETRY_AFTER,
            http::HeaderValue::from_static(retry_after),
        );
        ConnectError::new(ErrorCode::ResourceExhausted, "limit reached").with_headers(headers)
    }

    /// A batch that fails its first call with `failure`, then answers every id
    /// with 7; the counter reports how many calls it received.
    fn fails_once(failure: ConnectError) -> (Arc<AtomicUsize>, BatchFn<(), u32>) {
        let attempts = Arc::new(AtomicUsize::new(0));
        let observed = attempts.clone();
        let batch: BatchFn<(), u32> = Arc::new(move |(), ids, _| {
            let attempt = observed.fetch_add(1, Ordering::SeqCst);
            let failure = failure.clone();
            Box::pin(async move {
                if attempt == 0 {
                    Err(failure)
                } else {
                    Ok(ids.into_iter().map(|id| (id, Ok(7))).collect())
                }
            })
        });
        (attempts, batch)
    }

    /// Sends one id through `batch` under `headers`; returns its outcome and
    /// how far the paused clock moved.
    async fn send_one(
        batch: &BatchFn<(), u32>,
        headers: &CallHeaders,
    ) -> (Result<u32, ConnectError>, Duration) {
        let wire = Wire::new(Arc::new(Anonymous), None);
        let explicit = Explicit {
            rpc: "/contract.Widgets/BatchGetWidgets",
            single: "/contract.Widgets/GetWidget",
            batch,
            template_bytes: 0,
        };
        let started = Instant::now();
        let (mut results, _) = wire
            .run_explicit(&explicit, (), &["w1".into()], headers)
            .await;
        (results.remove(0), started.elapsed())
    }

    #[tokio::test(start_paused = true)]
    async fn a_throttle_is_retried_after_its_retry_after_for_a_batch() {
        let (attempts, batch) = fails_once(throttled("2"));
        let (outcome, elapsed) = send_one(&batch, &CallHeaders::new()).await;
        assert_eq!(outcome.expect("replayed"), 7);
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        assert!(
            (Duration::from_millis(2050)..Duration::from_millis(2100)).contains(&elapsed),
            "{elapsed:?} is outside the retry-after plus jitter bounds"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn the_call_timeout_ends_a_batch_whose_retry_wait_would_pass_it() {
        for (failure, timeout) in [
            (throttled("3"), Duration::from_secs(2)),
            (
                ConnectError::unavailable("draining"),
                Duration::from_millis(10),
            ),
        ] {
            let attempts = Arc::new(AtomicUsize::new(0));
            let observed = attempts.clone();
            let batch: BatchFn<(), u32> = Arc::new(move |(), _, _| {
                observed.fetch_add(1, Ordering::SeqCst);
                let failure = failure.clone();
                Box::pin(async move { Err(failure) })
            });
            let (outcome, elapsed) = send_one(&batch, &CallHeaders::new().timeout(timeout)).await;
            assert_eq!(
                outcome.expect_err("deadline").code,
                ErrorCode::DeadlineExceeded
            );
            assert_eq!(attempts.load(Ordering::SeqCst), 1);
            assert_eq!(elapsed, Duration::ZERO);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_call_timeout_that_covers_the_wait_leaves_the_retry() {
        let (attempts, batch) = fails_once(throttled("1"));
        let headers = CallHeaders::new().timeout(Duration::from_secs(30));
        let (outcome, _) = send_one(&batch, &headers).await;
        assert_eq!(outcome.expect("replayed"), 7);
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }
}
