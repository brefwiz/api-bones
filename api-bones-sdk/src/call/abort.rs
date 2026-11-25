// SPDX-License-Identifier: MIT
//! Abandoning a call from outside.
//!
//! The browser binding gives every async call an `AbortSignal`. It becomes an
//! [`AbortFlag`]: when it fires, the call future is dropped (the transport
//! aborts its fetch on drop) and the call fails with `canceled`. Natively a
//! caller aborts a call by dropping its future; there is no token type, though
//! a flag works there too.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll, Waker};

use api_bones_rt::BoxStream;
use futures_util::StreamExt as _;
use futures_util::future::{Either, select};
use futures_util::stream::unfold;

use crate::error::Error;

#[derive(Default)]
struct State {
    fired: bool,
    wakers: Vec<Waker>,
}

fn locked(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

/// An abort that has or has not happened. Clones share one state.
#[derive(Clone, Default)]
pub struct AbortFlag {
    state: Arc<Mutex<State>>,
    #[cfg(all(feature = "wasm", target_arch = "wasm32"))]
    listener: Option<Arc<signal::Listener>>,
}

impl AbortFlag {
    /// A flag that has not fired.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Fires the flag. Safe to call again.
    pub fn abort(&self) {
        let wakers = {
            let mut state = locked(&self.state);
            state.fired = true;
            std::mem::take(&mut state.wakers)
        };
        for waker in wakers {
            waker.wake();
        }
    }

    /// Whether the flag has fired.
    #[must_use]
    pub fn is_aborted(&self) -> bool {
        locked(&self.state).fired
    }

    /// A future that is ready once the flag has fired.
    #[must_use]
    pub fn aborted(&self) -> Aborted {
        Aborted(Arc::clone(&self.state))
    }
}

/// Ready once its [`AbortFlag`] has fired.
pub struct Aborted(Arc<Mutex<State>>);

impl Future for Aborted {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let mut state = locked(&self.0);
        if state.fired {
            return Poll::Ready(());
        }
        if !state.wakers.iter().any(|waker| waker.will_wake(cx.waker())) {
            state.wakers.push(cx.waker().clone());
        }
        Poll::Pending
    }
}

/// `stream`, whose next item is `canceled` once `abort` has fired. The handle
/// that carries it finishes on that error, as it does for `cancel()`.
pub(crate) fn abortable<T: api_bones_rt::MaybeSend + 'static>(
    stream: BoxStream<'static, Result<T, Error>>,
    abort: AbortFlag,
) -> BoxStream<'static, Result<T, Error>> {
    Box::pin(unfold((stream, abort), |(mut stream, abort)| async move {
        let step = match select(abort.aborted(), stream.next()).await {
            Either::Left(_) => None,
            Either::Right((item, _)) => Some(item),
        };
        match step {
            None => Some((Err(Error::canceled()), (stream, abort))),
            Some(item) => item.map(|item| (item, (stream, abort))),
        }
    }))
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod signal {
    use super::AbortFlag;
    use std::sync::Arc;
    use wasm_bindgen::JsCast as _;
    use wasm_bindgen::closure::Closure;

    /// The `abort` listener on a signal, removed when the flag is dropped.
    pub(super) struct Listener {
        signal: web_sys::AbortSignal,
        closure: Closure<dyn FnMut()>,
    }

    impl Drop for Listener {
        fn drop(&mut self) {
            // A signal that refuses the removal has nothing left to leak.
            let _ = self.signal.remove_event_listener_with_callback(
                "abort",
                self.closure.as_ref().unchecked_ref(),
            );
        }
    }

    impl AbortFlag {
        /// A flag that fires when `signal` aborts, or already has when
        /// `signal` is aborted already. With no signal it never fires on its
        /// own.
        #[must_use]
        pub fn from_signal(signal: Option<&web_sys::AbortSignal>) -> Self {
            let mut flag = Self::new();
            let Some(signal) = signal else {
                return flag;
            };
            if signal.aborted() {
                flag.abort();
                return flag;
            }
            let target = flag.clone();
            let closure = Closure::<dyn FnMut()>::new(move || target.abort());
            if signal
                .add_event_listener_with_callback("abort", closure.as_ref().unchecked_ref())
                .is_ok()
            {
                flag.listener = Some(Arc::new(Listener {
                    signal: signal.clone(),
                    closure,
                }));
            }
            flag
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use futures_util::FutureExt as _;
    use futures_util::stream;

    #[test]
    fn a_flag_that_has_not_fired_is_pending_and_one_that_has_is_ready() {
        let flag = AbortFlag::new();
        assert!(!flag.is_aborted());
        assert!(flag.aborted().now_or_never().is_none());
        flag.abort();
        flag.abort();
        assert!(flag.is_aborted());
        assert!(flag.aborted().now_or_never().is_some());
        assert!(flag.clone().aborted().now_or_never().is_some());
    }

    #[tokio::test]
    async fn firing_wakes_every_waiter() {
        let flag = AbortFlag::new();
        let waiters = [flag.aborted(), flag.aborted()];
        let all = tokio::spawn(async move {
            for waiter in waiters {
                waiter.await;
            }
        });
        tokio::task::yield_now().await;
        flag.abort();
        all.await.expect("both waiters woke");
    }

    #[tokio::test]
    async fn a_stream_delivers_until_the_abort_and_then_says_canceled() {
        let flag = AbortFlag::new();
        let items: BoxStream<'static, Result<u32, Error>> =
            Box::pin(stream::iter([Ok(1), Ok(2)]).chain(stream::pending()));
        let mut gated = abortable(items, flag.clone());
        assert_eq!(gated.next().await.expect("an item").expect("ok"), 1);
        assert_eq!(gated.next().await.expect("an item").expect("ok"), 2);
        flag.abort();
        let refusal = gated
            .next()
            .await
            .expect("the abort")
            .expect_err("canceled");
        assert_eq!(refusal.code, crate::error::CANCELED);
    }

    #[tokio::test]
    async fn a_stream_that_runs_out_ends() {
        let items: BoxStream<'static, Result<u32, Error>> = Box::pin(stream::iter([Ok(1)]));
        let mut gated = abortable(items, AbortFlag::new());
        assert_eq!(gated.next().await.expect("an item").expect("ok"), 1);
        assert!(gated.next().await.is_none());
    }
}
