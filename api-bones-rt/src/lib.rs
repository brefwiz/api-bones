// SPDX-License-Identifier: MIT
//! The runtime seam of the api-bones SDK crates.
//!
//! The target decides the runtime, so nothing here is injected or configured:
//!
//! - on a native target the seam is tokio (`rt` and `time`), and tokio's paused
//!   clock drives [`sleep`], [`sleep_until`], [`timeout`] and [`Instant`];
//! - on `wasm32` it is the host's event loop: tasks run through
//!   `wasm_bindgen_futures::spawn_local` and timers are read from
//!   `globalThis`, so the same code runs in a window, a worker and Node.
//!
//! A browser has one thread, so a future on `wasm32` is never `Send`. The
//! [`MaybeSend`] and [`MaybeSync`] bounds, the [`BoxFuture`], [`BoxStream`] and
//! [`SharedFn`] aliases are the one place that difference is spelled: they
//! demand `Send` natively and nothing on `wasm32`. A `dyn` type admits only
//! one non-auto trait, so a boxed value uses the alias, while an `impl Trait`
//! or a generic bound uses the marker trait.
//!
//! This crate depends on no `api-bones` major, so a consumer on any major can
//! use the seam.
//!
//! `wasm32` with `target_feature = "atomics"` is refused at compile time: the
//! single-thread guarantee the non-`Send` aliases rest on does not hold there.

#[cfg(all(target_arch = "wasm32", target_feature = "atomics"))]
compile_error!(
    "api-bones-rt supports wasm32 only without the `atomics` target feature: \
     its non-Send futures assume one thread"
);

use core::fmt;
use core::future::Future;
use core::pin::Pin;
use std::sync::Arc;

/// Implements, for the `Instant` of the module it is invoked in, the checked
/// and operator arithmetic. The inner value must support the same operations
/// and the type must define `saturating_duration_since`.
macro_rules! instant_arithmetic {
    () => {
        impl Instant {
            /// This instant plus `d`, or `None` on overflow.
            #[must_use]
            pub fn checked_add(self, d: core::time::Duration) -> Option<Self> {
                self.0.checked_add(d).map(Self)
            }

            /// This instant minus `d`, or `None` on underflow.
            #[must_use]
            pub fn checked_sub(self, d: core::time::Duration) -> Option<Self> {
                self.0.checked_sub(d).map(Self)
            }
        }

        impl core::ops::Add<core::time::Duration> for Instant {
            type Output = Self;

            fn add(self, d: core::time::Duration) -> Self {
                Self(self.0 + d)
            }
        }

        impl core::ops::Sub<core::time::Duration> for Instant {
            type Output = Self;

            fn sub(self, d: core::time::Duration) -> Self {
                Self(self.0 - d)
            }
        }

        impl core::ops::Sub for Instant {
            type Output = core::time::Duration;

            fn sub(self, earlier: Self) -> core::time::Duration {
                self.saturating_duration_since(earlier)
            }
        }
    };
}

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub use native::{Instant, Task, sleep, sleep_until, spawn, system_now, timeout, yield_now};

#[cfg(target_arch = "wasm32")]
mod wasm;
#[cfg(target_arch = "wasm32")]
pub use wasm::{Instant, Task, sleep, sleep_until, spawn, system_now, timeout, yield_now};

