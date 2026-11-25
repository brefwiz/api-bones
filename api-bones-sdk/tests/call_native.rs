// SPDX-License-Identifier: MIT
//! The call helper on native targets: the call budget, the identity checks,
//! the trace context, the spans and the abort, driven through its public API.
//!
//! The fetch closures here stand in for the generated client's one RPC; the
//! helper's own behaviour (what it hands each send, what it enforces around
//! it) is what is under test. The same helper against a real server is the
//! Gherkin lane of `tests/rust`.

#![cfg(not(target_arch = "wasm32"))]

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use api_bones::connect::Idempotency;
use api_bones_batch::{Credential, Identity};
use api_bones_page::{PageInfo, PageOptions, PagedRequest, PagedResponse, RevisionCache};
use api_bones_rt::BoxFuture;
use api_bones_sdk::call::{AbortFlag, Attempt, CALL_BUDGET, CallEnv};
use api_bones_sdk::{
    CALL_BUDGET_ELAPSED, CANCELED, CallCtx, Error, IDENTITY_CHANGED, SpanEnd, SpanKind, SpanSink,
    SpanStart, SpanStatus, Traceparent, Tracer,
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
    CallEnv::new(move || {
        let n = count.fetch_add(1, Ordering::SeqCst);
        Ok(credential(if n < reads { "a" } else { "b" }, n))
    })
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
    fn start(&self, span: &SpanStart) -> Option<String> {
        let mut starts = self.starts.lock().expect("sink");
        starts.push(span.clone());
        Some(format!(
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
            seen.push((attempt.deadline(), attempt.deadline().remaining()));
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
    let refusal = CallEnv::new(|| Err::<Credential, _>(ConnectError::unavailable("no token")))
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

struct PageResp {
    items: Vec<u32>,
    next: &'static str,
}

impl PagedResponse for PageResp {
    type Item = u32;

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

/// A two-page list whose every page takes `each` to arrive.
fn two_pages(
    each: Duration,
    deadlines: Arc<Mutex<Vec<api_bones_transport::Deadline>>>,
) -> api_bones_sdk::call::PageSend<PageReq, PageResp> {
    Arc::new(
        move |(request, attempt): (PageReq, Attempt)| -> BoxFuture<'static, _> {
            deadlines
                .lock()
                .expect("deadlines")
                .push(attempt.deadline());
            Box::pin(async move {
                tokio::time::sleep(each).await;
                Ok(if request.cursor.is_empty() {
                    PageResp {
                        items: vec![1],
                        next: "p2",
                    }
                } else {
                    PageResp {
                        items: vec![2],
                        next: "",
                    }
                })
            })
        },
    )
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
    let env = CallEnv::new(move || {
        let n = count.fetch_add(1, Ordering::SeqCst);
        Ok(credential(
            if flag.load(Ordering::SeqCst) {
                "b"
            } else {
                "a"
            },
            n,
        ))
    });
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
