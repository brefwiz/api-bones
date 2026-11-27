// SPDX-License-Identifier: MIT
//! The one parse of a Layer 2 crate's SDK exports.
//!
//! The `api-bones-sdk` macros expand each export, and `api-bones-sdk-gen`
//! reads the same exports to write the crate's feature list and its
//! `l2-methods.json`. Both call this crate, so the set of types that may cross
//! the wasm boundary, the spelling of an export and the meaning of an
//! attribute are decided once.
//!
//! - [`ty`] classifies what crosses the boundary: a generated message, a scalar
//!   in its `google.protobuf` wrapper, or nothing.
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
pub use l2::{L2Error, L2Kind, L2Method, L2Methods, L2ReadError, SpecL2Methods, l2_methods};
pub use model::{
    ArgKind, Client, ErrorField, ErrorType, ErrorVariant, Export, Model, Param, Shape,
};
pub use names::{NameError, export_name, gate_features, json_name, typescript_name};
pub use ty::{
    ALLOWED, EMPTY, Env, ErrorRef, HandleKind, ProtoRef, Return, Wire, Wrapper, classify_return,
    classify_wire, path_text, proto_ref,
};
