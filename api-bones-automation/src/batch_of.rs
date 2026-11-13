// SPDX-License-Identifier: MIT
//! Conformance check for the `bones.v1.batch_of` method option.

use core::fmt;
use std::collections::{BTreeMap, BTreeSet};

use prost_reflect::{DescriptorPool, ExtensionDescriptor, MethodDescriptor, ServiceDescriptor};

/// The extension the check reads.
const BATCH_OF: &str = "bones.v1.batch_of";

/// The authorization extension a batch method must share with its single.
const AUTHZ: &str = "bones.v1.authz";

/// The extension listing the error variants an RPC declares.
const ERRORS: &str = "bones.v1.errors";

/// The error variant a create declares when a duplicate is its own refusal.
const ALREADY_EXISTS: &str = "ALREADY_EXISTS";

/// `google.protobuf.MethodOptions.IdempotencyLevel.NO_SIDE_EFFECTS`.
pub(crate) const NO_SIDE_EFFECTS: i32 = 1;

fn level_name(level: i32) -> &'static str {
    match level {
        1 => "NO_SIDE_EFFECTS",
        2 => "IDEMPOTENT",
        _ => "IDEMPOTENCY_UNKNOWN",
    }
}

/// One `batch_of` declaration that does not name a method of its own service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchOfRefusal {
    /// The RPC that declares the option, as `<package>.<Service>/<Method>`.
    pub method: String,
    /// The value the option carries.
    pub batch_of: String,
    pub reason: BatchOfReason,
}

/// Why a `batch_of` value is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BatchOfReason {
    /// The value names a method of a different service.
    OtherService,
    /// The service declares no method of that name.
    UnknownMethod,
    /// The value names the declaring RPC itself.
    SelfReference,
    /// The batch method's authorization is not the named single's.
    AuthzDiffers,
    /// The batch method's `idempotency_level` is not the named single's.
    /// Levels are `google.protobuf.MethodOptions.IdempotencyLevel` numbers.
    IdempotencyDiffers {
        /// The single's declared level.
        single: i32,
        /// The batch method's declared level.
        batch: i32,
    },
    /// The batch method declares `ALREADY_EXISTS` in `bones.v1.errors` while
    /// the named single does not. An item refuses with `already_exists` only
    /// when its single declares it.
    AlreadyExistsUndeclared,
}

impl fmt::Display for BatchOfRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let why = match self.reason {
            BatchOfReason::OtherService => "names a method of another service",
            BatchOfReason::UnknownMethod => "names a method the service does not declare",
            BatchOfReason::SelfReference => "names the RPC itself",
            BatchOfReason::AuthzDiffers => "does not keep the authorization of the method it names",
            BatchOfReason::AlreadyExistsUndeclared => {
                "declares ALREADY_EXISTS but the method it names does not"
            }
            BatchOfReason::IdempotencyDiffers { single, batch } => {
                return write!(
                    f,
                    "{}: batch_of \"{}\" does not keep the idempotency level of the method it names (single {}, batch {})",
                    self.method,
                    self.batch_of,
                    level_name(single),
                    level_name(batch)
                );
            }
        };
        write!(f, "{}: batch_of \"{}\" {why}", self.method, self.batch_of)
    }
}

/// Check every `bones.v1.batch_of` in `descriptor_set` (a serialized
/// `FileDescriptorSet` that includes `bones/v1/annotations.proto`).
///
/// The value is the bare name of a method of the same service, or that
/// method written `<package>.<Service>/<Method>`. Anything else is refused.
/// The batch method also keeps its single's authorization and idempotency
/// level, and declares `ALREADY_EXISTS` only when its single does.
/// A set that does not import the vocabulary declares no `batch_of` and passes.
///
/// # Errors
///
/// [`BatchOfError`] listing every refusal, in service then method order, or
/// the decode failure.
pub fn check_batch_of(descriptor_set: &[u8]) -> Result<(), BatchOfError> {
    let pool = DescriptorPool::decode(descriptor_set)
        .map_err(|e| BatchOfError::Descriptors(e.to_string()))?;
    check_pool(&pool)
}

/// Check serialized batch declarations against the pool that produced them.
///
/// Build paths that expect batching use this check so missing custom options
/// cannot turn conformance validation into an empty check.
///
/// # Errors
/// Returns [`BatchOfError`] when declarations differ, cannot decode, or fail
/// the same conformance rules as [`check_batch_of`].
pub fn check_batch_of_against_pool(
    descriptor_set: &[u8],
    expected: &DescriptorPool,
) -> Result<(), BatchOfError> {
    let pool = DescriptorPool::decode(descriptor_set)
        .map_err(|e| BatchOfError::Descriptors(e.to_string()))?;
    let actual = declarations(&pool);
    let expected = declarations(expected);
    for method in expected.keys().chain(actual.keys()) {
        if expected.get(method) != actual.get(method) {
            return Err(BatchOfError::Descriptors(format!(
                "{method}: batch_of declaration mismatch: expected {:?}, serialized {:?}",
                expected.get(method),
                actual.get(method)
            )));
        }
    }
    check_pool(&pool)
}

