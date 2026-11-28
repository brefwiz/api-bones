// SPDX-License-Identifier: MIT
//! The attribute macros of the api-bones SDK.
//!
//! A Layer 2 crate annotates its types and its client; the macros read the
//! annotations through `api-bones-sdk-model`, the same parse the generator
//! uses, so the set of types that may cross the wasm boundary is decided once.
//! A value that crosses is a proto message or a proto scalar; the types are
//! generated from proto, never declared here.
//!
//! - [`error`]: an enum of the errors a client declares. Expands to
//!   `Display`, `std::error::Error` and `From<E> for api_bones_sdk::Error`.
//! - [`client`]: an inherent impl whose `#[export]` methods are the Layer 2
//!   surface. The impl is kept as written; on wasm32 each export also becomes
//!   a method of a class named `<svc>__<Type>`, gated by the features
//!   `sdk-all`, `sdk-<surface>` and `sdk-m-<method>`.
//!   A receiver-less `#[export(sync)]` function and an `#[export(sync)]` const
//!   of the impl are static exports of the class. `#[client]` also goes on an
//!   inline `mod`, whose `#[export(sync)]` functions and consts are the
//!   exports of a class named `<svc>__<mod>` that has no instance.
//!   With the consumer's feature `uniffi` on a native target, each export also
//!   becomes a free function of the uniffi binding and the client an object
//!   named `<Type>Ffi`; the consumer calls `uniffi::setup_scaffolding!` once.
//! - [`export`]: marks a method, function or const of a `#[client]` impl or
//!   module, and is refused anywhere else.
//!
//! The expansions live in plain modules over `proc_macro2` so they are unit
//! tested; the functions here only convert token streams.

mod client;
mod common;
mod error_type;
mod service;
mod uniffi;
mod wasm;

use proc_macro::TokenStream;

/// An enum of errors; `#[error(emitter = "...")]` on the type and
/// `#[error(code = "...", platform = "...")]` on each variant.
#[proc_macro_attribute]
pub fn error(args: TokenStream, item: TokenStream) -> TokenStream {
    error_type::expand(args.into(), item.into()).into()
}

/// An inherent impl, or an inline module, whose `#[export]` items are exported
/// to wasm; `#[client(surface = "<sdk_surfaces id>")]`.
#[proc_macro_attribute]
pub fn client(args: TokenStream, item: TokenStream) -> TokenStream {
    client::expand(args.into(), item.into()).into()
}

/// A method, function or const of a `#[client]` impl or module. `#[client]`
/// consumes it, so the macro only runs for an item outside one.
#[proc_macro_attribute]
pub fn export(args: TokenStream, item: TokenStream) -> TokenStream {
    common::misplaced_export(args.into(), &item.into()).into()
}
