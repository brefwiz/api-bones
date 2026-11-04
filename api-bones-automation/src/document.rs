// SPDX-License-Identifier: MIT
//! Document validation against manifests the caller has already admitted.

use core::fmt;

use crate::{Refusal, compile::check_document_inner};

/// The point at which a document is checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentPhase {
    /// Resolve a template's references while its parameter slots remain open.
    Admission,
    /// Require every required parameter to have an argument or a default.
    Publish,
}

/// A document and the product manifests available to resolve its references.
#[derive(Debug, Clone, Copy)]
pub struct DocumentInput<'a> {
    /// Serialized `bones.v1.AutomationDocument` bytes.
    pub document: &'a [u8],
    /// The declaring product, used for references whose product is empty.
    pub product: &'a str,
    /// Serialized `AutomationManifest` bytes, one per admitted product,
    /// including the declaring product. Signature and identity verification
    /// remain the caller's responsibility.
    pub manifests: &'a [&'a [u8]],
    pub phase: DocumentPhase,
}

/// Why a document could not be checked or was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentError {
    /// The document cannot be decoded.
    Undecodable(String),
    /// A supplied manifest is unusable; identifies its position and cause.
    Manifest { index: usize, detail: String },
    /// Empty declaring product, duplicate product, or absent declaring manifest.
    Input(String),
    /// Document refusals, in document traversal order.
    Refused(Vec<Refusal>),
}

impl fmt::Display for DocumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Undecodable(detail) => write!(f, "document does not decode: {detail}"),
            Self::Manifest { index, detail } => write!(f, "manifest {index} is unusable: {detail}"),
            Self::Input(detail) => write!(f, "document input is invalid: {detail}"),
            Self::Refused(refusals) => crate::compile::write_refusals(f, refusals),
        }
    }
}

impl std::error::Error for DocumentError {}

/// Check one document using only the supplied admitted manifests.
///
/// Resolves references against each product's declared inventory and schema,
/// enforcing document rules identically for local and cross-product bindings.
/// Admission permits required parameter slots; publish requires their values.
/// No signature, trust root, registry or network access is involved.
///
/// # Errors
///
/// [`DocumentError`] identifies unusable input or every document refusal found.
pub fn check_document(input: &DocumentInput<'_>) -> Result<(), DocumentError> {
    check_document_inner(input)
}
