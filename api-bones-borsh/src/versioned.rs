//! Wire envelope for canonical Borsh bytes.

use alloc::vec::Vec;
use borsh::{BorshDeserialize, BorshSerialize};

/// Wire envelope pairing a [`codec_version`](VersionedBytes::codec_version)
/// tag with opaque canonical bytes.
///
/// The outer struct is itself Borsh-serialisable so it can be nested inside
/// signed envelopes without an extra framing layer.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct VersionedBytes {
    /// Codec version declared by the originating type via
    /// [`BorshCanonical::CODEC_VERSION`](crate::BorshCanonical::CODEC_VERSION).
    pub codec_version: u8,
    /// Canonical Borsh bytes of the inner payload.
    pub bytes: Vec<u8>,
}
