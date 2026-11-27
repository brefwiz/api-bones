// SPDX-License-Identifier: MIT
//! The call helper on native targets: the call budget, the identity checks,
//! the trace context, the spans and the abort, driven through its public API.
//!
//! The fetch closures here stand in for the generated client's one RPC; the
//! helper's own behaviour (what it hands each send, what it enforces around
//! it) is what is under test. The same helper against a real server is the
//! Gherkin lane of `tests/sdk-call/rust`.

#![cfg(not(target_arch = "wasm32"))]

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use api_bones::connect::{Idempotency, retries_for};
use api_bones_batch::{Credential, CredentialAsk, CredentialSource, Identity};
use api_bones_page::{PageInfo, PageOptions, PagedRequest, PagedResponse, RevisionCache};
use api_bones_rt::BoxFuture;
use api_bones_sdk::call::{
    AbortFlag, Attempt, CALL_BUDGET, CallEnv, CanonicalPagedRequest, ListResponse, PageSend,
};
use api_bones_sdk::{
    CALL_BUDGET_ELAPSED, CANCELED, CallCtx, Error, IDENTITY_CHANGED, SpanEnd, SpanKind, SpanSink,
    SpanStart, SpanStarted, SpanStatus, Traceparent, Tracer,
};
use connectrpc::ConnectError;
use futures_util::stream;

const SERVICE: &str = "acme.v1.Widgets";
const PARENT: &str = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";

/// The credential of `subject`, with a token numbered `token`.
fn credential(subject: &str, token: usize) -> Credential {
    Credential {
        identity: Identity {
            subject: subject.to_owned(),
            org: "org".to_owned(),
        },
        headers: vec![(
            "authorization".to_owned(),
            format!("Bearer {token}").into_bytes(),
        )],
    }
}

/// An env whose identity is `a` for the first `reads` credential reads and
/// `b` after them; every read carries a new token.
fn switching(reads: usize) -> CallEnv {
    let count = Arc::new(AtomicUsize::new(0));
    CallEnv::new(fixed(move || {
        let n = count.fetch_add(1, Ordering::SeqCst);
        Ok(credential(if n < reads { "a" } else { "b" }, n))
    }))
}

/// A credential source that answers what `read` says to every read, whether or
/// not it was asked to refresh.
fn fixed(
    read: impl Fn() -> Result<Credential, ConnectError> + Send + Sync + 'static,
) -> impl CredentialSource {
    move |_: CredentialAsk| {
        let answer = read();
        async move { answer }
    }
}

/// A source that answers `plain` to a read and `renewed` to a read that asks to
/// refresh, counting the refreshes it was asked for.
fn renewing(
    plain: Credential,
    renewed: Result<Credential, ConnectError>,
) -> (impl CredentialSource, Arc<AtomicUsize>) {
    let asked = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&asked);
    let source = move |ask: CredentialAsk| {
        let answer = if ask.refresh {
            count.fetch_add(1, Ordering::SeqCst);
            renewed.clone()
        } else {
            Ok(plain.clone())
        };
        async move { answer }
    };
    (source, asked)
}

fn steady() -> CallEnv {
    switching(usize::MAX)
}

fn header(attempt: &Attempt, name: &str) -> Option<String> {
    attempt
        .headers()
        .iter()
        .find(|(present, _)| present == name)
        .map(|(_, value)| String::from_utf8(value.clone()).expect("text header"))
}

/// What a recording sink saw; every started span is recorded.
#[derive(Default)]
struct Sink {
    starts: Mutex<Vec<SpanStart>>,
    ends: Mutex<Vec<SpanEnd>>,
}

impl SpanSink for Sink {
    fn start(&self, span: &SpanStart) -> SpanStarted {
        let mut starts = self.starts.lock().expect("sink");
        starts.push(span.clone());
        SpanStarted::recording(format!(
            "00-{:032x}-{:016x}-01",
            0xabcd_u128,
            starts.len() as u64
        ))
    }

    fn end(&self, end: &SpanEnd) {
        self.ends.lock().expect("sink").push(end.clone());
    }
}

fn traced(env: CallEnv) -> (CallEnv, Arc<Sink>) {
    let sink = Arc::new(Sink::default());
    (env.with_tracer(Tracer::new(sink.clone())), sink)
}

fn ok(_: Attempt) -> std::future::Ready<Result<u32, ConnectError>> {
    std::future::ready(Ok(1))
}

/// The sends of one answered `GetWidget` call of `env`.
async fn attempts(env: CallEnv) -> Vec<Attempt> {
    let seen = Mutex::new(Vec::new());
    env.call(SERVICE, "GetWidget")
        .unary(Idempotency::NoSideEffects, |attempt| {
            seen.lock().expect("seen").push(attempt);
            std::future::ready(Ok::<_, ConnectError>(1))
        })
        .await
        .expect("answered");
    seen.into_inner().expect("seen")
}

/// The refusal of one `GetWidget` call of `env`, and how many sends it made.
async fn refusal_and_sends(env: CallEnv) -> (Error, usize) {
    let sent = AtomicUsize::new(0);
    let refusal = env
        .call(SERVICE, "GetWidget")
        .unary(Idempotency::NoSideEffects, |_| {
            sent.fetch_add(1, Ordering::SeqCst);
            std::future::ready(Ok::<_, ConnectError>(1))
        })
        .await
        .expect_err("refused");
    (refusal, sent.load(Ordering::SeqCst))
}

#[tokio::test(start_paused = true)]
async fn a_call_with_no_trace_context_sends_a_fresh_valid_traceparent() {
    let seen = attempts(steady()).await;
    let traceparent = header(&seen[0], "traceparent").expect("a traceparent");
    assert!(Traceparent::parse(&traceparent).is_some(), "{traceparent}");
    assert_eq!(header(&seen[0], "tracestate"), None);
    assert_eq!(
        header(&seen[0], "authorization").as_deref(),
        Some("Bearer 1")
    );
}

#[tokio::test(start_paused = true)]
async fn a_call_inside_a_caller_trace_sends_a_child_of_it() {
    let ctx = CallCtx {
        traceparent: Some(PARENT.to_owned()),
        tracestate: Some("vendor=1".to_owned()),
        ..CallCtx::default()
    };
    let seen = CallCtx::scope(ctx, attempts(steady())).await;
    let traceparent = header(&seen[0], "traceparent").expect("a traceparent");
    let (parent, child): (Vec<_>, Vec<_>) = (
        PARENT.split('-').collect(),
        traceparent.split('-').collect(),
    );
    assert_eq!(child[1], parent[1], "same trace");
    assert_ne!(child[2], parent[2], "new span");
    assert_eq!(header(&seen[0], "tracestate").as_deref(), Some("vendor=1"));
}

