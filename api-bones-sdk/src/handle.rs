// SPDX-License-Identifier: MIT
//! The handle a call returns when its result arrives over time.
//!
//! A list, a stream and a batch are all a [`Handle`]: a source of results with
//! `next()`, `cancel()` and `span()`. The contract is the same for each:
//!
//! - `next()` after the end gives `None`, as often as it is asked.
//! - A `next()` that starts while another is pending is refused with
//!   `HANDLE_BUSY`; the pending one is unaffected.
//! - `cancel()` is idempotent. It abandons a pending `next()`, which fails with
//!   `canceled`, and every `next()` after it gives `None`.
//! - Dropping a pending `next()` abandons the walk the same way as `cancel()`.
//! - `span()` is the `traceparent` of the span covering the whole walk, or
//!   `None` when that span is not recording.
//!
//! The walk span ends exactly once: `ok` when the source runs out, `error`
//! when it fails, `unset` when the walk is abandoned.

use std::fmt;
use std::sync::{Mutex, MutexGuard, PoisonError};

use api_bones_page::{ListCall as PageWalk, Page, PagedRequest, PagedResponse};
use api_bones_rt::{BoxStream, MaybeSend, MaybeSync};
use futures_util::StreamExt as _;
use futures_util::future::{AbortHandle, Abortable};

use crate::error::Error;
use crate::span::{ActiveSpan, SpanStatus};

/// A list: one [`Page`] per `next()`.
pub type ListCall<T> = Handle<Page<T>>;

/// A stream: one item per `next()`.
pub type Stream<T> = Handle<T>;

/// A batch: the next delivered chunk per `next()`, in request order.
pub type Batch<T> = Handle<Vec<Result<T, Error>>>;

/// A source that has not run out, and the span of its walk.
struct Live<T> {
    stream: BoxStream<'static, Result<T, Error>>,
    span: ActiveSpan,
}

impl<T> Live<T> {
    /// Ends the walk. The source is released before its span ends.
    fn finish(self, status: SpanStatus, error: Option<&Error>) {
        let Self { stream, span } = self;
        drop(stream);
        match error {
            Some(error) => span.fail(error),
            None => span.end(status),
        }
    }
}

enum Phase<T> {
    /// Waiting for a `next()`.
    Idle(Live<T>),
    /// A `next()` is pending and holds the source.
    Busy,
    /// Ran out, failed or was abandoned.
    Finished,
}

struct Inner<T> {
    phase: Phase<T>,
    /// Aborts the pending `next()`.
    abort: Option<AbortHandle>,
    canceled: bool,
}

fn locked<T>(inner: &Mutex<Inner<T>>) -> MutexGuard<'_, Inner<T>> {
    inner.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Frees a handle whose pending `next()` was dropped before it finished.
struct Release<'a, T>(&'a Mutex<Inner<T>>);

impl<T> Drop for Release<'_, T> {
    fn drop(&mut self) {
        let mut inner = locked(self.0);
        inner.abort = None;
        if matches!(inner.phase, Phase::Busy) {
            inner.phase = Phase::Finished;
        }
    }
}

/// A source of results with `next()`, `cancel()` and `span()`.
pub struct Handle<T> {
    inner: Mutex<Inner<T>>,
    span: Option<String>,
}

impl<T> fmt::Debug for Handle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Handle")
            .field("span", &self.span)
            .finish_non_exhaustive()
    }
}

