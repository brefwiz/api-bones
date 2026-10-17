// SPDX-License-Identifier: MIT
//! The reference compiler and signature check for the automation manifest.
//!
//! One implementation serves the service-kit, the `brefwiz` CLI and the
//! layer:
//!
//! - [`compile`] turns a product's descriptor set into the serialized
//!   `AutomationManifest`, refusing on rules D1-D6 of
//!   `bones/v1/automation_manifest.proto`, byte-identically for equal input.
//! - [`declaration_digest`] is the digest the manifest carries and a product
//!   serves in its `AutomationSurfaceRef`.
//! - [`signature`] signs and verifies the DSSE Ed25519 signature over the
//!   manifest. The crate holds no key and no pin: signing takes the caller's
//!   key, verifying takes the caller's key set with validity epochs.

mod compile;
mod digest;
mod manifest;
mod pool;
pub mod signature;

pub use compile::{CompileError, CompileInput, Refusal, Rule, compile};
pub use digest::declaration_digest;
pub use manifest::{MANIFEST_FORMAT, ManifestError, ManifestHeader, read_header};