#[tokio::test(start_paused = true)]
async fn each_send_states_the_call_budget() {
    let left = attempts(steady()).await[0]
        .deadline()
        .expect("a unary send states its deadline")
        .remaining()
        .expect("time left");
    assert!(
        left > CALL_BUDGET.saturating_sub(Duration::from_secs(1)) && left <= CALL_BUDGET,
        "{left:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn a_retried_call_keeps_one_deadline_and_never_gains_time() {
    let seen = Mutex::new(Vec::new());
    let value = steady()
        .call(SERVICE, "GetWidget")
        .unary(Idempotency::Idempotent, |attempt| {
            let mut seen = seen.lock().expect("seen");
            let deadline = attempt.deadline().expect("a unary send has a deadline");
            seen.push((deadline, deadline.remaining()));
            let fail = seen.len() < 3;
            std::future::ready(if fail {
                Err(ConnectError::unavailable("try again"))
            } else {
                Ok(9)
            })
        })
        .await
        .expect("the third attempt answers");
    assert_eq!(value, 9);
    let seen = seen.into_inner().expect("seen");
    assert_eq!(seen.len(), 3);
    assert!(seen.iter().all(|(deadline, _)| *deadline == seen[0].0));
    assert!(seen.windows(2).all(|pair| pair[1].1 <= pair[0].1));
}

#[tokio::test(start_paused = true)]
async fn a_write_is_never_retried() {
    let sent = AtomicUsize::new(0);
    let refusal = steady()
        .call(SERVICE, "CreateWidget")
        .unary(Idempotency::NonIdempotent, |_| {
            sent.fetch_add(1, Ordering::SeqCst);
            std::future::ready(Err::<u32, _>(ConnectError::unavailable("down")))
        })
        .await
        .expect_err("refused");
    assert_eq!(sent.load(Ordering::SeqCst), 1);
    assert_eq!(refusal.platform, "SERVICE_UNAVAILABLE");
}

#[tokio::test(start_paused = true)]
async fn an_identity_that_changed_before_the_send_refuses_the_call_unsent() {
    let (refusal, sent) = refusal_and_sends(switching(1)).await;
    assert_eq!(sent, 0);
    assert_eq!(refusal.code, IDENTITY_CHANGED);
    assert_eq!(refusal.emitter, "bones");
    assert_eq!(refusal.platform, "UNAUTHORIZED");
}

#[tokio::test(start_paused = true)]
async fn an_identity_that_changed_after_the_answer_refuses_its_delivery() {
    let (refusal, sent) = refusal_and_sends(switching(2)).await;
    assert_eq!(sent, 1);
    assert_eq!(refusal.code, IDENTITY_CHANGED);
}

#[tokio::test(start_paused = true)]
async fn a_refreshed_token_of_the_same_identity_is_sent_and_accepted() {
    // Read 0 captured the identity; read 1 is the one this send carries.
    let seen = attempts(steady()).await;
    assert_eq!(
        header(&seen[0], "authorization").as_deref(),
        Some("Bearer 1")
    );
}

#[tokio::test(start_paused = true)]
async fn an_anonymous_call_presents_no_credential() {
    let seen = attempts(CallEnv::anonymous()).await;
    assert_eq!(header(&seen[0], "authorization"), None);
    assert!(header(&seen[0], "traceparent").is_some());
}

#[tokio::test(start_paused = true)]
async fn a_credential_source_that_cannot_answer_fails_the_call_closed() {
    let sent = AtomicUsize::new(0);
    let refusal = CallEnv::new(fixed(|| Err(ConnectError::unavailable("no token"))))
        .call(SERVICE, "GetWidget")
        .unary(Idempotency::NoSideEffects, |_| {
            sent.fetch_add(1, Ordering::SeqCst);
            std::future::ready(Ok::<_, ConnectError>(1))
        })
        .await
        .expect_err("closed");
    assert_eq!(sent.load(Ordering::SeqCst), 0);
    assert_eq!(refusal.platform, "SERVICE_UNAVAILABLE");
}

#[tokio::test(start_paused = true)]
async fn a_call_that_outlasts_the_budget_fails_at_the_budget() {
    let started = tokio::time::Instant::now();
    let refusal = steady()
        .call(SERVICE, "GetWidget")
        .unary(Idempotency::NoSideEffects, |_| {
            std::future::pending::<Result<u32, ConnectError>>()
        })
        .await
        .expect_err("out of time");
    assert_eq!(refusal.code, CALL_BUDGET_ELAPSED);
    assert_eq!(refusal.emitter, "bones");
    assert_eq!(started.elapsed(), CALL_BUDGET);
}

#[tokio::test(start_paused = true)]
async fn the_budget_counts_from_the_start_of_the_call() {
    let started = tokio::time::Instant::now();
    let call = steady().call(SERVICE, "GetWidget");
    tokio::time::sleep(Duration::from_secs(10)).await;
    let refusal = call
        .unary(Idempotency::NoSideEffects, |_| {
            std::future::pending::<Result<u32, ConnectError>>()
        })
        .await
        .expect_err("out of time");
    assert_eq!(refusal.code, CALL_BUDGET_ELAPSED);
    assert_eq!(started.elapsed(), CALL_BUDGET);
}

#[tokio::test(start_paused = true)]
async fn a_server_refusal_as_unauthenticated_tells_the_host_and_drops_the_cache() {
    let told = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&told);
    let env = steady()
        .with_revision_cache(RevisionCache::default())
        .on_unauthenticated(Arc::new(move || {
            seen.fetch_add(1, Ordering::SeqCst);
        }));
    let refusal = env
        .call(SERVICE, "GetWidget")
        .unary(Idempotency::NoSideEffects, |_| {
            std::future::ready(Err::<u32, _>(ConnectError::unauthenticated("expired")))
        })
        .await
        .expect_err("refused");
    assert_eq!(refusal.platform, "UNAUTHORIZED");
    assert_eq!(told.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn a_changed_identity_does_not_tell_the_host_the_server_refused() {
    let told = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&told);
    let env = switching(1)
        .with_revision_cache(RevisionCache::default())
        .on_unauthenticated(Arc::new(move || {
            seen.fetch_add(1, Ordering::SeqCst);
        }));
    env.call(SERVICE, "GetWidget")
        .unary(Idempotency::NoSideEffects, ok)
        .await
        .expect_err("refused");
    assert_eq!(told.load(Ordering::SeqCst), 0);
}

#[tokio::test(start_paused = true)]
async fn a_call_reports_one_client_span_that_ends_with_its_outcome() {
    let (env, sink) = traced(steady());
    let sent = attempts(env).await;
    let starts = sink.starts.lock().expect("sink");
    let ends = sink.ends.lock().expect("sink");
    assert_eq!(starts.len(), 1);
    assert_eq!(starts[0].name, "acme.v1.Widgets/GetWidget");
    assert_eq!(starts[0].kind, SpanKind::Client);
    assert_eq!(starts[0].attributes["rpc.system"], "connect_rpc");
    assert_eq!(starts[0].attributes["rpc.service"], SERVICE);
    assert_eq!(starts[0].attributes["rpc.method"], "GetWidget");
    assert_eq!(ends.len(), 1);
    assert_eq!(ends[0].status, SpanStatus::Ok);
    assert_eq!(
        header(&sent[0], "traceparent").as_deref(),
        Some(ends[0].traceparent.as_str()),
        "the request carries the span the host started"
    );
}

#[tokio::test(start_paused = true)]
async fn a_failed_call_ends_its_span_as_an_error() {
    let (env, sink) = traced(steady());
    env.call(SERVICE, "GetWidget")
        .unary(Idempotency::NoSideEffects, |_| {
            std::future::ready(Err::<u32, _>(ConnectError::not_found("none")))
        })
        .await
        .expect_err("not found");
    let ends = sink.ends.lock().expect("sink");
    assert_eq!(ends.len(), 1);
    assert_eq!(ends[0].status, SpanStatus::Error);
    assert!(ends[0].error.is_some());
}

/// Sets a flag when the future that holds it is dropped.
struct DropFlag(Arc<AtomicBool>);

impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[tokio::test(start_paused = true)]
async fn an_abort_drops_the_call_future_and_ends_its_span_unset() {
    let (env, sink) = traced(steady());
    let flag = AbortFlag::new();
    let dropped = Arc::new(AtomicBool::new(false));
    let guard = Arc::clone(&dropped);
    let call = env
        .call(SERVICE, "GetWidget")
        .with_abort(flag.clone())
        .unary(Idempotency::NoSideEffects, move |_| {
            let held = DropFlag(Arc::clone(&guard));
            async move {
                let _held = held;
                std::future::pending::<Result<u32, ConnectError>>().await
            }
        });
    let (result, ()) = tokio::join!(call, async {
        tokio::task::yield_now().await;
        flag.abort();
    });
    assert_eq!(result.expect_err("canceled").code, CANCELED);
    assert!(dropped.load(Ordering::SeqCst), "the request was dropped");
    assert_eq!(sink.ends.lock().expect("sink")[0].status, SpanStatus::Unset);
}

#[tokio::test(start_paused = true)]
async fn a_call_aborted_before_it_starts_sends_nothing() {
    let flag = AbortFlag::new();
    flag.abort();
    let sent = AtomicUsize::new(0);
    let refusal = steady()
        .call(SERVICE, "GetWidget")
        .with_abort(flag)
        .unary(Idempotency::NoSideEffects, |_| {
            sent.fetch_add(1, Ordering::SeqCst);
            std::future::ready(Ok::<_, ConnectError>(1))
        })
        .await
        .expect_err("canceled");
    assert_eq!(refusal.code, CANCELED);
    assert_eq!(sent.load(Ordering::SeqCst), 0);
}

#[tokio::test(start_paused = true)]
async fn a_stream_is_bounded_only_until_its_response_head() {
    let started = tokio::time::Instant::now();
    let handle = steady()
        .call(SERVICE, "Watch")
        .stream(Idempotency::NoSideEffects, |_| async {
            Ok::<_, ConnectError>(stream::once(async {
                tokio::time::sleep(CALL_BUDGET + Duration::from_secs(15)).await;
                Ok::<u32, ConnectError>(7)
            }))
        })
        .await
        .expect("the head arrives");
    assert_eq!(handle.next().await.expect("an item"), Some(7));
    assert_eq!(handle.next().await.expect("the end"), None);
    assert!(
        started.elapsed() > CALL_BUDGET,
        "the items are not on the budget"
    );
}

#[tokio::test(start_paused = true)]
async fn a_stream_open_states_no_deadline_to_the_server() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    let handle = steady()
        .call(SERVICE, "Watch")
        .stream(Idempotency::NoSideEffects, move |attempt| {
            log.lock().expect("seen").push(attempt.deadline());
            async { Ok::<_, ConnectError>(stream::once(async { Ok::<u32, ConnectError>(7) })) }
        })
        .await
        .expect("the head arrives");
    assert_eq!(handle.next().await.expect("an item"), Some(7));
    assert_eq!(*seen.lock().expect("seen"), vec![None]);
}

#[tokio::test(start_paused = true)]
async fn a_stream_whose_head_never_arrives_fails_at_the_budget() {
    let (env, sink) = traced(steady());
    let refusal =
        env.call(SERVICE, "Watch")
            .stream(Idempotency::NoSideEffects, |_| {
                std::future::pending::<
                    Result<stream::Pending<Result<u32, ConnectError>>, ConnectError>,
                >()
            })
            .await
            .expect_err("no head");
    assert_eq!(refusal.code, CALL_BUDGET_ELAPSED);
    let ends = sink.ends.lock().expect("sink");
    assert!(ends.iter().all(|end| end.status == SpanStatus::Error));
    assert_eq!(ends.len(), 2, "the head span and the walk span both end");
}

#[tokio::test(start_paused = true)]
async fn an_abort_cancels_a_stream_after_its_head() {
    let flag = AbortFlag::new();
    let handle = steady()
        .call(SERVICE, "Watch")
        .with_abort(flag.clone())
        .stream(Idempotency::NoSideEffects, |_| async {
            Ok::<_, ConnectError>(stream::pending::<Result<u32, ConnectError>>())
        })
        .await
        .expect("the head arrives");
    let (item, ()) = tokio::join!(handle.next(), async {
        tokio::task::yield_now().await;
        flag.abort();
    });
    assert_eq!(item.expect_err("canceled").code, CANCELED);
    assert_eq!(handle.next().await.expect("finished"), None);
}