impl<T: MaybeSend + 'static> Handle<T> {
    /// A handle over `stream`, whose walk `span` ends with the handle.
    #[must_use]
    pub fn new(stream: BoxStream<'static, Result<T, Error>>, span: ActiveSpan) -> Self {
        Self {
            span: span.recorded_traceparent(),
            inner: Mutex::new(Inner {
                phase: Phase::Idle(Live { stream, span }),
                abort: None,
                canceled: false,
            }),
        }
    }

    /// The next result; `None` once the source has run out or the handle was
    /// canceled.
    ///
    /// # Errors
    /// `HANDLE_BUSY` when another `next()` is pending, `canceled` when this
    /// one was abandoned by `cancel()`, and the source's own failure, after
    /// which the handle is finished.
    pub async fn next(&self) -> Result<Option<T>, Error> {
        let (mut live, registration) = {
            let mut inner = locked(&self.inner);
            match std::mem::replace(&mut inner.phase, Phase::Busy) {
                Phase::Finished => {
                    inner.phase = Phase::Finished;
                    return Ok(None);
                }
                Phase::Busy => return Err(Error::handle_busy()),
                Phase::Idle(live) => {
                    let (abort, registration) = AbortHandle::new_pair();
                    inner.abort = Some(abort);
                    (live, registration)
                }
            }
        };
        let _release = Release(&self.inner);
        let outcome = Abortable::new(
            async move {
                let item = live.stream.next().await;
                (live, item)
            },
            registration,
        )
        .await;
        let mut inner = locked(&self.inner);
        inner.abort = None;
        let Ok((live, item)) = outcome else {
            inner.phase = Phase::Finished;
            return Err(Error::canceled());
        };
        let canceled = inner.canceled;
        match item {
            Some(Ok(value)) if !canceled => {
                inner.phase = Phase::Idle(live);
                Ok(Some(value))
            }
            Some(Err(error)) => {
                inner.phase = Phase::Finished;
                drop(inner);
                live.finish(SpanStatus::Error, Some(&error));
                Err(error)
            }
            _ => {
                inner.phase = Phase::Finished;
                drop(inner);
                live.finish(
                    if canceled {
                        SpanStatus::Unset
                    } else {
                        SpanStatus::Ok
                    },
                    None,
                );
                if canceled {
                    Err(Error::canceled())
                } else {
                    Ok(None)
                }
            }
        }
    }

    /// Abandons the walk. A pending `next()` fails with `canceled`; every
    /// later one gives `None`. Safe to call again.
    pub fn cancel(&self) {
        let mut inner = locked(&self.inner);
        inner.canceled = true;
        if let Some(abort) = inner.abort.take() {
            abort.abort();
        }
        let idle = if matches!(inner.phase, Phase::Idle(_)) {
            Some(std::mem::replace(&mut inner.phase, Phase::Finished))
        } else {
            None
        };
        drop(inner);
        drop(idle);
    }

    /// The `traceparent` of the walk's span; `None` when it is not recording.
    #[must_use]
    pub fn span(&self) -> Option<String> {
        self.span.clone()
    }
}

impl<Item: MaybeSend + MaybeSync + 'static> Handle<Page<Item>> {
    /// A list handle that walks the pages of `call`.
    #[must_use]
    pub fn pages<Req, Resp>(call: PageWalk<Req, Resp>, span: ActiveSpan) -> Self
    where
        Req: PagedRequest,
        Resp: PagedResponse<Item = Item>,
    {
        Self::new(
            Box::pin(call.pages().map(|page| page.map_err(Error::from))),
            span,
        )
    }
}

