// SPDX-License-Identifier: MIT
//! The native host boundary of the api-bones SDK for Kotlin and Swift.
//!
//! This crate is the one place the uniffi scaffolding is generated: the
//! `api-bones-sdk` core never depends on uniffi and keeps its unconditional
//! `forbid(unsafe_code)`. It adapts the core's ports to a foreign host:
//!
//! - [`FfiHost`]: what a native app answers, the credential and the spans.
//! - [`FfiCallCtx`]: the trace fields and list reads one call carries.
//! - [`FfiHandle`]: a list, stream or batch the host reads with `next`.
//! - [`FfiError`]: the `bones.sdk.v1.CallError` bytes of a failed call.
//! - [`call_env`], [`transport`]: what a generated client's constructor wires.
//! - [`format_error`]: an error's message in the viewer's language.
//!
//! The crate holds no `unsafe` of its own: only the uniffi macros expand it.

#![deny(unsafe_code)]

#[cfg(target_arch = "wasm32")]
compile_error!("api-bones-sdk-ffi is native-only: the browser binding is api-bones-sdk's wasm");

mod ctx;
mod error;
mod format;
mod handle;
mod host;
mod transport;

pub use ctx::FfiCallCtx;
pub use error::FfiError;
pub use format::{api_bones_sdk_build_info, format_error};
pub use handle::FfiHandle;
pub use host::{FfiHost, call_env};
pub use transport::{check_base_url, transport};

uniffi::setup_scaffolding!("api_bones_sdk_ffi");
