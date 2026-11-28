// SPDX-License-Identifier: MIT
//! The call context a Layer 2 method runs under.
//!
//! A Layer 2 method takes no context parameter. The caller's trace fields and
//! the prefetch it chose for a list are installed around the call's future with
//! [`CallCtx::scope`] (or [`CallCtx::with`] for a call that starts without
//! awaiting), and the call helper reads them once, synchronously, when the
//! call starts.

use std::cell::RefCell;
use std::future::{Future, poll_fn};
use std::pin::pin;

/// The caller's W3C trace fields and the prefetch or first-page read it chose for a list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CallCtx {
    /// The caller's `traceparent`; absent for a call that starts a new trace.
    pub traceparent: Option<String>,
    /// The caller's `tracestate`.
    pub tracestate: Option<String>,
    /// The pages a list fetches ahead of its consumer: 0 reads none, any
    /// larger value reads one ahead; absent leaves the Layer 2 method's own
    /// choice.
    pub prefetch: Option<u8>,
    /// A list delivers its first page only: one request, `next_cursor` never
    /// followed, nothing fetched ahead.
    pub first_only: bool,
}

thread_local! {
    static CURRENT: RefCell<Option<CallCtx>> = const { RefCell::new(None) };
}

/// Puts the previous context back when a poll of a scoped future ends, by
/// returning, by pending or by panicking.
struct Restore(Option<CallCtx>);

impl Drop for Restore {
    fn drop(&mut self) {
        CURRENT.with(|current| *current.borrow_mut() = self.0.take());
    }
}

impl CallCtx {
    /// The context installed by the innermost [`CallCtx::scope`] being polled
    /// on this thread; an empty context outside any scope.
    #[must_use]
    pub fn current() -> Self {
        CURRENT.with(|current| current.borrow().clone().unwrap_or_default())
    }

    /// `future`, with `ctx` as [`CallCtx::current`] whenever it is polled.
    ///
    /// The context is installed for the duration of each poll only, so two
    /// scoped futures interleaved on one thread each see their own.
    pub async fn scope<F: Future>(ctx: Self, future: F) -> F::Output {
        let mut future = pin!(future);
        poll_fn(move |task| Self::with(ctx.clone(), || future.as_mut().poll(task))).await
    }

    /// `run`, with `ctx` as [`CallCtx::current`] for as long as it runs.
    ///
    /// For a call that starts synchronously, such as a list: the call reads
    /// the context when it starts, so the context only has to be installed
    /// around that start.
    #[must_use]
    pub fn with<R>(ctx: Self, run: impl FnOnce() -> R) -> R {
        let previous = CURRENT.with(|current| current.borrow_mut().replace(ctx));
        let _restore = Restore(previous);
        run()
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use futures_util::future::join;

    fn ctx(parent: &str) -> CallCtx {
        CallCtx {
            traceparent: Some(parent.to_owned()),
            tracestate: Some(format!("k={parent}")),
            prefetch: None,
            first_only: false,
        }
    }

    #[test]
    fn outside_a_scope_the_context_is_empty() {
        assert_eq!(CallCtx::current(), CallCtx::default());
        assert_eq!(CallCtx::current().traceparent, None);
        assert_eq!(CallCtx::current().tracestate, None);
    }

    #[test]
    fn a_synchronous_run_sees_its_context_and_gives_the_previous_one_back() {
        let seen = CallCtx::with(ctx("outer"), || {
            let inner = CallCtx::with(ctx("inner"), CallCtx::current);
            (inner, CallCtx::current())
        });
        assert_eq!(seen, (ctx("inner"), ctx("outer")));
        assert_eq!(CallCtx::current(), CallCtx::default());
    }

    #[test]
    fn a_panicking_run_gives_the_previous_context_back() {
        let panicked = std::panic::catch_unwind(|| {
            let _ran = CallCtx::with(ctx("a"), || -> u8 { panic!("the run failed") });
        });
        assert!(panicked.is_err());
        assert_eq!(CallCtx::current(), CallCtx::default());
    }

    #[tokio::test]
    async fn a_scope_installs_its_context_for_the_future() {
        let seen = CallCtx::scope(ctx("a"), async { CallCtx::current() }).await;
        assert_eq!(seen, ctx("a"));
        assert_eq!(CallCtx::current(), CallCtx::default());
    }

    #[tokio::test]
    async fn the_context_survives_a_suspension_of_the_future() {
        let seen = CallCtx::scope(ctx("a"), async {
            tokio::task::yield_now().await;
            CallCtx::current()
        })
        .await;
        assert_eq!(seen, ctx("a"));
    }

    #[tokio::test]
    async fn interleaved_scopes_each_see_their_own_context() {
        let read_twice = |name: &'static str| {
            CallCtx::scope(ctx(name), async move {
                let before = CallCtx::current();
                tokio::task::yield_now().await;
                (before, CallCtx::current())
            })
        };
        let (a, b) = join(read_twice("a"), read_twice("b")).await;
        assert_eq!(a, (ctx("a"), ctx("a")));
        assert_eq!(b, (ctx("b"), ctx("b")));
        assert_eq!(CallCtx::current(), CallCtx::default());
    }

    #[tokio::test]
    async fn an_inner_scope_shadows_the_outer_and_gives_it_back() {
        let (inner, after) = CallCtx::scope(ctx("outer"), async {
            let inner = CallCtx::scope(ctx("inner"), async { CallCtx::current() }).await;
            (inner, CallCtx::current())
        })
        .await;
        assert_eq!(inner, ctx("inner"));
        assert_eq!(after, ctx("outer"));
    }

    #[tokio::test]
    async fn the_prefetch_a_caller_chose_travels_with_the_trace_fields() {
        let chosen = CallCtx {
            prefetch: Some(0),
            ..ctx("a")
        };
        let seen = CallCtx::scope(chosen.clone(), async { CallCtx::current() }).await;
        assert_eq!(seen, chosen);
        assert_eq!(seen.prefetch, Some(0));
        assert_eq!(CallCtx::current().prefetch, None);
    }

    #[test]
    fn a_panic_in_a_scoped_future_does_not_leak_its_context() {
        let outcome = std::panic::catch_unwind(|| {
            let scoped = CallCtx::scope(ctx("a"), async { panic!("boom") });
            futures_util::FutureExt::now_or_never(scoped)
        });
        assert!(outcome.is_err());
        assert_eq!(CallCtx::current(), CallCtx::default());
    }
}