#[tokio::test(start_paused = true)]
async fn a_stream_delivers_its_failures_as_vocabulary_errors() {
    let handle = steady()
        .call(SERVICE, "Watch")
        .stream(Idempotency::NoSideEffects, |_| async {
            Ok::<_, ConnectError>(stream::iter([
                Ok(1_u32),
                Err(ConnectError::not_found("gone")),
            ]))
        })
        .await
        .expect("the head arrives");
    assert_eq!(handle.next().await.expect("an item"), Some(1));
    assert_eq!(
        handle.next().await.expect_err("failed").platform,
        "RESOURCE_NOT_FOUND"
    );
}

#[tokio::test(start_paused = true)]
async fn a_batch_delivers_its_items_in_request_order_as_one_chunk() {
    let handle =
        steady()
            .call(SERVICE, "BatchGetWidgets")
            .batch(Idempotency::NoSideEffects, |_| async {
                Ok::<_, ConnectError>(vec![
                    Ok(1_u32),
                    Err(ConnectError::not_found("missing")),
                    Ok(3),
                ])
            });
    let chunk = handle.next().await.expect("a chunk").expect("not ended");
    assert_eq!(chunk.len(), 3);
    assert_eq!(chunk[0].as_ref().ok(), Some(&1));
    assert_eq!(
        chunk[1].as_ref().expect_err("missing").platform,
        "RESOURCE_NOT_FOUND"
    );
    assert_eq!(chunk[2].as_ref().ok(), Some(&3));
    assert!(handle.next().await.expect("the end").is_none());
}

#[tokio::test(start_paused = true)]
async fn a_batch_keeps_its_own_code_for_a_changed_identity() {
    let handle = switching(2)
        .call(SERVICE, "BatchGetWidgets")
        .batch(Idempotency::NoSideEffects, |_| async {
            Ok::<Vec<Result<u32, ConnectError>>, ConnectError>(vec![Ok(1)])
        });
    let refusal = handle.next().await.expect_err("refused");
    assert_eq!(refusal.code, "BATCH_IDENTITY_CHANGED");
    assert_eq!(refusal.emitter, "bones");
}

#[tokio::test(start_paused = true)]
async fn a_batch_runs_inside_the_whole_call_budget() {
    let handle = steady()
        .call(SERVICE, "BatchGetWidgets")
        .batch(Idempotency::NoSideEffects, |_| {
            std::future::pending::<Result<Vec<Result<u32, ConnectError>>, ConnectError>>()
        });
    let refusal = handle.next().await.expect_err("out of time");
    assert_eq!(refusal.code, CALL_BUDGET_ELAPSED);
}

#[derive(Clone)]
struct PageReq {
    cursor: String,
}

impl PagedRequest for PageReq {
    fn set_page(&mut self, cursor: &str, _page_size: Option<u32>) {
        cursor.clone_into(&mut self.cursor);
    }
}

impl CanonicalPagedRequest for PageReq {
    fn encode(&self) -> Vec<u8> {
        b"widgets".to_vec()
    }
}

struct PageResp {
    items: Vec<u32>,
    next: &'static str,
}

impl PagedResponse for PageResp {
    type Item = u32;

    fn encoded_len(_: &u32) -> usize {
        1
    }

    fn split(self) -> (Vec<u32>, PageInfo) {
        (
            self.items,
            PageInfo {
                next_cursor: self.next.to_owned(),
                revision: "r1".to_owned(),
                total: None,
            },
        )
    }
}

/// The answer to one page of the two-page list: its two pages, or nothing
/// but the server's confirmation that the revision the caller holds is
/// current.
fn answer(request: &PageReq, not_modified: bool) -> ListResponse<PageResp> {
    let body = if not_modified {
        PageResp {
            items: Vec::new(),
            next: "",
        }
    } else if request.cursor.is_empty() {
        PageResp {
            items: vec![1],
            next: "p2",
        }
    } else {
        PageResp {
            items: vec![2],
            next: "",
        }
    };
    ListResponse { body, not_modified }
}

/// A list whose every page takes `each` to arrive, each answered by `reply`.
fn timed_pages(
    each: Duration,
    deadlines: Arc<Mutex<Vec<api_bones_transport::Deadline>>>,
    reply: impl Fn(&PageReq) -> ListResponse<PageResp> + Send + Sync + 'static,
) -> PageSend<PageReq, PageResp> {
    Arc::new(
        move |(request, attempt): (PageReq, Attempt)| -> BoxFuture<'static, _> {
            deadlines
                .lock()
                .expect("deadlines")
                .push(attempt.deadline().expect("a page send has a deadline"));
            let response = reply(&request);
            Box::pin(async move {
                tokio::time::sleep(each).await;
                Ok(response)
            })
        },
    )
}

/// A two-page list whose every page takes `each` to arrive.
fn two_pages(
    each: Duration,
    deadlines: Arc<Mutex<Vec<api_bones_transport::Deadline>>>,
) -> PageSend<PageReq, PageResp> {
    timed_pages(each, deadlines, |request| answer(request, false))
}

fn walk(
    env: &CallEnv,
    each: Duration,
    deadlines: Arc<Mutex<Vec<api_bones_transport::Deadline>>>,
) -> api_bones_sdk::ListCall<u32> {
    env.call("acme.v1.Widgets", "ListWidgets").list(
        Idempotency::NoSideEffects,
        PageReq {
            cursor: String::new(),
        },
        two_pages(each, deadlines),
        PageOptions {
            prefetch: 0,
            ..PageOptions::default()
        },
    )
}

/// A list of three pages whose every page takes `each` to arrive: a fetch
/// ahead that is not held to one page is told apart from one that is by the
/// requests it makes.
fn three_pages(
    each: Duration,
    deadlines: Arc<Mutex<Vec<api_bones_transport::Deadline>>>,
) -> PageSend<PageReq, PageResp> {
    timed_pages(each, deadlines, |request| {
        let (items, next) = match request.cursor.as_str() {
            "" => (vec![1], "p2"),
            "p2" => (vec![2], "p3"),
            _ => (vec![3], ""),
        };
        ListResponse {
            body: PageResp { items, next },
            not_modified: false,
        }
    })
}

/// A three-page list started under `ctx`, its first page read and a fetch
/// ahead given time to start.
async fn first_page_read_under(
    ctx: CallCtx,
    deadlines: &Arc<Mutex<Vec<api_bones_transport::Deadline>>>,
) -> api_bones_sdk::ListCall<u32> {
    let list = CallCtx::scope(ctx, async {
        steady().call(SERVICE, "ListWidgets").list(
            Idempotency::NoSideEffects,
            PageReq {
                cursor: String::new(),
            },
            three_pages(Duration::from_millis(5), Arc::clone(deadlines)),
            PageOptions::default(),
        )
    })
    .await;
    let first = list.next().await.expect("page 1").expect("a page");
    assert_eq!(&*first.items, &[1]);
    tokio::time::sleep(Duration::from_millis(100)).await;
    list
}

#[tokio::test(start_paused = true)]
async fn a_list_owns_one_budget_per_page_not_one_for_the_walk() {
    let started = tokio::time::Instant::now();
    let deadlines = Arc::default();
    let list = walk(&steady(), Duration::from_secs(20), Arc::clone(&deadlines));
    let first = list.next().await.expect("page 1").expect("a page");
    let second = list.next().await.expect("page 2").expect("a page");
    assert_eq!(&*first.items, &[1]);
    assert_eq!(&*second.items, &[2]);
    assert!(!second.has_next);
    assert!(list.next().await.expect("the end").is_none());
    assert!(
        started.elapsed() > CALL_BUDGET,
        "the walk outlasted one budget"
    );
    let deadlines = deadlines.lock().expect("deadlines");
    assert_eq!(deadlines.len(), 2);
    assert_ne!(deadlines[0], deadlines[1], "each page has its own deadline");
}

#[tokio::test(start_paused = true)]
async fn a_page_that_outlasts_its_budget_fails_the_list() {
    let list = walk(&steady(), CALL_BUDGET * 2, Arc::default());
    let refusal = list.next().await.expect_err("out of time");
    assert_eq!(refusal.code, CALL_BUDGET_ELAPSED);
}

#[tokio::test(start_paused = true)]
async fn a_list_walk_is_an_internal_span_over_client_spans_per_page() {
    let (env, sink) = traced(steady());
    let list = walk(&env, Duration::from_millis(5), Arc::default());
    let walk_span = list.span().expect("the walk span is recording");
    while list.next().await.expect("a page").is_some() {}
    let starts = sink.starts.lock().expect("sink");
    assert_eq!(starts[0].kind, SpanKind::Internal);
    assert_eq!(starts[0].name, "acme.v1.Widgets/ListWidgets");
    let clients: Vec<_> = starts
        .iter()
        .filter(|s| s.kind == SpanKind::Client)
        .collect();
    assert_eq!(clients.len(), 2);
    assert!(
        clients
            .iter()
            .all(|s| s.parent.as_deref() == Some(walk_span.as_str()))
    );
    assert_eq!(sink.ends.lock().expect("sink").len(), 3);
}

#[tokio::test(start_paused = true)]
async fn an_abort_cancels_the_page_in_flight() {
    let flag = AbortFlag::new();
    let list = steady()
        .call(SERVICE, "ListWidgets")
        .with_abort(flag.clone())
        .list(
            Idempotency::NoSideEffects,
            PageReq {
                cursor: String::new(),
            },
            two_pages(Duration::from_secs(60), Arc::default()),
            PageOptions {
                prefetch: 0,
                ..PageOptions::default()
            },
        );
    let (page, ()) = tokio::join!(list.next(), async {
        tokio::task::yield_now().await;
        flag.abort();
    });
    assert_eq!(page.expect_err("canceled").code, CANCELED);
}

