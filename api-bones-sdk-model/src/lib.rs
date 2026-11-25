// SPDX-License-Identifier: MIT
//! The one parse of a Layer 2 crate's SDK exports.
//!
//! The `api-bones-sdk` macros expand each export, and `api-bones-sdk-gen`
//! reads the same exports to write the crate's feature list and its
//! `l2-methods.json`. Both call this crate, so the set of types that may cross
//! the wasm boundary, the spelling of an export and the meaning of an
//! attribute are decided once.
//!
//! - [`ty`] classifies a parameter or return type against the allowed set.
//! - [`attrs`] reads the arguments of `#[client]`, `#[export]`, `#[error]`.
//! - [`model`] walks parsed source into a [`Model`].
//! - [`names`] spells the wasm export, the gate features and the TypeScript
//!   method.
//! - [`l2`] builds the `l2-methods.json` document.

pub mod attrs;
pub mod l2;
pub mod model;
pub mod names;
pub mod ty;

pub use attrs::{ClientArgs, ErrorArgs, ErrorVariantArgs, ExportArgs};
pub use l2::{L2Error, L2Method, L2Methods, SpecL2Methods, l2_methods};
pub use model::{
    ArgKind, Client, Enumeration, ErrorField, ErrorType, ErrorVariant, Export, Model, Param,
    Record, enumeration_values,
};
pub use names::{NameError, enum_value, export_name, gate_features, json_name, typescript_name};
pub use ty::{ALLOWED, Env, ErrorRef, HandleKind, Return, WireType, classify, classify_return};
