// SPDX-License-Identifier: MIT
//! The Layer 2 vocabulary of the api-bones SDK, decided once in Rust.
//!
//! Every language binding speaks through these types:
//!
//! - [`Error`]: the `api-bones.error/v1` wire error, with the local refusals
//!   the core makes itself (`HANDLE_BUSY`, `CANCELED`, `IDENTITY_CHANGED`,
//!   `CALL_BUDGET_ELAPSED`).
//! - [`Wire`]: the JSON codec of the allowed type set, so a binding passes
//!   arguments and results as JSON text.
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
pub mod call;
mod ctx;
mod error;
mod handle;
mod span;
mod wire;

pub use ctx::CallCtx;
pub use error::{
    Arg, CALL_BUDGET_ELAPSED, CANCELED, EMITTER, Error, HANDLE_BUSY, IDENTITY_CHANGED, PageDetail,
    PageKind, platform_of_connect,
};
pub use handle::{Batch, Handle, ListCall, Stream};
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
pub use span::JsSink;
pub use span::{
    ActiveSpan, Attributes, SpanEnd, SpanKind, SpanSink, SpanStart, SpanStatus, Traceparent, Tracer,
};
pub use wire::{
    BadValue, Wire, double_value, field, format_rfc3339, from_json_str, parse_args, parse_rfc3339,
    system_time_from_parts, system_time_parts, to_json_string,
};