#[tokio::test(start_paused = true)]
async fn a_list_walk_never_continues_as_another_identity() {
    // Page 1 reads the credential four times (capture, before the send, after
    // the answer, before its delivery); page 2's first read already finds
    // another identity.
    let (env, sink) = traced(switching(4));
    let deadlines = Arc::default();
    let list = walk(&env, Duration::from_millis(5), Arc::clone(&deadlines));
    let first = list.next().await.expect("page 1").expect("a page");
    assert_eq!(&*first.items, &[1]);
    let refusal = list.next().await.expect_err("another identity");
    assert_eq!(refusal.code, IDENTITY_CHANGED);
    assert_eq!(refusal.emitter, "bones");
    assert_eq!(refusal.platform, "UNAUTHORIZED");
    assert_eq!(
        deadlines.lock().expect("deadlines").len(),
        1,
        "page 2 unsent"
    );
    assert!(list.next().await.expect("the walk ended").is_none());
    let ends = sink.ends.lock().expect("sink");
    assert!(ends.iter().any(|end| end.status == SpanStatus::Error));
}

/// An env whose identity is `a` until the returned flag is set, and `b` from
/// then on; every read carries a new token.
fn switchable() -> (CallEnv, Arc<AtomicBool>) {
    let switched = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&switched);
    let count = Arc::new(AtomicUsize::new(0));
    let env = CallEnv::new(fixed(move || {
        let n = count.fetch_add(1, Ordering::SeqCst);
        Ok(credential(
            if flag.load(Ordering::SeqCst) {
                "b"
            } else {
                "a"
            },
            n,
        ))
    }));
    (env, switched)
}

#[tokio::test(start_paused = true)]
async fn a_page_fetched_ahead_is_never_delivered_to_a_caller_who_switched_after_it_was_fetched() {
    let (env, switched) = switchable();
    let deadlines = Arc::default();
    let list = env.call(SERVICE, "ListWidgets").list(
        Idempotency::NoSideEffects,
        PageReq {
            cursor: String::new(),
        },
        two_pages(Duration::from_millis(5), Arc::clone(&deadlines)),
        PageOptions {
            prefetch: 1,
            ..PageOptions::default()
        },
    );
    let first = list.next().await.expect("page 1").expect("a page");
    assert_eq!(&*first.items, &[1]);
    // The page after it is fetched ahead, as the caller who read page 1.
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(deadlines.lock().expect("deadlines").len(), 2, "page 2 sent");
    switched.store(true, Ordering::SeqCst);
    let refusal = list.next().await.expect_err("another identity");
    assert_eq!(refusal.code, IDENTITY_CHANGED);
    assert_eq!(refusal.emitter, "bones");
    assert_eq!(refusal.platform, "UNAUTHORIZED");
    assert!(list.next().await.expect("the walk ended").is_none());
}

#[tokio::test(start_paused = true)]
async fn the_prefetch_a_caller_chose_decides_how_many_pages_are_read() {
    // Each row: the caller's choice, then the pages sent once the first page
    // was delivered and a fetch ahead had time to start. The list has three
    // pages, so a choice above 1 that were not held to one page ahead would
    // send a third request.
    for (chosen, sent) in [(Some(0), 1), (Some(1), 2), (Some(5), 2), (None, 2)] {
        let deadlines = Arc::default();
        let ctx = CallCtx {
            prefetch: chosen,
            ..CallCtx::default()
        };
        let _list = first_page_read_under(ctx, &deadlines).await;
        assert_eq!(
            deadlines.lock().expect("deadlines").len(),
            sent,
            "prefetch {chosen:?}"
        );
    }
}

#[tokio::test(start_paused = true)]
async fn a_first_page_call_makes_one_request_and_fetches_nothing_ahead() {
    let deadlines = Arc::default();
    let ctx = CallCtx {
        first_only: true,
        ..CallCtx::default()
    };
    let list = first_page_read_under(ctx, &deadlines).await;
    assert_eq!(deadlines.lock().expect("deadlines").len(), 1);
    assert!(list.next().await.expect("the walk ended").is_none());
    assert_eq!(deadlines.lock().expect("deadlines").len(), 1);
}

#[tokio::test(start_paused = true)]
async fn a_stream_ends_with_identity_changed_when_the_caller_switches_between_items() {
    let (env, switched) = switchable();
    let (env, sink) = traced(env);
    let handle = env
        .call(SERVICE, "Watch")
        .stream(Idempotency::NoSideEffects, |_| async {
            Ok::<_, ConnectError>(stream::iter([Ok(1_u32), Ok(2), Ok(3)]))
        })
        .await
        .expect("the head arrives");
    assert_eq!(handle.next().await.expect("an item"), Some(1));
    switched.store(true, Ordering::SeqCst);
    let refusal = handle.next().await.expect_err("another identity");
    assert_eq!(refusal.code, IDENTITY_CHANGED);
    assert_eq!(refusal.emitter, "bones");
    assert_eq!(refusal.platform, "UNAUTHORIZED");
    assert_eq!(handle.next().await.expect("the end"), None);
    let ends = sink.ends.lock().expect("sink");
    assert!(ends.iter().any(|end| end.status == SpanStatus::Error));
}

#[tokio::test(start_paused = true)]
async fn a_refreshed_token_of_the_same_identity_keeps_a_stream_flowing() {
    let handle = steady()
        .call(SERVICE, "Watch")
        .stream(Idempotency::NoSideEffects, |_| async {
            Ok::<_, ConnectError>(stream::iter([Ok(1_u32), Ok(2), Ok(3)]))
        })
        .await
        .expect("the head arrives");
    for expected in 1..=3 {
        assert_eq!(handle.next().await.expect("an item"), Some(expected));
    }
    assert_eq!(handle.next().await.expect("the end"), None);
}

#[tokio::test(start_paused = true)]
async fn a_stream_opened_as_one_identity_and_answered_as_another_is_refused_at_the_open() {
    let refusal = switching(2)
        .call(SERVICE, "Watch")
        .stream(Idempotency::NoSideEffects, |_| async {
            Ok::<_, ConnectError>(stream::iter([Ok(1_u32)]))
        })
        .await
        .expect_err("refused");
    assert_eq!(refusal.code, IDENTITY_CHANGED);
}

/// A two-page list that confirms the revision it is asked about, and every
/// send it was made.
fn revalidating(seen: Arc<Mutex<Vec<Attempt>>>) -> PageSend<PageReq, PageResp> {
    Arc::new(
        move |(request, attempt): (PageReq, Attempt)| -> BoxFuture<'static, _> {
            let confirms = header(&attempt, "if-none-match").as_deref() == Some("\"r1\"");
            seen.lock().expect("seen").push(attempt);
            Box::pin(async move { Ok(answer(&request, confirms)) })
        },
    )
}

/// `env` bound to a server, which the revision cache needs to scope its keys.
fn at(env: &CallEnv, base_url: &str) -> CallEnv {
    env.at(base_url).expect("a URI").1
}

fn bound(env: &CallEnv) -> CallEnv {
    at(env, "http://widgets.example")
}

fn revalidated_walk(
    env: &CallEnv,
    send: PageSend<PageReq, PageResp>,
) -> api_bones_sdk::ListCall<u32> {
    env.call(SERVICE, "ListWidgets").list(
        Idempotency::NoSideEffects,
        PageReq {
            cursor: String::new(),
        },
        send,
        PageOptions {
            prefetch: 0,
            ..PageOptions::default()
        },
    )
}

async fn pages_of(list: &api_bones_sdk::ListCall<u32>) -> Vec<api_bones_page::Page<u32>> {
    let mut pages = Vec::new();
    while let Some(page) = list.next().await.expect("a page") {
        pages.push(page);
    }
    pages
}

#[tokio::test(start_paused = true)]
async fn a_confirmed_revision_replays_the_cached_pages() {
    let env = bound(&steady().with_revision_cache(RevisionCache::default()));
    let seen = Arc::default();
    let first = pages_of(&revalidated_walk(&env, revalidating(Arc::clone(&seen)))).await;
    assert_eq!(first.len(), 2);
    assert!(first.iter().all(|page| !page.from_cache));
    // The first walk opted in without holding a revision to confirm.
    {
        let seen = seen.lock().expect("seen");
        assert_eq!(header(&seen[0], "bones-list-cache").as_deref(), Some("1"));
        assert_eq!(header(&seen[0], "if-none-match"), None);
    }
    let replayed = pages_of(&revalidated_walk(&env, revalidating(Arc::clone(&seen)))).await;
    let items: Vec<_> = replayed.iter().map(|page| page.items.to_vec()).collect();
    assert_eq!(items, [vec![1], vec![2]]);
    assert!(replayed.iter().all(|page| page.from_cache));
    let seen = seen.lock().expect("seen");
    assert_eq!(seen.len(), 3, "the replay cost one revalidation send");
    assert_eq!(header(&seen[2], "if-none-match").as_deref(), Some("\"r1\""));
}

#[tokio::test(start_paused = true)]
async fn a_walk_without_a_cache_sends_no_revalidation_headers_and_replays_nothing() {
    let env = steady();
    let seen = Arc::default();
    for _ in 0..2 {
        let pages = pages_of(&revalidated_walk(&env, revalidating(Arc::clone(&seen)))).await;
        assert_eq!(pages.len(), 2);
        assert!(pages.iter().all(|page| !page.from_cache));
    }
    for attempt in seen.lock().expect("seen").iter() {
        assert_eq!(header(attempt, "bones-list-cache"), None);
        assert_eq!(header(attempt, "if-none-match"), None);
    }
}

