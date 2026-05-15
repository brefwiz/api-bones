//! Convenience re-exports — `use api_bones_borsh::prelude::*;` gives everything
//! needed to implement and use canonical Borsh encoding without a direct `borsh`
//! dependency in consumer crates.

pub use borsh::{BorshDeserialize, BorshSerialize, from_slice, to_vec};

pub use crate::{BorshCanonical, BorshCanonicalError, VersionedBytes};