/// A value that may cross threads where the target has threads.
///
/// Natively this is every `Send` type, and `Send` is a supertrait, so generic
/// code bounded by it can box into a [`BoxFuture`]. On `wasm32` it is every
/// type.
#[cfg(not(target_arch = "wasm32"))]
pub trait MaybeSend: Send {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Send + ?Sized> MaybeSend for T {}

/// A value that may cross threads where the target has threads.
///
/// Natively this is every `Send` type. On `wasm32` it is every type.
#[cfg(target_arch = "wasm32")]
pub trait MaybeSend {}
#[cfg(target_arch = "wasm32")]
impl<T: ?Sized> MaybeSend for T {}

/// A value that may be shared across threads where the target has threads.
///
/// Natively this is every `Sync` type, and `Sync` is a supertrait. On `wasm32`
/// it is every type.
#[cfg(not(target_arch = "wasm32"))]
pub trait MaybeSync: Sync {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Sync + ?Sized> MaybeSync for T {}

/// A value that may be shared across threads where the target has threads.
///
/// Natively this is every `Sync` type. On `wasm32` it is every type.
#[cfg(target_arch = "wasm32")]
pub trait MaybeSync {}
#[cfg(target_arch = "wasm32")]
impl<T: ?Sized> MaybeSync for T {}

/// A boxed future: `Send` natively, not `Send` on `wasm32`.
#[cfg(not(target_arch = "wasm32"))]
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
/// A boxed future: `Send` natively, not `Send` on `wasm32`.
#[cfg(target_arch = "wasm32")]
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// A boxed stream: `Send` natively, not `Send` on `wasm32`.
#[cfg(not(target_arch = "wasm32"))]
pub type BoxStream<'a, T> = Pin<Box<dyn futures_core::Stream<Item = T> + Send + 'a>>;
/// A boxed stream: `Send` natively, not `Send` on `wasm32`.
#[cfg(target_arch = "wasm32")]
pub type BoxStream<'a, T> = Pin<Box<dyn futures_core::Stream<Item = T> + 'a>>;

/// A shared callback: `Send + Sync` natively, neither on `wasm32`.
#[cfg(not(target_arch = "wasm32"))]
pub type SharedFn<A, R> = Arc<dyn Fn(A) -> R + Send + Sync>;
/// A shared callback: `Send + Sync` natively, neither on `wasm32`.
#[cfg(target_arch = "wasm32")]
pub type SharedFn<A, R> = Arc<dyn Fn(A) -> R>;

/// The deadline of a [`timeout`] passed before its future finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Elapsed;

impl fmt::Display for Elapsed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("deadline has elapsed")
    }
}

impl std::error::Error for Elapsed {}

/// The platform has no secure random source.
///
/// Natively the operating system always has one; on `wasm32` the host lacks
/// `crypto.getRandomValues`. A caller that needs unpredictable bytes has no
/// safe fallback, so it refuses the work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoRandomSource;

impl fmt::Display for NoRandomSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("this host has no secure random source (crypto.getRandomValues)")
    }
}

impl std::error::Error for NoRandomSource {}

/// Fills `buf` with bytes from the platform's secure random source.
///
/// The source is the operating system natively and `crypto.getRandomValues`
/// on `wasm32`.
///
/// # Errors
///
/// [`NoRandomSource`] when the platform has no random source. `buf` is then
/// unspecified and nothing falls back to a source that is not secure.
pub fn fill_random(buf: &mut [u8]) -> Result<(), NoRandomSource> {
    getrandom::fill(buf).map_err(|_| NoRandomSource)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_core::Stream;

    #[test]
    fn elapsed_names_the_missed_deadline() {
        assert_eq!(Elapsed.to_string(), "deadline has elapsed");
        let as_error: &dyn std::error::Error = &Elapsed;
        assert!(as_error.source().is_none());
    }

    #[test]
    fn fill_random_overwrites_the_whole_buffer() {
        let mut first = [0u8; 32];
        let mut second = [0u8; 32];
        fill_random(&mut first).unwrap();
        fill_random(&mut second).unwrap();
        assert_ne!(first, [0u8; 32]);
        assert_ne!(first, second);
    }

    #[test]
    fn fill_random_accepts_an_empty_buffer() {
        assert_eq!(fill_random(&mut []), Ok(()));
    }

    #[test]
    fn no_random_source_names_the_missing_host_api() {
        assert_eq!(
            NoRandomSource.to_string(),
            "this host has no secure random source (crypto.getRandomValues)"
        );
        let as_error: &dyn std::error::Error = &NoRandomSource;
        assert!(as_error.source().is_none());
    }

    #[test]
    fn the_aliases_hold_a_send_value_natively() {
        fn needs_send<T: Send>(_: &T) {}
        let future: BoxFuture<'static, u8> = Box::pin(async { 7 });
        needs_send(&future);
        let callback: SharedFn<u8, u8> = Arc::new(|n| n + 1);
        needs_send(&callback);
        assert_eq!(callback(1), 2);
    }

    #[tokio::test]
    async fn a_boxed_stream_yields_its_items() {
        struct Two(u8);
        impl Stream for Two {
            type Item = u8;
            fn poll_next(
                mut self: Pin<&mut Self>,
                _: &mut core::task::Context<'_>,
            ) -> core::task::Poll<Option<u8>> {
                self.0 += 1;
                core::task::Poll::Ready((self.0 <= 2).then_some(self.0))
            }
        }
        let mut stream: BoxStream<'static, u8> = Box::pin(Two(0));
        let mut seen = Vec::new();
        while let Some(n) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
            seen.push(n);
        }
        assert_eq!(seen, [1, 2]);
    }

    #[test]
    fn marker_traits_cover_send_and_sync_types() {
        fn maybe<T: MaybeSend + MaybeSync>() {}
        maybe::<u8>();
        maybe::<String>();
    }
}
