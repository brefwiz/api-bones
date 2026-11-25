// SPDX-License-Identifier: MIT
//! The attribute macros of the api-bones SDK.
//!
//! A Layer 2 crate annotates its types and its client; the macros read the
//! annotations through `api-bones-sdk-model`, the same parse the generator
//! uses, so the set of types that may cross the wasm boundary is decided once.
//!
//! - [`record`]: a struct whose named fields cross as a JSON object with
//!   camelCase keys. Expands to an `impl api_bones_sdk::Wire`.
//! - [`enumeration`]: a fieldless enum that crosses as a snake case string.
//!   Expands to an `impl Wire`.
//! - [`error`]: an enum of the errors a client declares. Expands to
//!   `Display`, `std::error::Error` and `From<E> for api_bones_sdk::Error`.
//! - [`client`]: an inherent impl whose `#[export]` methods are the Layer 2
//!   surface. The impl is kept as written; on wasm32 each export also becomes
//!   a method of a class named `<svc>__<Type>`, gated by the features
//!   `sdk-all`, `sdk-<surface>` and `sdk-m-<method>`.
//! - [`export`]: marks a method of a `#[client]` impl, and is refused anywhere
//!   else.
//!
//! The expansions live in plain modules over `proc_macro2` so they are unit
//! tested; the functions here only convert token streams.

mod client;
mod common;
mod enumeration;
mod error_type;
mod record;
mod service;
mod wasm;

use proc_macro::TokenStream;

/// A struct that crosses the wasm boundary as a JSON object.
#[proc_macro_attribute]
pub fn record(args: TokenStream, item: TokenStream) -> TokenStream {
    record::expand(args.into(), item.into()).into()
}

/// A fieldless enum that crosses the wasm boundary as a snake case string.
#[proc_macro_attribute]
pub fn enumeration(args: TokenStream, item: TokenStream) -> TokenStream {
    enumeration::expand(args.into(), item.into()).into()
}

/// An enum of errors; `#[error(emitter = "...")]` on the type and
/// `#[error(code = "...", platform = "...")]` on each variant.
#[proc_macro_attribute]
pub fn error(args: TokenStream, item: TokenStream) -> TokenStream {
    error_type::expand(args.into(), item.into()).into()
}

/// An inherent impl whose `#[export]` methods are exported to wasm;
/// `#[client(surface = "<sdk_surfaces id>")]`.
#[proc_macro_attribute]
pub fn client(args: TokenStream, item: TokenStream) -> TokenStream {
    client::expand(args.into(), item.into()).into()
}

/// A method of a `#[client]` impl. `#[client]` consumes it, so the macro only
/// runs for a method outside one.
#[proc_macro_attribute]
pub fn export(args: TokenStream, item: TokenStream) -> TokenStream {
    common::misplaced_export(args.into(), &item.into()).into()
}
