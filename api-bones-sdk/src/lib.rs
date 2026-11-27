// SPDX-License-Identifier: MIT
//! The Layer 2 vocabulary of the api-bones SDK, decided once in Rust.
//!
//! Every language binding speaks through these types:
//!
//! - [`Error`]: the `api-bones.error/v1` wire error, with the local refusals
//!   the core makes itself (`HANDLE_BUSY`, `CANCELED`, `IDENTITY_CHANGED`,
//!   `CALL_BUDGET_ELAPSED`).
//! - The protobuf boundary: every value that crosses between a host and the
//!   core is the encoding of a `bones.sdk.v1` message, never text. The
//!   encoders a binding needs are reached through `__private`.
//! - [`CallCtx`]: the caller's trace fields, installed around a call with
//!   [`CallCtx::scope`] instead of a parameter on every method.
//! - [`SpanSink`], [`Tracer`], [`ActiveSpan`]: spans reported live to a host
//!   sink, with no replay.
//! - [`Handle`], [`ListCall`], [`Stream`], [`Batch`]: what a call returns when
//!   its result arrives over time, with `next()`, `cancel()` and `span()`.

#![forbid(unsafe_code)]

#[doc(hidden)]
#[path = "private.rs"]
pub mod __private;
mod boundary;
pub mod call;
mod ctx;
mod error;
mod handle;
mod span;

/// The version of the boundary this core speaks: the messages of
/// `bones.sdk.v1` and the export shapes of the generated wrappers. A wrapper
/// or runtime stamps the version it was built for, and the core refuses to
/// construct a client for another (`SDK_BOUNDARY_MISMATCH`). Bumped on any
/// change a stale wrapper could misread.
pub const BOUNDARY: u32 = 1;

pub use ctx::CallCtx;
pub use error::{
    Arg, CALL_BUDGET_ELAPSED, CANCELED, EMITTER, Error, HANDLE_BUSY, IDENTITY_CHANGED, PageDetail,
    PageKind, SDK_BOUNDARY_MISMATCH, platform_of_connect,
};
pub use handle::{Batch, Handle, ListCall, Stream};
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
pub use span::JsSink;
pub use span::{
    ActiveSpan, Attributes, SpanEnd, SpanKind, SpanSink, SpanStart, SpanStarted, SpanStatus,
    Traceparent, Tracer,
};
