// SPDX-License-Identifier: MIT
//! Conformance check for the `bones.v1.idempotency_key` method option.
//!
//! The marker is a promise the service must keep: the same key and request are
//! answered the first answer, recorded in the call's own transaction; the same
//! key with a different request is refused `ALREADY_EXISTS`. A key and its
//! stored answer belong to the calling organization: a key means nothing
//! outside it, and no answer or key existence is disclosed across
//! organizations. The check verifies the declaration's shape only, not that
//! the service keeps the promise. A marked method is `IDEMPOTENT`, so clients
//! resend it automatically; a service must not mark a method until it records
//! and replays answers. A method carrying an `idempotency_key` field without
//! the marker is not checked.

use core::fmt;

use prost_reflect::{DescriptorPool, ExtensionDescriptor, Kind, MethodDescriptor};

use crate::batch_of::{
    IDEMPOTENT, NO_SIDE_EFFECTS, declares_already_exists, idempotency_level, level_name,
};

/// The extension the check reads.
const IDEMPOTENCY_KEY: &str = "bones.v1.idempotency_key";

/// The extension listing the error variants an RPC declares.
const ERRORS: &str = "bones.v1.errors";

/// The request field a keyed call carries its key in.
const KEY_FIELD: &str = "idempotency_key";

/// One method that breaks the idempotency key rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdempotencyKeyRefusal {
    /// The RPC, as `<package>.<Service>/<Method>`.
    pub method: String,
    pub reason: IdempotencyKeyReason,
}

/// Why a method is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum IdempotencyKeyReason {
    /// The method is keyed but its request has no top-level singular
    /// `string idempotency_key` without presence (no `optional`, no oneof).
    KeyFieldMissing,
    /// The method is keyed and declares `NO_SIDE_EFFECTS`: a read is never keyed.
    KeyedRead,
    /// The method is keyed but does not declare `IDEMPOTENT`.
    NotIdempotent {
        /// The declared `google.protobuf.MethodOptions.IdempotencyLevel` number.
        level: i32,
    },
    /// The method is keyed but does not declare `ALREADY_EXISTS`, the refusal
    /// of a key reused with a different request.
    ConflictUndeclared,
}

impl fmt::Display for IdempotencyKeyRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let why = match self.reason {
            IdempotencyKeyReason::KeyFieldMissing => {
                "is keyed but its request has no top-level string idempotency_key"
            }
            IdempotencyKeyReason::KeyedRead => "is keyed but declares NO_SIDE_EFFECTS",
            IdempotencyKeyReason::NotIdempotent { level } => {
                return write!(
                    f,
                    "{}: is keyed but declares {}, not IDEMPOTENT",
                    self.method,
                    level_name(level)
                );
            }
            IdempotencyKeyReason::ConflictUndeclared => {
                "is keyed but does not declare ALREADY_EXISTS for a reused key"
            }
        };
        write!(f, "{}: {why}", self.method)
    }
}

/// Check every method of `descriptor_set` (a serialized `FileDescriptorSet`
/// that includes `bones/v1/annotations.proto`) against the keyed write rule.
///
/// A method declaring `bones.v1.idempotency_key` true, single or batch, has a
/// top-level `string idempotency_key` request field (singular, not `optional`,
/// not in a oneof), declares `idempotency_level = IDEMPOTENT`, lists
/// `ALREADY_EXISTS` in `bones.v1.errors`, and is not `NO_SIDE_EFFECTS`. A
/// method that is not keyed is not checked. A set that does not import the
/// vocabulary declares no key and passes. The check covers the declaration's
/// shape only.
///
/// # Errors
///
/// [`IdempotencyKeyError`] listing every refusal, in service then method
/// order, or the decode failure.
pub fn check_idempotency_key(descriptor_set: &[u8]) -> Result<(), IdempotencyKeyError> {
    let pool = DescriptorPool::decode(descriptor_set)
        .map_err(|e| IdempotencyKeyError::Descriptors(e.to_string()))?;
    let Some(ext) = pool.get_extension_by_name(IDEMPOTENCY_KEY) else {
        return Ok(());
    };
    let errors = pool.get_extension_by_name(ERRORS);
    let mut refusals = Vec::new();
    for service in pool.services() {
        for method in service.methods() {
            if let Err(reason) = check_method(&method, &ext, errors.as_ref()) {
                refusals.push(IdempotencyKeyRefusal {
                    method: format!("{}/{}", service.full_name(), method.name()),
                    reason,
                });
            }
        }
    }
    if refusals.is_empty() {
        Ok(())
    } else {
        Err(IdempotencyKeyError::Refused(refusals))
    }
}

fn check_method(
    method: &MethodDescriptor,
    ext: &ExtensionDescriptor,
    errors: Option<&ExtensionDescriptor>,
) -> Result<(), IdempotencyKeyReason> {
    if !is_keyed(method, Some(ext)) {
        return Ok(());
    }
    let carries_key = method
        .input()
        .get_field_by_name(KEY_FIELD)
        .is_some_and(|field| {
            field.kind() == Kind::String && !field.is_list() && !field.supports_presence()
        });
    if !carries_key {
        return Err(IdempotencyKeyReason::KeyFieldMissing);
    }
    let level = idempotency_level(method);
    if level == NO_SIDE_EFFECTS {
        return Err(IdempotencyKeyReason::KeyedRead);
    }
    if level != IDEMPOTENT {
        return Err(IdempotencyKeyReason::NotIdempotent { level });
    }
    if !declares_already_exists(method, errors) {
        return Err(IdempotencyKeyReason::ConflictUndeclared);
    }
    Ok(())
}

/// Whether the RPC declares `bones.v1.idempotency_key` true.
pub(crate) fn is_keyed(method: &MethodDescriptor, keyed: Option<&ExtensionDescriptor>) -> bool {
    keyed.is_some_and(|ext| method.options().get_extension(ext).as_bool() == Some(true))
}

/// Why a descriptor set did not pass [`check_idempotency_key`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdempotencyKeyError {
    /// The set does not decode into a complete pool.
    Descriptors(String),
    /// Every refused method.
    Refused(Vec<IdempotencyKeyRefusal>),
}

crate::compile::impl_check_error!(IdempotencyKeyError);
