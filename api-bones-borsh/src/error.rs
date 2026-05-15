//! Error type for canonical Borsh encoding operations.

/// Error returned by [`crate::BorshCanonical::to_canonical_bytes`].
#[derive(Debug)]
pub struct BorshCanonicalError {
    inner: borsh::io::Error,
}

impl BorshCanonicalError {
    /// Construct from a [`borsh::io::Error`].
    pub(crate) fn from_io(err: borsh::io::Error) -> Self {
        Self { inner: err }
    }

    /// Return the underlying I/O error.
    #[must_use]
    pub fn io_error(&self) -> &borsh::io::Error {
        &self.inner
    }
}

impl core::fmt::Display for BorshCanonicalError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "borsh canonical encoding error: {}", self.inner)
    }
}

#[cfg(feature = "std")]
extern crate std;

#[cfg(feature = "std")]
impl std::error::Error for BorshCanonicalError {}
