// SPDX-License-Identifier: MIT
//! The DSSE signature of a manifest.
//!
//! `SignedAutomationManifest.signature` holds a DSSE signature object,
//! `{"keyid": ..., "sig": ...}`, where `sig` is the standard base64 of an
//! Ed25519 signature over the DSSE pre-authentication encoding of
//! [`PAYLOAD_TYPE`] and the manifest bytes. The payload type separates this
//! domain from every other DSSE use of the same key family.
//!
//! The key is a dedicated automation-manifest key, rotated by epoch. It is
//! neither the release root nor the law-pack key. This module holds no key
//! and no pin: [`sign`] takes the signer's key and [`verify`] takes the
//! caller's key set.

use core::fmt;

use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signature, Signer};
pub use ed25519_dalek::{SigningKey, VerifyingKey};
use prost_reflect::{DynamicMessage, Value, prost::Message};
use serde::{Deserialize, Serialize};

use crate::manifest::{ManifestError, ManifestHeader, read_header};
use crate::pool::shipped;

/// The DSSE payload type of a signed automation manifest.
pub const PAYLOAD_TYPE: &str = "application/vnd.brefwiz.automation-manifest.v1+protobuf";

/// The DSSE signature object.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Dsse {
    keyid: String,
    sig: String,
}

/// DSSE v1 pre-authentication encoding of `payload` under `payload_type`.
#[must_use]
pub fn pae(payload_type: &str, payload: &[u8]) -> Vec<u8> {
    let mut out = format!(
        "DSSEv1 {} {payload_type} {} ",
        payload_type.len(),
        payload.len()
    )
    .into_bytes();
    out.extend_from_slice(payload);
    out
}

/// The DSSE signature object over `manifest`, made with `key` and named
/// `keyid`. Ed25519 is deterministic, so equal input gives equal bytes.
#[must_use]
pub fn sign(manifest: &[u8], keyid: &str, key: &SigningKey) -> Vec<u8> {
    let signature = key.sign(&pae(PAYLOAD_TYPE, manifest));
    serde_json::json!({
        "keyid": keyid,
        "sig": STANDARD.encode(signature.to_bytes()),
    })
    .to_string()
    .into_bytes()
}

/// The serialized `SignedAutomationManifest` carrying `manifest` exactly and
/// `signature` as made by [`sign`].
#[must_use]
pub fn signed_manifest(manifest: &[u8], signature: &[u8]) -> Vec<u8> {
    let mut message = DynamicMessage::new(shipped("bones.v1.SignedAutomationManifest"));
    message.set_field_by_name("manifest", Value::Bytes(manifest.to_vec().into()));
    message.set_field_by_name("signature", Value::Bytes(signature.to_vec().into()));
    message.encode_to_vec()
}

/// One key the caller trusts, valid from `not_before` up to but excluding
/// `not_after`, both in Unix seconds.
#[derive(Debug, Clone)]
pub struct TrustedKey {
    pub keyid: String,
    pub key: VerifyingKey,
    pub not_before: i64,
    pub not_after: i64,
}

/// A signed manifest that verified: the exact manifest bytes and the key that
/// signed them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verified {
    pub manifest: Vec<u8>,
    pub keyid: String,
}

/// A signed manifest a reader refuses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyError {
    /// The envelope or the signature object is malformed.
    Malformed(String),
    /// No key in the set carries the signature's `keyid`.
    UnknownKey(String),
    /// A key carries the `keyid`, but none is valid at the time of the check.
    KeyNotValid { keyid: String, at: i64 },
    /// The signature does not verify under any valid key of that name.
    BadSignature(String),
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(why) => write!(f, "signed manifest is malformed: {why}"),
            Self::UnknownKey(keyid) => write!(f, "signing key {keyid} is not trusted"),
            Self::KeyNotValid { keyid, at } => {
                write!(f, "signing key {keyid} is not valid at {at}")
            }
            Self::BadSignature(keyid) => write!(f, "signature by {keyid} does not verify"),
        }
    }
}

