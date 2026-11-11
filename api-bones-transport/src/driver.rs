// SPDX-License-Identifier: MIT
//! The process runtime that drives every connection the transport opens.
//!
//! Connection tasks, the connector (DNS, TCP, TLS), the keepalive and idle
//! timers and the request body pumps all run here, never on the caller's
//! runtime, so a connection outlives the runtime of the call that opened it.

use std::num::NonZeroUsize;
use std::sync::OnceLock;

use tokio::runtime::{Builder, Handle, Runtime};

/// Upper bound on the driver's worker threads: connections are I/O bound, and
/// the transport must not take a core per CPU from the service it serves.
const MAX_WORKERS: usize = 4;

/// The driver runtime's handle, starting the runtime on first use.
///
/// The runtime lives in a `static` and is never dropped, so it is safe to
/// reach from inside any other runtime.
///
/// # Panics
///
/// Panics when the operating system refuses the runtime's threads.
pub(crate) fn handle() -> &'static Handle {
    static DRIVER: OnceLock<Runtime> = OnceLock::new();
    DRIVER.get_or_init(start).handle()
}

fn start() -> Runtime {
    let workers = std::thread::available_parallelism()
        .map_or(1, NonZeroUsize::get)
        .min(MAX_WORKERS);
    Builder::new_multi_thread()
        .worker_threads(workers)
        .thread_name("api-bones-transport")
        .enable_all()
        .build()
        .expect("api-bones-transport: the operating system refused the driver runtime's threads")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_driver_is_one_runtime_for_the_process() {
        assert!(std::ptr::eq(handle(), handle()));
        assert!(handle().metrics().num_workers() <= MAX_WORKERS);
    }
}
