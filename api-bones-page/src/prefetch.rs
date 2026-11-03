// SPDX-License-Identifier: MIT
//! A page fetch running ahead of the consumer, aborted when dropped.

use tokio::task::JoinHandle;

/// A spawned fetch. Dropping it aborts the fetch, which drops the in-flight
/// request future and with it the transport call.
pub(crate) struct Prefetch<T>(JoinHandle<T>);

impl<T: Send + 'static> Prefetch<T> {
    pub(crate) fn spawn(fetch: impl Future<Output = T> + Send + 'static) -> Self {
        Self(tokio::spawn(fetch))
    }

    /// The fetch's result; `None` when the task was aborted or panicked.
    pub(crate) async fn finish(mut self) -> Option<T> {
        let handle = &mut self.0;
        handle.await.ok()
    }
}

impl<T> Drop for Prefetch<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}
