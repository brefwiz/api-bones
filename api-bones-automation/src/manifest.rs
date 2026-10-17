// SPDX-License-Identifier: MIT
//! Reading the identifying header of a serialized manifest.

use core::fmt;

use prost_reflect::DynamicMessage;

use crate::pool::shipped;

/// The revision of the manifest contract this crate compiles and reads.
pub const MANIFEST_FORMAT: u32 = 1;

/// The fields of a manifest an admitting reader compares before trusting it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestHeader {
    pub format: u32,
    pub product: String,
    pub version: String,
    pub commit: String,
    pub declaration_digest: String,
}

/// A serialized manifest a reader refuses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestError {
    /// The bytes are not a serialized `AutomationManifest`.
    Undecodable(String),
    /// The manifest declares a revision this reader does not know, including 0.
    UnknownFormat(u32),
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Undecodable(why) => write!(f, "manifest does not decode: {why}"),
            Self::UnknownFormat(found) => write!(
                f,
                "manifest format {found} is not supported; supported format: {MANIFEST_FORMAT}"
            ),
        }
    }
}

impl std::error::Error for ManifestError {}

/// Read the header of a serialized `AutomationManifest`, refusing any format
/// other than [`MANIFEST_FORMAT`].
///
/// # Errors
///
/// [`ManifestError`] when the bytes do not decode or name another format.
pub fn read_header(manifest: &[u8]) -> Result<ManifestHeader, ManifestError> {
    let message = DynamicMessage::decode(shipped("bones.v1.AutomationManifest"), manifest)
        .map_err(|e| ManifestError::Undecodable(e.to_string()))?;
    let text = |name: &str| {
        message
            .get_field_by_name(name)
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default()
    };
    let format = message
        .get_field_by_name("format")
        .and_then(|v| v.as_u32())
        .unwrap_or_default();
    if format != MANIFEST_FORMAT {
        return Err(ManifestError::UnknownFormat(format));
    }
    Ok(ManifestHeader {
        format,
        product: text("product"),
        version: text("version"),
        commit: text("commit"),
        declaration_digest: text("declaration_digest"),
    })
}
