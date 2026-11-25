// SPDX-License-Identifier: MIT
//! The `wasm32` half of the seam: the host's event loop.
//!
//! Timers and the monotonic clock are read from `globalThis`, so the same
//! code runs in a window, a worker and Node. There is one thread, so nothing
//! here is `Send`.

use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};
use core::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use futures_channel::oneshot;
use futures_util::future::{AbortHandle, Abortable, Either, select};
use js_sys::{Function, Reflect};
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen::closure::Closure;

use crate::{Elapsed, MaybeSend};

/// The longest delay a host timer honours; a longer one fires at once.
const MAX_TIMER_MS: f64 = 2_147_483_647.0;

/// A function read from `globalThis`.
fn global_fn(name: &str) -> Function {
    Reflect::get(&js_sys::global(), &JsValue::from_str(name))
        .ok()
        .and_then(|value| value.dyn_into::<Function>().ok())
        .unwrap_or_else(|| panic!("globalThis.{name} is not a function in this host"))
}

/// A running task. Dropping it aborts the task; [`Task::detach`] lets it run
/// on without a handle.
#[derive(Debug)]
pub struct Task<T> {
    done: oneshot::Receiver<T>,
    abort: Option<AbortHandle>,
}

impl<T> Task<T> {
    /// Waits for the task and returns what it produced, or `None` when it
    /// was aborted.
    ///
    /// Dropping this future aborts the task, as dropping the [`Task`] does.
    pub async fn join(mut self) -> Option<T> {
        (&mut self.done).await.ok()
    }

    /// Lets the task run to completion with nothing left to join or abort it.
    pub fn detach(mut self) {
        self.abort = None;
    }
}

impl<T> Drop for Task<T> {
    fn drop(&mut self) {
        if let Some(abort) = self.abort.take() {
            abort.abort();
        }
    }
}

/// Runs `f` on the host's event loop.
pub fn spawn<F>(f: F) -> Task<F::Output>
where
    F: Future + MaybeSend + 'static,
    F::Output: MaybeSend + 'static,
{
    let (tx, done) = oneshot::channel();
    let (abort, registration) = AbortHandle::new_pair();
    wasm_bindgen_futures::spawn_local(async move {
        let run = async move {
            let _ = tx.send(f.await);
        };
        let _ = Abortable::new(run, registration).await;
    });
    Task {
        done,
        abort: Some(abort),
    }
}

/// A host timer that is cleared when dropped.
struct Timer {
    id: JsValue,
    fired: oneshot::Receiver<()>,
    _callback: Closure<dyn FnMut()>,
}

impl Timer {
    fn new(ms: f64) -> Self {
        let (tx, fired) = oneshot::channel();
        let callback: Closure<dyn FnMut()> = Closure::once(move || {
            let _ = tx.send(());
        });
        let id = global_fn("setTimeout")
            .call2(&js_sys::global(), callback.as_ref(), &JsValue::from_f64(ms))
            .unwrap_or_else(|e| panic!("setTimeout refused the timer: {e:?}"));
        Self {
            id,
            fired,
            _callback: callback,
        }
    }
}

impl Future for Timer {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        Pin::new(&mut self.fired).poll(cx).map(|_| ())
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        let _ = global_fn("clearTimeout").call1(&js_sys::global(), &self.id);
    }
}

/// Completes after `d`.
pub async fn sleep(d: Duration) {
    let mut remaining_ms = d.as_secs_f64() * 1000.0;
    loop {
        let step = remaining_ms.min(MAX_TIMER_MS);
        Timer::new(step).await;
        remaining_ms -= step;
        if remaining_ms <= 0.0 {
            break;
        }
    }
}

/// Completes at `t`, at once when `t` has passed.
pub async fn sleep_until(t: Instant) {
    sleep(t.saturating_duration_since(Instant::now())).await;
}

/// Runs `f` for at most `d`.
///
/// # Errors
///
/// Returns [`Elapsed`] when `d` passes before `f` finishes; `f` is dropped.
pub async fn timeout<F: Future>(d: Duration, f: F) -> Result<F::Output, Elapsed> {
    let f = core::pin::pin!(f);
    let deadline = core::pin::pin!(sleep(d));
    match select(f, deadline).await {
        Either::Left((output, _)) => Ok(output),
        Either::Right(((), _)) => Err(Elapsed),
    }
}

/// Hands the host one turn before continuing.
pub async fn yield_now() {
    struct Requeue(bool);

    impl Future for Requeue {
        type Output = ();

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
            if self.0 {
                return Poll::Ready(());
            }
            self.0 = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }

    Requeue(false).await;
}

/// The wall-clock time now.
#[must_use]
pub fn system_now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs_f64(js_sys::Date::now().max(0.0) / 1000.0)
}

/// Milliseconds on the host's monotonic clock.
fn performance_now_ms() -> f64 {
    let performance = Reflect::get(&js_sys::global(), &JsValue::from_str("performance"))
        .unwrap_or(JsValue::UNDEFINED);
    Reflect::get(&performance, &JsValue::from_str("now"))
        .ok()
        .and_then(|value| value.dyn_into::<Function>().ok())
        .and_then(|now| now.call0(&performance).ok())
        .and_then(|ms| ms.as_f64())
        .expect("globalThis.performance.now() is not available in this host")
}

/// A monotonic instant, read from `globalThis.performance.now()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Instant(Duration);

impl Instant {
    /// The current instant.
    #[must_use]
    pub fn now() -> Self {
        Self(Duration::from_secs_f64(
            performance_now_ms().max(0.0) / 1000.0,
        ))
    }

    /// The time since this instant.
    #[must_use]
    pub fn elapsed(self) -> Duration {
        Self::now().saturating_duration_since(self)
    }

    /// The time from `earlier` to this instant, zero when `earlier` is later.
    #[must_use]
    pub fn saturating_duration_since(self, earlier: Self) -> Duration {
        self.0.saturating_sub(earlier.0)
    }
}

instant_arithmetic!();