impl std::error::Error for VerifyError {}

/// Verify a serialized `SignedAutomationManifest` against `keys` at time `at`
/// (Unix seconds), returning the exact manifest bytes it carries.
///
/// The signature must name a key in the set that is valid at `at`, and verify
/// strictly over the pre-authentication encoding of [`PAYLOAD_TYPE`] and the
/// manifest bytes.
///
/// # Errors
///
/// [`VerifyError`] when the envelope is malformed, the key is unknown or out
/// of its validity window, or the signature does not verify.
pub fn verify(signed: &[u8], keys: &[TrustedKey], at: i64) -> Result<Verified, VerifyError> {
    let envelope = DynamicMessage::decode(shipped("bones.v1.SignedAutomationManifest"), signed)
        .map_err(|e| VerifyError::Malformed(e.to_string()))?;
    let bytes = |name: &str| {
        envelope
            .get_field_by_name(name)
            .and_then(|v| v.as_bytes().map(|b| b.to_vec()))
            .unwrap_or_default()
    };
    let manifest = bytes("manifest");
    let object: Dsse = serde_json::from_slice(&bytes("signature"))
        .map_err(|e| VerifyError::Malformed(e.to_string()))?;
    let raw = STANDARD
        .decode(&object.sig)
        .map_err(|e| VerifyError::Malformed(e.to_string()))?;
    let signature =
        Signature::from_slice(&raw).map_err(|e| VerifyError::Malformed(e.to_string()))?;

    let named: Vec<&TrustedKey> = keys.iter().filter(|k| k.keyid == object.keyid).collect();
    if named.is_empty() {
        return Err(VerifyError::UnknownKey(object.keyid));
    }
    let valid: Vec<&&TrustedKey> = named
        .iter()
        .filter(|k| k.not_before <= at && at < k.not_after)
        .collect();
    if valid.is_empty() {
        return Err(VerifyError::KeyNotValid {
            keyid: object.keyid,
            at,
        });
    }
    let message = pae(PAYLOAD_TYPE, &manifest);
    if valid
        .iter()
        .any(|k| k.key.verify_strict(&message, &signature).is_ok())
    {
        Ok(Verified {
            manifest,
            keyid: object.keyid,
        })
    } else {
        Err(VerifyError::BadSignature(object.keyid))
    }
}

/// A signed manifest the layer refuses to admit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdmitError {
    Verify(VerifyError),
    Manifest(ManifestError),
    /// The manifest names another product than the serving workload's.
    ProductMismatch {
        expected: String,
        found: String,
    },
    /// The manifest's digest is not the one the product serves.
    DigestMismatch {
        expected: String,
        found: String,
    },
}

impl fmt::Display for AdmitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Verify(e) => fmt::Display::fmt(e, f),
            Self::Manifest(e) => fmt::Display::fmt(e, f),
            Self::ProductMismatch { expected, found } => {
                write!(f, "manifest is for product {found}, expected {expected}")
            }
            Self::DigestMismatch { expected, found } => {
                write!(f, "manifest digest {found} is not the served {expected}")
            }
        }
    }
}

impl std::error::Error for AdmitError {}

/// Verify `signed`, then admit it only if its product is `product` and its
/// `declaration_digest` is `served_digest`.
///
/// # Errors
///
/// [`AdmitError`] when verification fails or the product or digest differs.
pub fn admit(
    signed: &[u8],
    keys: &[TrustedKey],
    at: i64,
    product: &str,
    served_digest: &str,
) -> Result<ManifestHeader, AdmitError> {
    let verified = verify(signed, keys, at).map_err(AdmitError::Verify)?;
    let header = read_header(&verified.manifest).map_err(AdmitError::Manifest)?;
    if header.product != product {
        return Err(AdmitError::ProductMismatch {
            expected: product.to_owned(),
            found: header.product,
        });
    }
    if header.declaration_digest != served_digest {
        return Err(AdmitError::DigestMismatch {
            expected: served_digest.to_owned(),
            found: header.declaration_digest,
        });
    }
    Ok(header)
}
