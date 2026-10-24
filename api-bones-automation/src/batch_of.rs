// SPDX-License-Identifier: MIT
//! Conformance check for the `bones.v1.batch_of` method option.

use core::fmt;
use std::collections::BTreeSet;

use prost_reflect::{DescriptorPool, ExtensionDescriptor, MethodDescriptor, ServiceDescriptor};

/// The extension the check reads.
const BATCH_OF: &str = "bones.v1.batch_of";

/// The authorization extension a batch method must share with its single.
const AUTHZ: &str = "bones.v1.authz";

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
}

impl fmt::Display for BatchOfRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let why = match self.reason {
            BatchOfReason::OtherService => "names a method of another service",
            BatchOfReason::UnknownMethod => "names a method the service does not declare",
            BatchOfReason::SelfReference => "names the RPC itself",
            BatchOfReason::AuthzDiffers => "does not keep the authorization of the method it names",
        };
        write!(f, "{}: batch_of \"{}\" {why}", self.method, self.batch_of)
    }
}

/// Check every `bones.v1.batch_of` in `descriptor_set` (a serialized
/// `FileDescriptorSet` that includes `bones/v1/annotations.proto`).
///
/// The value is the bare name of a method of the same service, or that
/// method written `<package>.<Service>/<Method>`. Anything else is refused.
/// A set that does not import the vocabulary declares no `batch_of` and passes.
///
/// # Errors
///
/// [`BatchOfError`] listing every refusal, in service then method order, or
/// the decode failure.
pub fn check_batch_of(descriptor_set: &[u8]) -> Result<(), BatchOfError> {
    let pool = DescriptorPool::decode(descriptor_set)
        .map_err(|e| BatchOfError::Descriptors(e.to_string()))?;
    let Some(ext) = pool.get_extension_by_name(BATCH_OF) else {
        return Ok(());
    };
    let authz = pool.get_extension_by_name(AUTHZ);
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
            if let Err(reason) = resolve(&service, named, &method, authz.as_ref()) {
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
    if authorization(batch, authz) == authorization(&single, authz) {
        Ok(())
    } else {
        Err(BatchOfReason::AuthzDiffers)
    }
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

impl fmt::Display for BatchOfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Descriptors(why) => write!(f, "descriptor set is unusable: {why}"),
            Self::Refused(refusals) => crate::compile::write_refusals(f, refusals),
        }
    }
}

impl std::error::Error for BatchOfError {}
