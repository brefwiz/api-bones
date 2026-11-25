// SPDX-License-Identifier: MIT
//! A page fetch running ahead of the consumer, aborted when dropped.

use api_bones_rt::{MaybeSend, Task, spawn};

/// A spawned fetch. Dropping it aborts the fetch, which drops the in-flight
/// request future and with it the transport call.
pub(crate) struct Prefetch<T>(Task<T>);

impl<T: MaybeSend + 'static> Prefetch<T> {
    pub(crate) fn spawn(fetch: impl Future<Output = T> + MaybeSend + 'static) -> Self {
        Self(spawn(fetch))
    }

    /// The fetch's result; `None` when the task was aborted or panicked.
    pub(crate) async fn finish(self) -> Option<T> {
        self.0.join().await
    }
}
