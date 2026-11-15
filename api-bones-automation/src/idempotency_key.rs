// SPDX-License-Identifier: MIT
//! Conformance check for the `bones.v1.idempotency_key` method option.
//!
//! The marker is a promise the service must keep: the same key and request are
//! answered the first answer, recorded in the call's own transaction; the same
//! key with a different request is refused `ALREADY_EXISTS`. A key and its
//! stored answer belong to the calling organization, the calling workload, the
//! person it relays when it relays one, and the method
//! (`<package>.<Service>/<Method>`): the same key under any other organization,
//! workload, person or method is a different key, which gets neither the stored
//! answer nor `ALREADY_EXISTS` and learns nothing about the first. The service
//! authenticates and authorizes the call before it looks a key up, so a caller
//! not allowed to run the method learns neither a stored answer nor whether a
//! key exists, and the relayed person is taken only from the verified relay,
//! never from the request. A request that holds nothing but the key therefore
//! never sees a different request under a key and does not declare
//! `ALREADY_EXISTS`; a request with an extension range holds a body. A keyed
//! request reused inside another request, directly, through a cycle or as
//! itself, carries a key field of its own, which must be empty: a non-empty
//! embedded key refuses the whole call `INVALID_ARGUMENT`, so a call has one
//! key, the top-level one, and a method embedding such a request declares
//! `INVALID_ARGUMENT`. The check verifies the declaration's shape only, not that
//! the service keeps the promise. A marked method is `IDEMPOTENT`, so clients
//! resend it automatically; a service must not mark a method until it records
//! and replays answers. A method carrying an `idempotency_key` field without
//! the marker is not checked.

use core::fmt;
use std::collections::BTreeSet;

use prost_reflect::{
    DescriptorPool, ExtensionDescriptor, Kind, MessageDescriptor, MethodDescriptor,
};

use crate::batch_of::{
    ALREADY_EXISTS, IDEMPOTENT, NO_SIDE_EFFECTS, declares_error, idempotency_level, level_name,
};

/// The error variant a call declares when it refuses a malformed request.
const INVALID_ARGUMENT: &str = "INVALID_ARGUMENT";

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
    /// The method is keyed, its request holds no field besides the key, and it
    /// declares `ALREADY_EXISTS`: the key's scope includes the method and the
    /// caller, so the same key never arrives with a different request.
    ConflictImpossible,
    /// The method is keyed and its request embeds the request of a keyed
    /// method, but it does not declare `INVALID_ARGUMENT`, the refusal of a
    /// non-empty embedded key.
    EmbeddedKeyUndeclared,
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
            IdempotencyKeyReason::ConflictImpossible => {
                "is keyed with a request holding nothing but the key but declares ALREADY_EXISTS, which cannot occur"
            }
            IdempotencyKeyReason::EmbeddedKeyUndeclared => {
                "is keyed and embeds the request of a keyed method but does not declare INVALID_ARGUMENT for a non-empty embedded key"
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
/// `ALREADY_EXISTS` in `bones.v1.errors` when its request holds a field besides
/// the key (and not otherwise), lists `INVALID_ARGUMENT` when its request embeds
/// the request of a keyed method at any depth (a request embedding itself,
/// directly or through a cycle, does), and is not `NO_SIDE_EFFECTS`. A method
/// that is not keyed is not checked. The first rule a method breaks is the one
/// reported, in the order above: key field, read, level, `ALREADY_EXISTS`
/// (declared or not as the body requires), then `INVALID_ARGUMENT`. A set that does not import the
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
    let keyed_requests: BTreeSet<String> = pool
        .services()
        .flat_map(|service| service.methods().collect::<Vec<_>>())
        .filter(|method| is_keyed(method, Some(&ext)))
        .map(|method| method.input().full_name().to_owned())
        .collect();
    let mut refusals = Vec::new();
    for service in pool.services() {
        for method in service.methods() {
            if let Err(reason) = check_method(&method, &ext, errors.as_ref(), &keyed_requests) {
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
    keyed_requests: &BTreeSet<String>,
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
    let declares_conflict = declares_error(method, errors, ALREADY_EXISTS);
    let has_body = has_field_besides_key(&method.input());
    if has_body && !declares_conflict {
        return Err(IdempotencyKeyReason::ConflictUndeclared);
    }
    if !has_body && declares_conflict {
        return Err(IdempotencyKeyReason::ConflictImpossible);
    }
    if embeds_any(&method.input(), keyed_requests)
        && !declares_error(method, errors, INVALID_ARGUMENT)
    {
        return Err(IdempotencyKeyReason::EmbeddedKeyUndeclared);
    }
    Ok(())
}

/// Whether `message` holds any field other than the top-level key field, of
/// any type, or declares an extension range: extensions are fields besides the
/// key that a request can carry.
pub(crate) fn has_field_besides_key(message: &MessageDescriptor) -> bool {
    message.fields().any(|field| field.name() != KEY_FIELD)
        || message.extension_ranges().next().is_some()
}

/// Whether a message-typed field of `root`, at any depth, is one of `targets`.
fn embeds_any(root: &MessageDescriptor, targets: &BTreeSet<String>) -> bool {
    let mut seen = BTreeSet::from([root.full_name().to_owned()]);
    let mut pending = vec![root.clone()];
    while let Some(message) = pending.pop() {
        for field in message.fields() {
            let Kind::Message(inner) = field.kind() else {
                continue;
            };
            if targets.contains(inner.full_name()) {
                return true;
            }
            if seen.insert(inner.full_name().to_owned()) {
                pending.push(inner);
            }
        }
    }
    false
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust_paging::resolver;

    /// A proto2 `Req` (a key field with presence, so only reachable below the
    /// key-field check) holding `fields`.
    fn request(fields: &str) -> MessageDescriptor {
        let source = format!(
            "syntax = \"proto2\"; package fixture.v1; message Req {{ optional string idempotency_key = 1; {fields} }}"
        );
        let mut compiler = protox::Compiler::with_file_resolver(resolver::Resolver(source));
        compiler.include_imports(true);
        compiler
            .open_file(resolver::PATH)
            .expect("fixture compiles");
        compiler
            .descriptor_pool()
            .get_message_by_name("fixture.v1.Req")
            .expect("Req")
    }

    #[test]
    fn an_extension_range_is_a_body_beside_the_key() {
        assert!(has_field_besides_key(&request("extensions 100 to 199;")));
        assert!(has_field_besides_key(&request("optional int32 id = 2;")));
        assert!(!has_field_besides_key(&request("")));
    }
}