#[tokio::test(start_paused = true)]
async fn a_caller_who_switches_mid_replay_is_refused_and_gets_no_cached_page_after() {
    let (env, switched) = switchable();
    let cache = RevisionCache::default();
    let env = bound(&env.with_revision_cache(cache.clone()));
    let seen = Arc::default();
    let held = pages_of(&revalidated_walk(&env, revalidating(Arc::clone(&seen)))).await;
    assert_eq!(held.len(), 2);
    assert!(!cache.is_empty());
    let list = revalidated_walk(&env, revalidating(Arc::clone(&seen)));
    let first = list.next().await.expect("page 1").expect("a page");
    assert!(first.from_cache);
    switched.store(true, Ordering::SeqCst);
    let refusal = list.next().await.expect_err("another identity");
    assert_eq!(refusal.code, IDENTITY_CHANGED);
    assert!(list.next().await.expect("the walk ended").is_none());
    assert!(cache.is_empty(), "the switch dropped the cache");
}

#[tokio::test(start_paused = true)]
async fn another_identity_never_shares_the_cache_of_the_first() {
    let (env, switched) = switchable();
    let env = bound(&env.with_revision_cache(RevisionCache::default()));
    let seen = Arc::default();
    let held = pages_of(&revalidated_walk(&env, revalidating(Arc::clone(&seen)))).await;
    assert_eq!(held.len(), 2);
    switched.store(true, Ordering::SeqCst);
    let other = pages_of(&revalidated_walk(&env, revalidating(Arc::clone(&seen)))).await;
    assert_eq!(other.len(), 2);
    assert!(other.iter().all(|page| !page.from_cache));
    let seen = seen.lock().expect("seen");
    assert_eq!(
        header(&seen[2], "if-none-match"),
        None,
        "nothing to confirm"
    );
}

#[tokio::test(start_paused = true)]
async fn a_bound_cache_keeps_the_codes_of_a_server_refusal() {
    let cache = RevisionCache::default();
    let env = bound(&steady().with_revision_cache(cache.clone()));
    let refusing: PageSend<PageReq, PageResp> =
        Arc::new(|_: (PageReq, Attempt)| -> BoxFuture<'static, _> {
            Box::pin(async { Err(ConnectError::unauthenticated("token expired")) })
        });
    let refusal = revalidated_walk(&env, refusing)
        .next()
        .await
        .expect_err("refused");
    assert_eq!(
        refusal.code,
        Error::from(ConnectError::unauthenticated("token expired")).code
    );
    assert_ne!(refusal.code, IDENTITY_CHANGED);
    assert!(cache.is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_credential_that_cannot_be_read_fails_the_cached_walk_closed() {
    let env = bound(
        &CallEnv::new(fixed(|| Err(ConnectError::unavailable("no credential"))))
            .with_revision_cache(RevisionCache::default()),
    );
    let seen = Arc::default();
    let refusal = revalidated_walk(&env, revalidating(Arc::clone(&seen)))
        .next()
        .await
        .expect_err("closed");
    assert_eq!(refusal.platform, "SERVICE_UNAVAILABLE");
    assert_eq!(seen.lock().expect("seen").len(), 0, "nothing was sent");
}

#[tokio::test(start_paused = true)]
async fn a_switch_while_a_page_is_in_flight_keeps_identity_changed_with_a_cache_bound() {
    let (env, switched) = switchable();
    let cache = RevisionCache::default();
    let env = bound(&env.with_revision_cache(cache.clone()));
    let send: PageSend<PageReq, PageResp> = Arc::new(
        move |(request, _): (PageReq, Attempt)| -> BoxFuture<'static, _> {
            switched.store(true, Ordering::SeqCst);
            Box::pin(async move { Ok(answer(&request, false)) })
        },
    );
    let refusal = revalidated_walk(&env, send)
        .next()
        .await
        .expect_err("another identity");
    assert_eq!(refusal.code, IDENTITY_CHANGED);
    assert!(cache.is_empty());
}

#[tokio::test(start_paused = true)]
async fn an_env_bound_to_no_server_caches_nothing() {
    let cache = RevisionCache::default();
    let env = steady().with_revision_cache(cache.clone());
    let seen = Arc::default();
    for _ in 0..2 {
        let pages = pages_of(&revalidated_walk(&env, revalidating(Arc::clone(&seen)))).await;
        assert!(pages.iter().all(|page| !page.from_cache));
    }
    assert!(cache.is_empty());
    for attempt in seen.lock().expect("seen").iter() {
        assert_eq!(header(attempt, "bones-list-cache"), None);
        assert_eq!(header(attempt, "if-none-match"), None);
    }
}

#[tokio::test(start_paused = true)]
async fn two_servers_sharing_one_cache_never_replay_each_others_pages() {
    let cache = RevisionCache::default();
    let staging = at(
        &steady().with_revision_cache(cache.clone()),
        "http://staging.example",
    );
    let prod = at(
        &steady().with_revision_cache(cache.clone()),
        "http://prod.example",
    );
    let seen = Arc::default();
    // A chain replays while it is cached within budget.
    pages_of(&revalidated_walk(&staging, revalidating(Arc::clone(&seen)))).await;
    let other = pages_of(&revalidated_walk(&prod, revalidating(Arc::clone(&seen)))).await;
    assert!(other.iter().all(|page| !page.from_cache));
    assert_eq!(
        header(&seen.lock().expect("seen")[2], "if-none-match"),
        None
    );
    let again = pages_of(&revalidated_walk(&staging, revalidating(Arc::clone(&seen)))).await;
    assert!(
        again.iter().all(|page| page.from_cache),
        "each server keeps its own chain"
    );
}

#[tokio::test(start_paused = true)]
async fn a_refusal_of_one_env_leaves_another_envs_cached_walk_replaying() {
    let cache = RevisionCache::default();
    let server = "http://widgets.example";
    let alice = at(
        &CallEnv::new(fixed(|| Ok(credential("a", 0)))).with_revision_cache(cache.clone()),
        server,
    );
    let bob = at(
        &CallEnv::new(fixed(|| Ok(credential("b", 0)))).with_revision_cache(cache.clone()),
        server,
    );
    let seen = Arc::default();
    pages_of(&revalidated_walk(&alice, revalidating(Arc::clone(&seen)))).await;
    pages_of(&revalidated_walk(&bob, revalidating(Arc::clone(&seen)))).await;
    alice
        .call(SERVICE, "GetWidget")
        .unary(Idempotency::NoSideEffects, |_| {
            std::future::ready(Err::<u32, _>(ConnectError::unauthenticated("expired")))
        })
        .await
        .expect_err("refused");
    let bob_again = pages_of(&revalidated_walk(&bob, revalidating(Arc::clone(&seen)))).await;
    assert!(
        bob_again.iter().all(|page| page.from_cache),
        "the other env's chain survives"
    );
    let alice_again = pages_of(&revalidated_walk(&alice, revalidating(Arc::clone(&seen)))).await;
    assert!(
        alice_again.iter().all(|page| !page.from_cache),
        "the refused env's own chain is gone"
    );
}

#[test]
fn a_base_url_that_is_not_a_uri_is_invalid_argument() {
    let error = steady().at("not a uri").err().expect("refused");
    assert_eq!(error.platform, "VALIDATION_FAILED");
}

#[tokio::test(start_paused = true)]
async fn a_refused_call_drops_the_slot_of_the_identity_it_started_as() {
    let (env, switched) = switchable();
    let cache = RevisionCache::default();
    let env = bound(&env.with_revision_cache(cache.clone()));
    let seen = Arc::default();
    // Other sessions of the shared cache fill the slots of `a` and of `b`; the
    // refused session recorded neither, so only the refusal can drop `a`'s.
    let (other_a, _) = endable("a", &Arc::default(), &cache);
    let (other_b, _) = endable("b", &Arc::default(), &cache);
    pages_of(&revalidated_walk(&other_a, revalidating(Arc::clone(&seen)))).await;
    pages_of(&revalidated_walk(&other_b, revalidating(Arc::clone(&seen)))).await;
    // A call that started as `a` is refused after the credential became `b`.
    let flag = Arc::clone(&switched);
    let refusal = env
        .call(SERVICE, "GetWidget")
        .unary(Idempotency::NoSideEffects, move |_| {
            flag.store(true, Ordering::SeqCst);
            std::future::ready(Ok::<u32, _>(1))
        })
        .await
        .expect_err("another identity");
    assert_eq!(refusal.code, IDENTITY_CHANGED);
    let bob = pages_of(&revalidated_walk(&other_b, revalidating(Arc::clone(&seen)))).await;
    assert!(
        bob.iter().all(|page| page.from_cache),
        "b's slot is untouched"
    );
    let ada = pages_of(&revalidated_walk(&other_a, revalidating(Arc::clone(&seen)))).await;
    assert!(
        ada.iter().all(|page| !page.from_cache),
        "a's slot was dropped"
    );
}

/// An env of `subject` whose credential source fails closed once `down` is
/// set, bound to the widgets server and sharing `cache`; the count of
/// unauthenticated refusals told to the host.
fn endable(
    subject: &'static str,
    down: &Arc<AtomicBool>,
    cache: &RevisionCache,
) -> (CallEnv, Arc<AtomicUsize>) {
    let down = Arc::clone(down);
    let told = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&told);
    let env = CallEnv::new(fixed(move || {
        if down.load(Ordering::SeqCst) {
            Err(ConnectError::unavailable("signed out"))
        } else {
            Ok(credential(subject, 0))
        }
    }))
    .with_revision_cache(cache.clone())
    .on_unauthenticated(Arc::new(move || {
        seen.fetch_add(1, Ordering::SeqCst);
    }));
    (bound(&env), told)
}