fn declarations(pool: &DescriptorPool) -> BTreeMap<String, String> {
    let Some(ext) = pool.get_extension_by_name(BATCH_OF) else {
        return BTreeMap::new();
    };
    let mut declarations = BTreeMap::new();
    for service in pool.services() {
        for method in service.methods() {
            let options = method.options();
            let value = options.get_extension(&ext);
            let Some(named) = value.as_str().filter(|value| !value.is_empty()) else {
                continue;
            };
            declarations.insert(
                format!("{}/{}", service.full_name(), method.name()),
                named.to_owned(),
            );
        }
    }
    declarations
}

fn check_pool(pool: &DescriptorPool) -> Result<(), BatchOfError> {
    let Some(ext) = pool.get_extension_by_name(BATCH_OF) else {
        return Ok(());
    };
    let authz = pool.get_extension_by_name(AUTHZ);
    let errors = pool.get_extension_by_name(ERRORS);
    let mut refusals = Vec::new();
    for service in pool.services() {
        for method in service.methods() {
            let options = method.options();
            if !options.has_extension(&ext) {
                continue;
            }
            let value = options.get_extension(&ext);
            let Some(named) = value.as_str().filter(|v| !v.is_empty()) else {
                continue;
            };
            let site = format!("{}/{}", service.full_name(), method.name());
            if let Err(reason) = resolve(&service, named, &method, authz.as_ref(), errors.as_ref())
            {
                refusals.push(BatchOfRefusal {
                    method: site,
                    batch_of: named.to_owned(),
                    reason,
                });
            }
        }
    }
    if refusals.is_empty() {
        Ok(())
    } else {
        Err(BatchOfError::Refused(refusals))
    }
}

fn resolve(
    service: &ServiceDescriptor,
    named: &str,
    batch: &MethodDescriptor,
    authz: Option<&ExtensionDescriptor>,
    errors: Option<&ExtensionDescriptor>,
) -> Result<(), BatchOfReason> {
    let name = match named.split_once('/') {
        Some((svc, name)) if svc == service.full_name() => name,
        Some(_) => return Err(BatchOfReason::OtherService),
        None => named,
    };
    if name == batch.name() {
        return Err(BatchOfReason::SelfReference);
    }
    let single = service
        .methods()
        .find(|m| m.name() == name)
        .ok_or(BatchOfReason::UnknownMethod)?;
    if authorization(batch, authz) != authorization(&single, authz) {
        return Err(BatchOfReason::AuthzDiffers);
    }
    let (single_level, batch_level) = (idempotency_level(&single), idempotency_level(batch));
    if single_level != batch_level {
        return Err(BatchOfReason::IdempotencyDiffers {
            single: single_level,
            batch: batch_level,
        });
    }
    if declares_already_exists(batch, errors) && !declares_already_exists(&single, errors) {
        return Err(BatchOfReason::AlreadyExistsUndeclared);
    }
    Ok(())
}

/// Whether the RPC lists `ALREADY_EXISTS` in its `bones.v1.errors`.
fn declares_already_exists(
    method: &MethodDescriptor,
    errors: Option<&ExtensionDescriptor>,
) -> bool {
    let Some(ext) = errors else {
        return false;
    };
    let options = method.options();
    options.has_extension(ext)
        && options.get_extension(ext).as_list().is_some_and(|list| {
            list.iter()
                .any(|variant| variant.as_str() == Some(ALREADY_EXISTS))
        })
}

/// The declared `idempotency_level` number; 0 (unknown) when the RPC declares none.
pub(crate) fn idempotency_level(method: &MethodDescriptor) -> i32 {
    method
        .options()
        .get_field_by_name("idempotency_level")
        .and_then(|v| v.as_enum_number())
        .unwrap_or_default()
}

/// The declared authorization: its kind and its `requires` as a set, or `None`
/// when the RPC declares none. An undeclared RPC is denied, so two undeclared
/// RPCs agree, and a declared one never equals an undeclared one.
fn authorization(
    method: &MethodDescriptor,
    authz: Option<&ExtensionDescriptor>,
) -> Option<(i32, BTreeSet<String>)> {
    let ext = authz?;
    let options = method.options();
    if !options.has_extension(ext) {
        return None;
    }
    let value = options.get_extension(ext);
    let rule = value.as_message()?;
    let kind = rule
        .get_field_by_name("kind")
        .and_then(|v| v.as_enum_number())
        .unwrap_or_default();
    let requires = rule
        .get_field_by_name("requires")
        .and_then(|v| {
            v.as_list().map(|l| {
                l.iter()
                    .filter_map(|s| s.as_str().map(str::to_owned))
                    .collect()
            })
        })
        .unwrap_or_default();
    Some((kind, requires))
}

/// Why a descriptor set did not pass [`check_batch_of`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatchOfError {
    /// The set does not decode into a complete pool.
    Descriptors(String),
    /// Every refused declaration.
    Refused(Vec<BatchOfRefusal>),
}

crate::compile::impl_check_error!(BatchOfError);
