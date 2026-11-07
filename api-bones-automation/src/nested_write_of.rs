// SPDX-License-Identifier: MIT
//! Conformance check for the `bones.v1.nested_write_of` field option.

use core::fmt;

use prost_reflect::{DescriptorPool, FieldDescriptor, Kind, MessageDescriptor, MethodDescriptor};

/// The extension the check reads.
const NESTED_WRITE_OF: &str = "bones.v1.nested_write_of";

/// One `nested_write_of` declaration that is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NestedWriteOfRefusal {
    /// The field that declares the option, as `<package>.<Message>.<field>`.
    pub field: String,
    /// The value the option carries.
    pub nested_write_of: String,
    pub reason: NestedWriteOfReason,
}

/// Why a `nested_write_of` declaration is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NestedWriteOfReason {
    /// No RPC takes the field's message as its input.
    NoRequest,
    /// The value names a method of a different service.
    OtherService,
    /// The service declares no method of that name.
    UnknownMethod,
    /// The value names the RPC whose request carries the field.
    SelfReference,
    /// The field is not a children field: a map, a repeated scalar, or a
    /// singular field that is not a `<Child>Set` holding exactly
    /// `repeated <Child> items = 1`.
    NotChildren,
}

impl fmt::Display for NestedWriteOfRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let why = match self.reason {
            NestedWriteOfReason::NoRequest => "is on a message no RPC takes as its input",
            NestedWriteOfReason::OtherService => "names a method of another service",
            NestedWriteOfReason::UnknownMethod => "names a method the service does not declare",
            NestedWriteOfReason::SelfReference => "names the RPC that carries the field",
            NestedWriteOfReason::NotChildren => {
                "is on a field that is not `repeated <Child>` or `optional <Child>Set`"
            }
        };
        write!(
            f,
            "{}: nested_write_of \"{}\" {why}",
            self.field, self.nested_write_of
        )
    }
}

/// Check every `bones.v1.nested_write_of` in `descriptor_set` (a serialized
/// `FileDescriptorSet` that includes `bones/v1/annotations.proto`).
///
/// The value is the bare name of a method of the same service, or that method
/// written `<package>.<Service>/<Method>`. The field is `repeated <Child>`
/// (create) or `optional <Child>Set` (update) with
/// `message <Child>Set { repeated <Child> items = 1; }`. A set that does not
/// import the vocabulary declares no `nested_write_of` and passes.
///
/// # Errors
///
/// [`NestedWriteOfError`] listing every refusal, in message then field order,
/// or the decode failure.
pub fn check_nested_write_of(descriptor_set: &[u8]) -> Result<(), NestedWriteOfError> {
    let pool = DescriptorPool::decode(descriptor_set)
        .map_err(|e| NestedWriteOfError::Descriptors(e.to_string()))?;
    let Some(ext) = pool.get_extension_by_name(NESTED_WRITE_OF) else {
        return Ok(());
    };
    let mut refusals = Vec::new();
    for message in pool.all_messages() {
        for field in message.fields() {
            let options = field.options();
            if !options.has_extension(&ext) {
                continue;
            }
            let value = options.get_extension(&ext);
            let Some(named) = value.as_str().filter(|v| !v.is_empty()) else {
                continue;
            };
            if let Err(reason) = resolve(&pool, &message, &field, named) {
                refusals.push(NestedWriteOfRefusal {
                    field: field.full_name().to_owned(),
                    nested_write_of: named.to_owned(),
                    reason,
                });
            }
        }
    }
    if refusals.is_empty() {
        Ok(())
    } else {
        Err(NestedWriteOfError::Refused(refusals))
    }
}

fn resolve(
    pool: &DescriptorPool,
    message: &MessageDescriptor,
    field: &FieldDescriptor,
    named: &str,
) -> Result<(), NestedWriteOfReason> {
    let carriers: Vec<MethodDescriptor> = pool
        .services()
        .flat_map(|service| service.methods().collect::<Vec<_>>())
        .filter(|method| method.input().full_name() == message.full_name())
        .collect();
    if carriers.is_empty() {
        return Err(NestedWriteOfReason::NoRequest);
    }
    // The first refusal is reported; one carrier that accepts the name is enough.
    let mut first = None;
    for carrier in &carriers {
        match names_sibling(carrier, named) {
            Ok(()) => {
                return if is_children(field) {
                    Ok(())
                } else {
                    Err(NestedWriteOfReason::NotChildren)
                };
            }
            Err(reason) => {
                first.get_or_insert(reason);
            }
        }
    }
    Err(first.unwrap_or(NestedWriteOfReason::UnknownMethod))
}

fn names_sibling(carrier: &MethodDescriptor, named: &str) -> Result<(), NestedWriteOfReason> {
    let service = carrier.parent_service();
    let name = match named.split_once('/') {
        Some((svc, name)) if svc == service.full_name() => name,
        Some(_) => return Err(NestedWriteOfReason::OtherService),
        None => named,
    };
    if name == carrier.name() {
        return Err(NestedWriteOfReason::SelfReference);
    }
    service
        .methods()
        .find(|m| m.name() == name)
        .map(|_| ())
        .ok_or(NestedWriteOfReason::UnknownMethod)
}

/// `repeated <Child>`, or a `<Child>Set` message holding only
/// `repeated <Child> items = 1`.
fn is_children(field: &FieldDescriptor) -> bool {
    let Kind::Message(child) = field.kind() else {
        return false;
    };
    if field.is_map() {
        return false;
    }
    if field.is_list() {
        return true;
    }
    let mut fields = child.fields();
    let (Some(items), None) = (fields.next(), fields.next()) else {
        return false;
    };
    let Kind::Message(item) = items.kind() else {
        return false;
    };
    items.name() == "items"
        && items.number() == 1
        && items.is_list()
        && !items.is_map()
        && child.name() == format!("{}Set", item.name())
}

/// Why a descriptor set did not pass [`check_nested_write_of`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NestedWriteOfError {
    /// The set does not decode into a complete pool.
    Descriptors(String),
    /// Every refused declaration.
    Refused(Vec<NestedWriteOfRefusal>),
}

crate::compile::impl_check_error!(NestedWriteOfError);