impl<Item: MaybeSend + MaybeSync + Clone + 'static> Handle<Item> {
    /// A stream handle that walks every item of `call`.
    #[must_use]
    pub fn items<Req, Resp>(call: PageWalk<Req, Resp>, span: ActiveSpan) -> Self
    where
        Req: PagedRequest,
        Resp: PagedResponse<Item = Item>,
    {
        Self::new(
            Box::pin(call.items().map(|item| item.map_err(Error::from))),
            span,
        )
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::task::{Context, Poll};

    use api_bones::connect::Idempotency;
    use api_bones_page::{Fetch, PageInfo};
    use connectrpc::ConnectError;
    use futures_util::FutureExt as _;
    use futures_util::stream;
    use futures_util::task::noop_waker_ref;

    use crate::ctx::CallCtx;
    use crate::span::{Attributes, SpanEnd, SpanKind, SpanSink, SpanStart, Tracer};

    const WALK: &str = "00-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-bbbbbbbbbbbbbbbb-01";

    #[derive(Default)]
    struct Sink {
        ends: Mutex<Vec<SpanEnd>>,
    }

    impl SpanSink for Sink {
        fn start(&self, _: &SpanStart) -> Option<String> {
            Some(WALK.to_owned())
        }

        fn end(&self, end: &SpanEnd) {
            self.ends.lock().unwrap().push(end.clone());
        }
    }

    impl Sink {
        fn statuses(&self) -> Vec<SpanStatus> {
            self.ends
                .lock()
                .unwrap()
                .iter()
                .map(|end| end.status)
                .collect()
        }
    }

    fn walk(sink: &Arc<Sink>) -> ActiveSpan {
        Tracer::new(Arc::<Sink>::clone(sink)).start(
            "walk",
            SpanKind::Internal,
            &CallCtx::default(),
            Attributes::new(),
        )
    }

    fn handle_of(results: Vec<Result<u32, Error>>, sink: &Arc<Sink>) -> Handle<u32> {
        Handle::new(Box::pin(stream::iter(results)), walk(sink))
    }

    fn poll_once<F: Future>(future: &mut Pin<Box<F>>) -> Poll<F::Output> {
        future
            .as_mut()
            .poll(&mut Context::from_waker(noop_waker_ref()))
    }

    #[tokio::test]
    async fn a_handle_yields_its_results_then_none_forever() {
        let sink = Arc::new(Sink::default());
        let handle = handle_of(vec![Ok(1), Ok(2)], &sink);
        assert_eq!(handle.next().await, Ok(Some(1)));
        assert_eq!(handle.next().await, Ok(Some(2)));
        assert!(sink.statuses().is_empty(), "the walk is still open");
        assert_eq!(handle.next().await, Ok(None));
        assert_eq!(handle.next().await, Ok(None));
        assert_eq!(sink.statuses(), vec![SpanStatus::Ok], "the span ends once");
    }

    #[tokio::test]
    async fn a_failure_ends_the_walk_and_its_span_as_an_error() {
        let sink = Arc::new(Sink::default());
        let handle = handle_of(vec![Ok(1), Err(Error::internal("boom")), Ok(3)], &sink);
        assert_eq!(handle.next().await, Ok(Some(1)));
        assert_eq!(handle.next().await, Err(Error::internal("boom")));
        assert_eq!(handle.next().await, Ok(None));
        let ends = sink.ends.lock().unwrap();
        assert_eq!(ends.len(), 1);
        assert_eq!(ends[0].status, SpanStatus::Error);
        assert_eq!(
            ends[0].error.as_deref(),
            Some("internal INTERNAL_SERVER_ERROR")
        );
    }

    #[tokio::test]
    async fn an_overlapping_next_is_refused_and_cancel_abandons_the_pending_one() {
        let sink = Arc::new(Sink::default());
        let handle: Handle<u32> = Handle::new(Box::pin(stream::pending()), walk(&sink));
        let mut first = Box::pin(handle.next());
        assert!(poll_once(&mut first).is_pending());
        assert_eq!(
            handle.next().now_or_never(),
            Some(Err(Error::handle_busy()))
        );
        assert!(
            poll_once(&mut first).is_pending(),
            "the refusal left it alone"
        );
        handle.cancel();
        handle.cancel();
        assert_eq!(poll_once(&mut first), Poll::Ready(Err(Error::canceled())));
        assert_eq!(handle.next().await, Ok(None));
        assert_eq!(sink.statuses(), vec![SpanStatus::Unset]);
    }

    #[tokio::test]
    async fn a_dropped_next_abandons_the_walk() {
        let sink = Arc::new(Sink::default());
        let handle: Handle<u32> = Handle::new(Box::pin(stream::pending()), walk(&sink));
        let mut pending = Box::pin(handle.next());
        assert!(poll_once(&mut pending).is_pending());
        drop(pending);
        assert_eq!(
            handle.next().await,
            Ok(None),
            "no HANDLE_BUSY after the drop"
        );
        assert_eq!(sink.statuses(), vec![SpanStatus::Unset]);
    }

    #[tokio::test]
    async fn cancel_between_results_is_idempotent_and_ends_the_span_unset() {
        let sink = Arc::new(Sink::default());
        let handle = handle_of(vec![Ok(1), Ok(2)], &sink);
        assert_eq!(handle.next().await, Ok(Some(1)));
        handle.cancel();
        handle.cancel();
        assert_eq!(handle.next().await, Ok(None));
        assert_eq!(sink.statuses(), vec![SpanStatus::Unset]);
    }

    #[test]
    fn span_is_the_walk_traceparent_when_recording() {
        let sink = Arc::new(Sink::default());
        let handle = handle_of(vec![], &sink);
        assert_eq!(handle.span().as_deref(), Some(WALK));
        assert!(format!("{handle:?}").contains(WALK));
        let silent: Handle<u32> = Handle::new(
            Box::pin(stream::empty()),
            Tracer::default().start(
                "walk",
                SpanKind::Internal,
                &CallCtx::default(),
                Attributes::new(),
            ),
        );
        assert_eq!(silent.span(), None);
    }

    #[derive(Clone)]
    struct Request {
        cursor: String,
    }

    impl PagedRequest for Request {
        fn set_page(&mut self, cursor: &str, _: Option<u32>) {
            cursor.clone_into(&mut self.cursor);
        }
    }

    struct Response {
        items: Vec<u32>,
        next: &'static str,
    }

    impl PagedResponse for Response {
        type Item = u32;

        fn split(self) -> (Vec<u32>, PageInfo) {
            let info = PageInfo {
                next_cursor: self.next.to_owned(),
                revision: "r1".to_owned(),
                ..PageInfo::default()
            };
            (self.items, info)
        }
    }

    fn list(fail_on_second: bool) -> PageWalk<Request, Response> {
        let fetch: Fetch<Request, Response> = Arc::new(move |request: Request| {
            Box::pin(async move {
                match request.cursor.as_str() {
                    "" => Ok(Response {
                        items: vec![1, 2],
                        next: "c2",
                    }),
                    _ if fail_on_second => Err(ConnectError::internal("page two failed")),
                    _ => Ok(Response {
                        items: vec![3],
                        next: "",
                    }),
                }
            })
        });
        PageWalk::new(
            "/contract.v1.Numbers/List",
            Idempotency::NoSideEffects,
            fetch,
            Request {
                cursor: String::new(),
            },
        )
    }

    #[tokio::test]
    async fn a_list_handle_yields_one_page_per_next() {
        let sink = Arc::new(Sink::default());
        let handle: ListCall<u32> = Handle::pages(list(false), walk(&sink));
        let first = handle.next().await.unwrap().unwrap();
        assert_eq!(&*first.items, &[1, 2]);
        assert!(first.has_next);
        assert_eq!(first.revision, "r1");
        let second = handle.next().await.unwrap().unwrap();
        assert_eq!(&*second.items, &[3]);
        assert!(!second.has_next);
        assert!(handle.next().await.unwrap().is_none());
        assert_eq!(sink.statuses(), vec![SpanStatus::Ok]);
    }

    #[tokio::test]
    async fn a_list_failure_names_the_pages_delivered_before_it() {
        let sink = Arc::new(Sink::default());
        let handle: ListCall<u32> = Handle::pages(list(true), walk(&sink));
        handle.next().await.unwrap().unwrap();
        let error = handle.next().await.unwrap_err();
        assert_eq!(error.platform, "INTERNAL_SERVER_ERROR");
        let page = error.page.unwrap();
        assert_eq!(page.delivered_pages, 1);
        assert_eq!(page.delivered_items, 2);
        assert_eq!(sink.statuses(), vec![SpanStatus::Error]);
    }

    #[tokio::test]
    async fn a_stream_handle_yields_one_item_per_next() {
        let sink = Arc::new(Sink::default());
        let handle: Stream<u32> = Handle::items(list(false), walk(&sink));
        let mut seen = Vec::new();
        while let Some(item) = handle.next().await.unwrap() {
            seen.push(item);
        }
        assert_eq!(seen, vec![1, 2, 3]);
    }
}
