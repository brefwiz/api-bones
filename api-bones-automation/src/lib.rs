// SPDX-License-Identifier: MIT
//! The reference compiler and signature check for the automation manifest.
//!
//! One implementation serves the service-kit, the `brefwiz` CLI and the
//! layer:
//!
//! - [`compile`] turns a product's descriptor set into the serialized
//!   `AutomationManifest`, refusing on rules D1-D6 of
//!   `bones/v1/automation_manifest.proto`, byte-identically for equal input.
//! - [`check_surface`] applies the same rules to a product's declarations
//!   without its identity and returns the digest the manifest will carry, so
//!   a product's build refuses what its release would, save a template
//!   naming its own product, which only [`compile`] can recognise.
//! - [`check_document`] resolves document bindings against admitted product
//!   manifests and checks required parameter values when publishing.
//! - [`declaration_digest`] is the digest the manifest carries and a product
//!   serves in its `AutomationSurfaceRef`.
//! - [`check_batch_of`] refuses a `bones.v1.batch_of` that does not name a
//!   method of its own service, or whose method does not keep that method's
//!   authorization.
//! - [`check_nested_write_of`] refuses a `bones.v1.nested_write_of` that does
//!   not name a sibling unary write, sits on a message no RPC or a read takes,
//!   is not a children field, or holds children the named write does not
//!   write.
//! - [`signature`] signs and verifies the DSSE Ed25519 signature over the
//!   manifest. The crate holds no key and no pin: signing takes the caller's
//!   key, verifying takes the caller's key set with validity epochs.

mod batch_of;
mod compile;
mod digest;
mod document;
mod manifest;
mod nested_write_of;
mod pool;
mod rust_paging;
pub mod signature;

pub use batch_of::{
    BatchOfError, BatchOfReason, BatchOfRefusal, check_batch_of, check_batch_of_against_pool,
};
pub use compile::{
    CheckedSurface, CompileError, CompileInput, Refusal, Rule, SurfaceInput, check_surface, compile,
};
pub use digest::declaration_digest;
pub use document::{DocumentError, DocumentInput, DocumentPhase, check_document};
pub use manifest::{MANIFEST_FORMAT, ManifestError, ManifestHeader, read_header};
pub use nested_write_of::{
    NestedWriteOfError, NestedWriteOfReason, NestedWriteOfRefusal, check_nested_write_of,
};
pub use rust_paging::{RustPagingError, emit_rust_paging};

mod method_shape;
pub use method_shape::{BatchPolicy, PagingPolicy, batch_policy, paging_policy};