#[tokio::test(start_paused = true)]
async fn a_session_that_can_no_longer_answer_drops_the_slot_it_filled_without_telling_the_host() {
    let cache = RevisionCache::default();
    let down = Arc::new(AtomicBool::new(false));
    let (env, told) = endable("a", &down, &cache);
    let seen = Arc::default();
    pages_of(&revalidated_walk(&env, revalidating(Arc::clone(&seen)))).await;
    assert!(!cache.is_empty());
    down.store(true, Ordering::SeqCst);
    let refusal = revalidated_walk(&env, revalidating(Arc::clone(&seen)))
        .next()
        .await
        .expect_err("signed out");
    assert_eq!(refusal.platform, "SERVICE_UNAVAILABLE");
    assert!(cache.is_empty(), "the signed-out caller's slot is gone");
    assert_eq!(told.load(Ordering::SeqCst), 0);
}

#[tokio::test(start_paused = true)]
async fn a_session_that_switches_caller_drops_the_slot_of_the_one_before() {
    let (env, switched) = switchable();
    let cache = RevisionCache::default();
    let env = bound(&env.with_revision_cache(cache.clone()));
    let seen = Arc::default();
    pages_of(&revalidated_walk(&env, revalidating(Arc::clone(&seen)))).await;
    switched.store(true, Ordering::SeqCst);
    let bob = pages_of(&revalidated_walk(&env, revalidating(Arc::clone(&seen)))).await;
    assert!(bob.iter().all(|page| !page.from_cache));
    let bob_again = pages_of(&revalidated_walk(&env, revalidating(Arc::clone(&seen)))).await;
    assert!(
        bob_again.iter().all(|page| page.from_cache),
        "b's slot is held"
    );
    // Another session of the shared cache reads `a`: nothing of `a` is left.
    let down = Arc::new(AtomicBool::new(false));
    let (ada, _) = endable("a", &down, &cache);
    let ada = pages_of(&revalidated_walk(&ada, revalidating(Arc::clone(&seen)))).await;
    assert!(
        ada.iter().all(|page| !page.from_cache),
        "a's slot was dropped when the session moved to b"
    );
}

#[tokio::test(start_paused = true)]
async fn a_session_that_ends_leaves_the_slot_of_another_session_of_the_shared_cache() {
    let cache = RevisionCache::default();
    let down = Arc::new(AtomicBool::new(false));
    let (alice, _) = endable("a", &down, &cache);
    let (bob, _) = endable("b", &Arc::default(), &cache);
    let seen = Arc::default();
    pages_of(&revalidated_walk(&alice, revalidating(Arc::clone(&seen)))).await;
    pages_of(&revalidated_walk(&bob, revalidating(Arc::clone(&seen)))).await;
    down.store(true, Ordering::SeqCst);
    revalidated_walk(&alice, revalidating(Arc::clone(&seen)))
        .next()
        .await
        .expect_err("signed out");
    let bob_again = pages_of(&revalidated_walk(&bob, revalidating(Arc::clone(&seen)))).await;
    assert!(
        bob_again.iter().all(|page| page.from_cache),
        "the other session's slot is held"
    );
    let (fresh, _) = endable("a", &Arc::default(), &cache);
    let alice_again = pages_of(&revalidated_walk(&fresh, revalidating(Arc::clone(&seen)))).await;
    assert!(
        alice_again.iter().all(|page| !page.from_cache),
        "the ended session's slot is gone"
    );
}

/// A list send that answers like `revalidating` once `open` is set, and waits
/// for `gate` before its first answer.
fn held_until(
    gate: &Arc<tokio::sync::Notify>,
    open: &Arc<AtomicBool>,
) -> PageSend<PageReq, PageResp> {
    let gate = Arc::clone(gate);
    let open = Arc::clone(open);
    Arc::new(
        move |(request, _): (PageReq, Attempt)| -> BoxFuture<'static, _> {
            let gate = Arc::clone(&gate);
            let open = Arc::clone(&open);
            Box::pin(async move {
                if !open.load(Ordering::SeqCst) {
                    gate.notified().await;
                }
                Ok(answer(&request, false))
            })
        },
    )
}

#[tokio::test(start_paused = true)]
async fn a_walk_in_flight_when_the_session_ends_is_refused_and_holds_nothing() {
    let cache = RevisionCache::default();
    let down = Arc::new(AtomicBool::new(false));
    let (env, _) = endable("a", &down, &cache);
    let seen = Arc::default();
    let gate = Arc::new(tokio::sync::Notify::new());
    let open = Arc::new(AtomicBool::new(false));
    // The walk opens `a`'s slot before its first send. The end removes the
    // slot, so the answer that arrives afterwards is refused and never stored.
    let walk = revalidated_walk(&env, held_until(&gate, &open));
    let in_flight = walk.next();
    let sign_out_and_back_in = async {
        tokio::time::sleep(Duration::from_millis(1)).await;
        down.store(true, Ordering::SeqCst);
        revalidated_walk(&env, revalidating(Arc::clone(&seen)))
            .next()
            .await
            .expect_err("signed out");
        down.store(false, Ordering::SeqCst);
        open.store(true, Ordering::SeqCst);
        gate.notify_one();
    };
    let (first, ()) = tokio::join!(in_flight, sign_out_and_back_in);
    let refusal = first.expect_err("the walk spanned a session end");
    assert_eq!(refusal.platform, "UNAUTHORIZED");
    assert!(cache.is_empty(), "the refused walk left pages held");
    // A further end still leaves nothing held.
    down.store(true, Ordering::SeqCst);
    revalidated_walk(&env, revalidating(Arc::clone(&seen)))
        .next()
        .await
        .expect_err("signed out again");
    assert!(cache.is_empty(), "the second end left a's pages held");
    down.store(false, Ordering::SeqCst);
    let again = pages_of(&revalidated_walk(&env, revalidating(Arc::clone(&seen)))).await;
    assert!(again.iter().all(|page| !page.from_cache));
}

/// An env over `source` and the count of unauthenticated refusals it told the host.
fn telling(source: impl CredentialSource + 'static) -> (CallEnv, Arc<AtomicUsize>) {
    let told = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&told);
    let env = CallEnv::new(source).on_unauthenticated(Arc::new(move || {
        seen.fetch_add(1, Ordering::SeqCst);
    }));
    (env, told)
}

/// An env that reads `plain` and, when asked to refresh, answers `renewed`;
/// the count of unauthenticated refusals told to the host and of refreshes the
/// source was asked for.
fn refreshing(
    renewed: Result<Credential, ConnectError>,
) -> (CallEnv, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let (source, asked) = renewing(credential("a", 1), renewed);
    let (env, told) = telling(source);
    (env, asked, told)
}

/// The `authorization` of each send of a unary call that is refused as
/// unauthenticated `refused` times, then answered; the call's outcome.
async fn refused_then_answered(
    env: &CallEnv,
    refused: usize,
) -> (Result<u32, Error>, Vec<Option<String>>) {
    let tokens = Mutex::new(Vec::new());
    let outcome = env
        .call(SERVICE, "GetWidget")
        .unary(Idempotency::NoSideEffects, |attempt| {
            let mut tokens = tokens.lock().expect("tokens");
            tokens.push(header(&attempt, "authorization"));
            std::future::ready(if tokens.len() <= refused {
                Err(ConnectError::unauthenticated("expired"))
            } else {
                Ok(7)
            })
        })
        .await;
    (outcome, tokens.into_inner().expect("tokens"))
}

#[tokio::test(start_paused = true)]
async fn a_refused_credential_is_refreshed_and_the_call_replayed_with_the_renewed_one() {
    let (env, asked, told) = refreshing(Ok(credential("a", 2)));
    let (outcome, tokens) = refused_then_answered(&env, 1).await;
    assert_eq!(outcome.expect("replayed"), 7);
    assert_eq!(
        tokens,
        [Some("Bearer 1".to_owned()), Some("Bearer 2".to_owned())]
    );
    assert_eq!(asked.load(Ordering::SeqCst), 1);
    assert_eq!(told.load(Ordering::SeqCst), 0);
}

