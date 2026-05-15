//! Canonical Borsh codec shim for chain-bound encoding (ADR platform/0047).
//!
//! Import [`prelude`] to get `BorshSerialize`, `BorshDeserialize`, and the
//! encode/decode helpers without depending on `borsh` directly.
#![no_std]
#![deny(warnings, unsafe_code)]
#![deny(missing_docs)]

extern crate alloc;

pub mod error;
pub mod prelude;
pub mod versioned;

use alloc::vec::Vec;
pub use borsh::{BorshDeserialize, BorshSerialize};

pub use error::BorshCanonicalError;
pub use versioned::VersionedBytes;

/// Opinionated, version-tagged Borsh encoding contract.
///
/// Implementors pin [`CODEC_VERSION`](Self::CODEC_VERSION) to a `u8` constant.
/// Any breaking change to the type's Borsh layout **must** increment this
/// constant so consumers can detect version mismatches without inspecting raw
/// bytes.
pub trait BorshCanonical: BorshSerialize {
    /// Monotonically increasing layout version.  Increment on every breaking
    /// change to the Borsh field order or field set.
    const CODEC_VERSION: u8;

    /// Encode `self` into canonical Borsh bytes.
    ///
    /// The default implementation delegates to [`borsh::to_vec`].
    fn to_canonical_bytes(&self) -> Result<Vec<u8>, BorshCanonicalError> {
        borsh::to_vec(self).map_err(BorshCanonicalError::from_io)
    }

    /// Encode `self` into a [`VersionedBytes`] envelope tagging the bytes
    /// with [`Self::CODEC_VERSION`].
    fn to_versioned(&self) -> Result<VersionedBytes, BorshCanonicalError> {
        Ok(VersionedBytes {
            codec_version: Self::CODEC_VERSION,
            bytes: self.to_canonical_bytes()?,
        })
    }
}
