// SPDX-License-Identifier: MIT
//! The native half of the seam: tokio, whose paused clock tests rely on.

use core::future::Future;
use core::time::Duration;
use std::time::SystemTime;

use crate::{Elapsed, MaybeSend};

/// A running task. Dropping it aborts the task; [`Task::detach`] lets it run
/// on without a handle.
#[derive(Debug)]
pub struct Task<T>(Option<tokio::task::JoinHandle<T>>);

impl<T> Task<T> {
    /// Waits for the task and returns what it produced, or `None` when it
    /// was aborted or panicked.
    ///
    /// Dropping this future aborts the task, as dropping the [`Task`] does.
    pub async fn join(mut self) -> Option<T> {
        let handle = self.0.as_mut()?;
        handle.await.ok()
    }

    /// Lets the task run to completion with nothing left to join or abort it.
    pub fn detach(mut self) {
        self.0 = None;
    }
}

impl<T> Drop for Task<T> {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            handle.abort();
        }
    }
}

/// Runs `f` as a task on the ambient tokio runtime.
///
/// # Panics
///
/// Panics when called outside a tokio runtime.
pub fn spawn<F>(f: F) -> Task<F::Output>
where
    F: Future + MaybeSend + 'static,
    F::Output: MaybeSend + 'static,
{
    Task(Some(tokio::spawn(f)))
}

/// Completes after `d`.
pub async fn sleep(d: Duration) {
    tokio::time::sleep(d).await;
}

/// Completes at `t`, at once when `t` has passed.
pub async fn sleep_until(t: Instant) {
    tokio::time::sleep_until(t.0).await;
}

/// Runs `f` for at most `d`.
///
/// # Errors
///
/// Returns [`Elapsed`] when `d` passes before `f` finishes; `f` is dropped.
pub async fn timeout<F: Future>(d: Duration, f: F) -> Result<F::Output, Elapsed> {
    tokio::time::timeout(d, f).await.map_err(|_| Elapsed)
}

/// Hands the executor one turn before continuing.
pub async fn yield_now() {
    tokio::task::yield_now().await;
}

/// The wall-clock time now.
#[must_use]
pub fn system_now() -> SystemTime {
    SystemTime::now()
}

/// A monotonic instant. Natively it is tokio's, so a paused clock moves it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Instant(tokio::time::Instant);

impl Instant {
    /// The current instant.
    #[must_use]
    pub fn now() -> Self {
        Self(tokio::time::Instant::now())
    }

    /// The time since this instant.
    #[must_use]
    pub fn elapsed(self) -> Duration {
        self.0.elapsed()
    }

    /// The time from `earlier` to this instant, zero when `earlier` is later.
    #[must_use]
    pub fn saturating_duration_since(self, earlier: Self) -> Duration {
        self.0.saturating_duration_since(earlier.0)
    }
}

instant_arithmetic!();

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    struct Flag(Arc<AtomicBool>);

    impl Drop for Flag {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn a_joined_task_returns_its_output() {
        assert_eq!(spawn(async { 41 + 1 }).join().await, Some(42));
    }

    #[tokio::test(start_paused = true)]
    async fn dropping_a_task_aborts_it() {
        let dropped = Arc::new(AtomicBool::new(false));
        let guard = Flag(Arc::clone(&dropped));
        let task = spawn(async move {
            let _guard = guard;
            sleep(Duration::from_secs(3600)).await;
        });
        yield_now().await;
        assert!(!dropped.load(Ordering::SeqCst));
        drop(task);
        sleep(Duration::from_millis(1)).await;
        assert!(dropped.load(Ordering::SeqCst));
    }

    #[tokio::test(start_paused = true)]
    async fn dropping_the_join_future_aborts_the_task() {
        let dropped = Arc::new(AtomicBool::new(false));
        let guard = Flag(Arc::clone(&dropped));
        let task = spawn(async move {
            let _guard = guard;
            sleep(Duration::from_secs(3600)).await;
        });
        let abandoned = timeout(Duration::from_millis(5), task.join()).await;
        assert_eq!(abandoned, Err(Elapsed));
        sleep(Duration::from_millis(1)).await;
        assert!(dropped.load(Ordering::SeqCst));
    }

    #[tokio::test(start_paused = true)]
    async fn a_detached_task_runs_to_completion() {
        let (tx, rx) = tokio::sync::oneshot::channel();
        spawn(async move {
            sleep(Duration::from_millis(10)).await;
            let _ = tx.send(7);
        })
        .detach();
        assert_eq!(rx.await, Ok(7));
    }

    #[tokio::test(start_paused = true)]
    async fn sleep_moves_the_paused_clock_by_its_duration() {
        let started = Instant::now();
        sleep(Duration::from_secs(5)).await;
        assert_eq!(started.elapsed(), Duration::from_secs(5));
    }

    #[tokio::test(start_paused = true)]
    async fn sleep_until_waits_for_the_instant_and_not_past_it() {
        let started = Instant::now();
        let target = started + Duration::from_secs(3);
        sleep_until(target).await;
        assert_eq!(Instant::now() - started, Duration::from_secs(3));
        sleep_until(started).await;
        assert_eq!(Instant::now() - started, Duration::from_secs(3));
    }

    #[tokio::test(start_paused = true)]
    async fn timeout_returns_the_output_before_the_deadline() {
        let done = timeout(Duration::from_secs(1), async {
            sleep(Duration::from_millis(10)).await;
            "done"
        })
        .await;
        assert_eq!(done, Ok("done"));
    }

    #[tokio::test(start_paused = true)]
    async fn timeout_reports_elapsed_after_the_deadline() {
        let late = timeout(Duration::from_millis(10), sleep(Duration::from_secs(1))).await;
        assert_eq!(late, Err(Elapsed));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn yield_now_lets_a_queued_task_run_first() {
        let ran = Arc::new(AtomicBool::new(false));
        let seen = Arc::clone(&ran);
        spawn(async move { seen.store(true, Ordering::SeqCst) }).detach();
        assert!(!ran.load(Ordering::SeqCst));
        yield_now().await;
        assert!(ran.load(Ordering::SeqCst));
    }

    #[tokio::test(start_paused = true)]
    async fn instant_arithmetic_is_consistent() {
        let a = Instant::now();
        sleep(Duration::from_secs(2)).await;
        let b = Instant::now();
        assert!(b > a);
        assert_eq!(b - a, Duration::from_secs(2));
        assert_eq!(a - b, Duration::ZERO);
        assert_eq!(b.saturating_duration_since(a), Duration::from_secs(2));
        assert_eq!(b - Duration::from_secs(2), a);
        assert_eq!(a.checked_add(Duration::from_secs(2)), Some(b));
        assert_eq!(b.checked_sub(Duration::from_secs(2)), Some(a));
        assert_eq!(a.checked_add(Duration::MAX), None);
        assert_eq!(a.checked_sub(Duration::MAX), None);
    }

    #[test]
    fn system_now_is_after_the_unix_epoch() {
        assert!(
            system_now()
                .duration_since(std::time::UNIX_EPOCH)
                .is_ok_and(|d| d > Duration::from_secs(1_700_000_000))
        );
    }
}