#[tokio::test(start_paused = true)]
async fn the_replay_is_not_counted_against_the_retry_budget() {
    let retries = usize::from(retries_for(Idempotency::NoSideEffects));
    let (env, asked, _) = refreshing(Ok(credential("a", 2)));
    let sent = AtomicUsize::new(0);
    let answer = env
        .call(SERVICE, "GetWidget")
        .unary(Idempotency::NoSideEffects, |_| {
            // One unavailable answer, the refusal, then every retry the budget
            // allows, all unavailable, before the answer.
            let n = sent.fetch_add(1, Ordering::SeqCst);
            std::future::ready(match n {
                1 => Err(ConnectError::unauthenticated("expired")),
                n if n <= 1 + retries => Err(ConnectError::unavailable("down")),
                _ => Ok(7_u32),
            })
        })
        .await
        .expect("the replay keeps a retry budget of its own");
    assert_eq!(answer, 7);
    assert_eq!(sent.load(Ordering::SeqCst), retries + 3);
    assert_eq!(asked.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn a_refresh_that_answers_the_refused_credential_returns_the_servers_error_and_tells_the_host_once()
 {
    let (env, asked, told) = refreshing(Ok(credential("a", 1)));
    let (outcome, tokens) = refused_then_answered(&env, 1).await;
    let refusal = outcome.expect_err("refused");
    assert_eq!(refusal.platform, "UNAUTHORIZED");
    assert_ne!(refusal.code, IDENTITY_CHANGED);
    assert_eq!(tokens.len(), 1, "nothing is replayed");
    assert_eq!(asked.load(Ordering::SeqCst), 1);
    assert_eq!(told.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn a_refusal_before_any_send_carried_a_credential_is_returned_without_a_refresh() {
    let reads = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&reads);
    let asked = Arc::new(AtomicUsize::new(0));
    let refreshes = Arc::clone(&asked);
    let env = CallEnv::new(move |ask: CredentialAsk| {
        refreshes.fetch_add(usize::from(ask.refresh), Ordering::SeqCst);
        // The capture reads once; the read before the send is refused.
        let answer = if count.fetch_add(1, Ordering::SeqCst) == 0 {
            Ok(credential("a", 1))
        } else {
            Err(ConnectError::unauthenticated("the source is signed out"))
        };
        async move { answer }
    });
    let sent = AtomicUsize::new(0);
    let outcome = env
        .call(SERVICE, "GetWidget")
        .unary(Idempotency::NoSideEffects, |_| {
            sent.fetch_add(1, Ordering::SeqCst);
            std::future::ready(Ok(7_u32))
        })
        .await;
    assert_eq!(outcome.expect_err("refused").platform, "UNAUTHORIZED");
    assert_eq!(sent.load(Ordering::SeqCst), 0, "nothing is sent");
    assert_eq!(asked.load(Ordering::SeqCst), 0, "nothing is refreshed");
}

#[tokio::test(start_paused = true)]
async fn a_refused_replay_is_not_refreshed_again_and_tells_the_host_once() {
    let (env, asked, told) = refreshing(Ok(credential("a", 2)));
    let (outcome, tokens) = refused_then_answered(&env, 2).await;
    let refusal = outcome.expect_err("refused");
    assert_eq!(refusal.platform, "UNAUTHORIZED");
    assert_eq!(tokens.len(), 2, "one send and one replay");
    assert_eq!(asked.load(Ordering::SeqCst), 1);
    assert_eq!(told.load(Ordering::SeqCst), 1);
}

/// Like `refreshing`, but a refresh spans several polls, so calls refused
/// together wait for one renewal, and the renewed credential is what a plain
/// read answers once it is made.
fn contended(renewed: Credential) -> (CallEnv, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let plain = Arc::new(Mutex::new(credential("a", 1)));
    let asked = Arc::new(AtomicUsize::new(0));
    let (held, count) = (Arc::clone(&plain), Arc::clone(&asked));
    let source = move |ask: CredentialAsk| {
        let (plain, count, renewed) = (Arc::clone(&held), Arc::clone(&count), renewed.clone());
        async move {
            if !ask.refresh {
                return Ok(plain.lock().expect("plain").clone());
            }
            count.fetch_add(1, Ordering::SeqCst);
            for _ in 0..3 {
                tokio::task::yield_now().await;
            }
            *plain.lock().expect("plain") = renewed.clone();
            Ok(renewed)
        }
    };
    let (env, told) = telling(source);
    (env, asked, told)
}

#[tokio::test(start_paused = true)]
async fn concurrent_calls_refused_on_one_credential_renew_it_once() {
    let (env, asked, told) = contended(credential("a", 2));
    let calls = futures_util::future::join_all((0..4).map(|_| refused_then_answered(&env, 1)));
    for (outcome, tokens) in calls.await {
        assert_eq!(outcome.expect("replayed"), 7);
        assert_eq!(
            tokens,
            [Some("Bearer 1".to_owned()), Some("Bearer 2".to_owned())]
        );
    }
    assert_eq!(asked.load(Ordering::SeqCst), 1);
    assert_eq!(told.load(Ordering::SeqCst), 0);
}

#[tokio::test(start_paused = true)]
async fn concurrent_calls_whose_renewal_is_the_refused_credential_tell_the_host_once() {
    let (env, asked, told) = contended(credential("a", 1));
    let calls = futures_util::future::join_all((0..4).map(|_| refused_then_answered(&env, 1)));
    for (outcome, tokens) in calls.await {
        let refusal = outcome.expect_err("refused");
        assert_eq!(refusal.platform, "UNAUTHORIZED");
        assert_ne!(refusal.code, IDENTITY_CHANGED);
        assert_eq!(tokens.len(), 1, "nothing is replayed");
    }
    assert_eq!(asked.load(Ordering::SeqCst), 1);
    assert_eq!(told.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn concurrent_calls_whose_replay_is_refused_again_share_one_renewal_and_start_no_second() {
    let (env, asked, told) = contended(credential("a", 2));
    let calls = futures_util::future::join_all((0..4).map(|_| refused_then_answered(&env, 2)));
    for (outcome, tokens) in calls.await {
        assert_eq!(outcome.expect_err("refused").platform, "UNAUTHORIZED");
        assert_eq!(tokens.len(), 2, "one send and one replay");
    }
    assert_eq!(asked.load(Ordering::SeqCst), 1);
    assert_eq!(told.load(Ordering::SeqCst), 4, "each refused replay tells");
}

#[tokio::test(start_paused = true)]
async fn a_refresh_that_answers_another_identity_is_identity_changed() {
    let (env, asked, told) = refreshing(Ok(credential("b", 2)));
    let (outcome, tokens) = refused_then_answered(&env, 1).await;
    let refusal = outcome.expect_err("refused");
    assert_eq!(refusal.code, IDENTITY_CHANGED);
    assert_eq!(
        tokens.len(),
        1,
        "the other caller's credential is never sent"
    );
    assert_eq!(asked.load(Ordering::SeqCst), 1);
    assert_eq!(told.load(Ordering::SeqCst), 0);
}

#[tokio::test(start_paused = true)]
async fn a_refresh_that_cannot_be_read_does_not_tell_the_host() {
    let (env, asked, told) = refreshing(Err(ConnectError::unavailable("the session is gone")));
    let (outcome, tokens) = refused_then_answered(&env, 1).await;
    let refusal = outcome.expect_err("refused");
    assert_eq!(refusal.platform, "SERVICE_UNAVAILABLE");
    assert_eq!(tokens.len(), 1);
    assert_eq!(asked.load(Ordering::SeqCst), 1);
    assert_eq!(told.load(Ordering::SeqCst), 0);
}

/// `refreshing(renewed)` bound to the widgets server over a revision cache
/// that holds the slot of its caller `a`.
async fn refreshing_with_a_slot(
    renewed: Result<Credential, ConnectError>,
) -> (CallEnv, Arc<AtomicUsize>, RevisionCache) {
    let (source, _) = renewing(credential("a", 1), renewed);
    let told = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&told);
    let cache = RevisionCache::default();
    let env = bound(
        &CallEnv::new(source)
            .with_revision_cache(cache.clone())
            .on_unauthenticated(Arc::new(move || {
                seen.fetch_add(1, Ordering::SeqCst);
            })),
    );
    pages_of(&revalidated_walk(&env, revalidating(Arc::default()))).await;
    assert!(!cache.is_empty());
    (env, told, cache)
}

#[tokio::test(start_paused = true)]
async fn a_refresh_that_fails_drops_the_callers_slot_and_tells_the_host_nothing() {
    for failure in [
        ConnectError::unavailable("the session is gone"),
        ConnectError::failed_precondition("the session is gone"),
    ] {
        let (env, told, cache) = refreshing_with_a_slot(Err(failure)).await;
        let (outcome, _) = refused_then_answered(&env, 1).await;
        outcome.expect_err("refused");
        assert!(cache.is_empty(), "the caller's slot is gone");
        assert_eq!(told.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test(start_paused = true)]
async fn a_stream_refused_after_an_item_is_never_replayed() {
    let (env, asked, told) = refreshing(Ok(credential("a", 2)));
    let handle = env
        .call(SERVICE, "Watch")
        .stream(Idempotency::NoSideEffects, |_| async {
            Ok::<_, ConnectError>(stream::iter([
                Ok(1_u32),
                Err(ConnectError::unauthenticated("expired")),
            ]))
        })
        .await
        .expect("the head arrives");
    assert_eq!(handle.next().await.expect("an item"), Some(1));
    let refusal = handle.next().await.expect_err("refused mid-stream");
    assert_eq!(refusal.platform, "UNAUTHORIZED");
    assert_eq!(asked.load(Ordering::SeqCst), 0, "nothing is refreshed");
    assert_eq!(told.load(Ordering::SeqCst), 1, "the host is told once");
}

/// What a `Watch` open answers: its frames, or the open's own refusal.
type Opened = Result<Vec<Result<u32, ConnectError>>, ConnectError>;

fn expired() -> ConnectError {
    ConnectError::unauthenticated("expired")
}

/// A `Watch` on `env` whose nth open answers `opens[n]`, the last one every
/// open after it, read to its end: what it delivered and how often it opened.
async fn watched(env: &CallEnv, opens: Vec<Opened>) -> (Vec<Result<u32, Error>>, usize) {
    let opened = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&opened);
    let handle = env
        .call(SERVICE, "Watch")
        .stream(Idempotency::NoSideEffects, move |_| {
            let n = count.fetch_add(1, Ordering::SeqCst);
            let answer = opens[n.min(opens.len() - 1)].clone();
            async move { answer.map(stream::iter) }
        })
        .await
        .expect("the first open is answered");
    let mut delivered = Vec::new();
    loop {
        match handle.next().await {
            Ok(Some(item)) => delivered.push(Ok(item)),
            Ok(None) => break,
            Err(error) => delivered.push(Err(error)),
        }
    }
    (delivered, opened.load(Ordering::SeqCst))
}

/// The watch delivered one item, a refusal as unauthorized, and the host was
/// told of it once.
fn refused_once_and_told(delivered: &[Result<u32, Error>], told: &AtomicUsize) {
    assert_eq!(delivered.len(), 1);
    assert_eq!(
        delivered[0].as_ref().expect_err("refused").platform,
        "UNAUTHORIZED"
    );
    assert_eq!(told.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn a_stream_refused_in_its_first_frame_is_renewed_and_opened_again() {
    let (env, asked, told) = refreshing(Ok(credential("a", 2)));
    let (delivered, opened) = watched(&env, vec![Ok(vec![Err(expired())]), Ok(vec![Ok(1)])]).await;
    assert_eq!(delivered.len(), 1);
    assert_eq!(*delivered[0].as_ref().expect("an item"), 1);
    assert_eq!(opened, 2);
    assert_eq!(asked.load(Ordering::SeqCst), 1);
    assert_eq!(
        told.load(Ordering::SeqCst),
        0,
        "the renewed open is not a refusal"
    );
}

#[tokio::test(start_paused = true)]
async fn a_stream_refused_in_its_first_frame_twice_tells_the_host_once_and_drops_the_slot() {
    let (env, told, cache) = refreshing_with_a_slot(Ok(credential("a", 2))).await;
    let (delivered, opened) = watched(&env, vec![Ok(vec![Err(expired())])]).await;
    assert_eq!(opened, 2, "replayed once and no more");
    refused_once_and_told(&delivered, &told);
    assert!(cache.is_empty(), "the caller's slot is gone");
}

#[tokio::test(start_paused = true)]
async fn a_stream_whose_renewed_open_is_refused_tells_the_host_once() {
    let (env, asked, told) = refreshing(Ok(credential("a", 2)));
    let (delivered, opened) = watched(&env, vec![Ok(vec![Err(expired())]), Err(expired())]).await;
    assert_eq!(opened, 2);
    assert_eq!(asked.load(Ordering::SeqCst), 1);
    refused_once_and_told(&delivered, &told);
}

#[tokio::test(start_paused = true)]
async fn a_stream_refused_in_its_first_frame_with_nothing_newer_to_send_tells_the_host_once() {
    let (env, asked, told) = refreshing(Ok(credential("a", 1)));
    let (delivered, opened) = watched(&env, vec![Ok(vec![Err(expired())])]).await;
    assert_eq!(opened, 1, "no renewed credential, no replay");
    assert_eq!(asked.load(Ordering::SeqCst), 1);
    refused_once_and_told(&delivered, &told);
}

#[tokio::test(start_paused = true)]
async fn a_head_refused_and_replayed_leaves_no_renewal_for_the_first_frame() {
    let (env, told, cache) = refreshing_with_a_slot(Ok(credential("a", 2))).await;
    let (delivered, opened) = watched(&env, vec![Err(expired()), Ok(vec![Err(expired())])]).await;
    assert_eq!(opened, 2, "the head replayed once, the first frame not");
    refused_once_and_told(&delivered, &told);
    assert!(cache.is_empty(), "the caller's slot is gone");
}

#[tokio::test(start_paused = true)]
async fn a_head_refused_and_replayed_asks_the_source_once_across_the_first_frame() {
    let (source, asked) = renewing(credential("a", 1), Ok(credential("a", 2)));
    let (env, told) = telling(source);
    let (delivered, opened) = watched(&env, vec![Err(expired()), Ok(vec![Err(expired())])]).await;
    assert_eq!(opened, 2);
    assert_eq!(delivered.len(), 1);
    assert_eq!(asked.load(Ordering::SeqCst), 1);
    assert_eq!(told.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn a_stream_whose_first_frame_renewal_fails_drops_the_slot_and_tells_the_host_nothing() {
    let (env, told, cache) =
        refreshing_with_a_slot(Err(ConnectError::unavailable("the session is gone"))).await;
    let (delivered, opened) = watched(&env, vec![Ok(vec![Err(expired())])]).await;
    assert_eq!(opened, 1, "no renewed credential, no replay");
    assert_eq!(delivered.len(), 1);
    delivered[0].as_ref().expect_err("refused");
    assert!(cache.is_empty(), "the caller's slot is gone");
    assert_eq!(told.load(Ordering::SeqCst), 0);
}

#[tokio::test(start_paused = true)]
async fn a_stream_refused_in_its_first_frame_for_another_reason_is_not_renewed() {
    let (env, asked, told) = refreshing(Ok(credential("a", 2)));
    let (delivered, opened) =
        watched(&env, vec![Ok(vec![Err(ConnectError::internal("boom"))])]).await;
    assert_eq!(opened, 1);
    assert_eq!(delivered.len(), 1);
    delivered[0].as_ref().expect_err("failed");
    assert_eq!(asked.load(Ordering::SeqCst), 0);
    assert_eq!(told.load(Ordering::SeqCst), 0);
}

#[tokio::test(start_paused = true)]
async fn a_stream_that_ends_before_its_first_frame_delivers_nothing_and_renews_nothing() {
    let (env, asked, told) = refreshing(Ok(credential("a", 2)));
    let (delivered, opened) = watched(&env, vec![Ok(vec![])]).await;
    assert!(delivered.is_empty(), "no frame was delivered");
    assert_eq!(opened, 1);
    assert_eq!(asked.load(Ordering::SeqCst), 0);
    assert_eq!(told.load(Ordering::SeqCst), 0);
}

/// A source that reads `credential("a", 1)` and never answers a refresh.
fn hanging_refresh() -> impl CredentialSource {
    |ask: CredentialAsk| async move {
        if ask.refresh {
            std::future::pending::<Result<Credential, ConnectError>>().await
        } else {
            Ok(credential("a", 1))
        }
    }
}

#[tokio::test(start_paused = true)]
async fn a_renewal_of_a_first_frame_that_never_answers_is_cut_at_the_budget() {
    let (env, told) = telling(hanging_refresh());
    let started = tokio::time::Instant::now();
    let (delivered, opened) = watched(&env, vec![Ok(vec![Err(expired())])]).await;
    assert_eq!(opened, 1);
    assert_eq!(delivered.len(), 1);
    assert_eq!(
        delivered[0].as_ref().expect_err("out of time").code,
        CALL_BUDGET_ELAPSED
    );
    assert_eq!(started.elapsed(), CALL_BUDGET);
    assert_eq!(told.load(Ordering::SeqCst), 0);
}

#[tokio::test(start_paused = true)]
async fn an_abort_during_a_first_frame_renewal_ends_the_stream_canceled() {
    let (env, _) = telling(hanging_refresh());
    let flag = AbortFlag::new();
    let handle = env
        .call(SERVICE, "Watch")
        .with_abort(flag.clone())
        .stream(Idempotency::NoSideEffects, |_| async {
            Ok::<_, ConnectError>(stream::iter([Err::<u32, _>(expired())]))
        })
        .await
        .expect("the head arrives");
    let (next, ()) = tokio::join!(handle.next(), async {
        tokio::task::yield_now().await;
        flag.abort();
    });
    assert_eq!(next.expect_err("canceled").code, CANCELED);
}

#[tokio::test(start_paused = true)]
async fn a_list_reads_its_identity_at_its_first_poll_not_when_it_is_built() {
    let (env, switched) = switchable();
    let list = walk(&env, Duration::from_millis(5), Arc::default());
    // The caller changes after the list was built and before it was read: the
    // walk is the one caller that reads it, so it is not refused.
    switched.store(true, Ordering::SeqCst);
    let first = list.next().await.expect("page 1").expect("a page");
    assert_eq!(&*first.items, &[1]);
}

/// A sink that records nothing: every span it is asked to start is not
/// recording, and it counts what the core crossed to it.
#[derive(Default)]
struct Quiet {
    starts: Mutex<Vec<SpanStart>>,
    ends: Mutex<Vec<SpanEnd>>,
}

impl SpanSink for Quiet {
    fn start(&self, span: &SpanStart) -> SpanStarted {
        self.starts.lock().expect("sink").push(span.clone());
        SpanStarted::not_recording(format!("00-{:032x}-{:016x}-01", 0xabcd_u128, 1_u64))
    }

    fn end(&self, end: &SpanEnd) {
        self.ends.lock().expect("sink").push(end.clone());
    }
}

#[tokio::test(start_paused = true)]
async fn a_walk_whose_span_is_not_recorded_makes_no_crossing_for_its_pages() {
    let sink = Arc::new(Quiet::default());
    let env = steady().with_tracer(Tracer::new(sink.clone()));
    let sent = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&sent);
    let pager: PageSend<PageReq, PageResp> = Arc::new(
        move |(request, attempt): (PageReq, Attempt)| -> BoxFuture<'static, _> {
            seen.lock()
                .expect("sent")
                .push(header(&attempt, "traceparent"));
            Box::pin(async move { Ok(answer(&request, false)) })
        },
    );
    let list = env.call(SERVICE, "ListWidgets").list(
        Idempotency::NoSideEffects,
        PageReq {
            cursor: String::new(),
        },
        pager,
        PageOptions {
            prefetch: 0,
            ..PageOptions::default()
        },
    );
    while list.next().await.expect("a page or the end").is_some() {}
    assert_eq!(
        sink.starts.lock().expect("sink").len(),
        1,
        "only the walk's own span crossed"
    );
    assert_eq!(sink.ends.lock().expect("sink").len(), 1);
    let trace = format!("{:032x}", 0xabcd_u128);
    let sent = sent.lock().expect("sent");
    assert_eq!(sent.len(), 2);
    assert!(
        sent.iter().all(|traceparent| traceparent
            .as_deref()
            .and_then(|text| text.split('-').nth(1))
            == Some(trace.as_str())),
        "every page continues the walk's trace: {sent:?}"
    );
}
