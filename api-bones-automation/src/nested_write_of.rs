// SPDX-License-Identifier: MIT
//! Conformance check for the `bones.v1.nested_write_of` field option.

use core::fmt;

use prost_reflect::{DescriptorPool, FieldDescriptor, Kind, MessageDescriptor, MethodDescriptor};

use crate::batch_of::{NO_SIDE_EFFECTS, idempotency_level};

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
    /// An RPC that takes the field's message as its input declares
    /// `idempotency_level = NO_SIDE_EFFECTS`: a read writes no children.
    ReadRequest,
    /// The value names a method of a different service.
    OtherService,
    /// The service declares no method of that name.
    UnknownMethod,
    /// The value names the RPC whose request carries the field.
    SelfReference,
    /// The named method is not a single child write: it streams, or it
    /// declares `idempotency_level = NO_SIDE_EFFECTS`.
    NotUnaryWrite,
    /// The field is not a children field: a map, a repeated scalar, or a
    /// singular field that is not a `<Child>Set` holding exactly
    /// `repeated <Child> items = 1`.
    NotChildren,
    /// The children's message is not the child the named method writes: it
    /// is neither that method's request message nor the type of exactly one
    /// singular message field of that request.
    ChildMismatch,
}

impl fmt::Display for NestedWriteOfRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let why = match self.reason {
            NestedWriteOfReason::NoRequest => "is on a message no RPC takes as its input",
            NestedWriteOfReason::ReadRequest => {
                "is on the request of a method that declares NO_SIDE_EFFECTS"
            }
            NestedWriteOfReason::OtherService => "names a method of another service",
            NestedWriteOfReason::UnknownMethod => "names a method the service does not declare",
            NestedWriteOfReason::SelfReference => "names the RPC that carries the field",
            NestedWriteOfReason::NotUnaryWrite => {
                "names a method that is not a unary write (it streams or declares NO_SIDE_EFFECTS)"
            }
            NestedWriteOfReason::NotChildren => {
                "is on a field that is not `repeated <Child>` or `optional <Child>Set`"
            }
            NestedWriteOfReason::ChildMismatch => {
                "is on children of a message the named method does not write"
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
/// written `<package>.<Service>/<Method>`, and that method is the single child
/// write: unary, and not `NO_SIDE_EFFECTS`. The field is `repeated <Child>`
/// (create) or `optional <Child>Set` (update) with
/// `message <Child>Set { repeated <Child> items = 1; }`, and `<Child>` is the
/// message the named method writes: its request message itself, or the type
/// of the one singular message field of that type in its request. No RPC that
/// takes the field's message declares `NO_SIDE_EFFECTS`. Every RPC that takes
/// the field's message must pass the whole check against its own service;
/// the first refusal is reported, carriers ordered by fully-qualified method
/// name so the verdict does not depend on declaration order. A set that does not import the
/// vocabulary declares no `nested_write_of` and passes.
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
    if carriers
        .iter()
        .any(|carrier| idempotency_level(carrier) == NO_SIDE_EFFECTS)
    {
        return Err(NestedWriteOfReason::ReadRequest);
    }
    // Every carrier must pass the whole check; the first refusal in carrier
    // order (fully-qualified method name) is reported, not declaration order.
    let mut carriers = carriers;
    carriers.sort_by(|a, b| a.full_name().cmp(b.full_name()));
    for carrier in &carriers {
        let single = names_sibling(carrier, named)?;
        let child = children_of(field).ok_or(NestedWriteOfReason::NotChildren)?;
        if !writes(&single, &child) {
            return Err(NestedWriteOfReason::ChildMismatch);
        }
    }
    Ok(())
}

fn names_sibling(
    carrier: &MethodDescriptor,
    named: &str,
) -> Result<MethodDescriptor, NestedWriteOfReason> {
    let service = carrier.parent_service();
    let name = match named.split_once('/') {
        Some((svc, name)) if svc == service.full_name() => name,
        Some(_) => return Err(NestedWriteOfReason::OtherService),
        None => named,
    };
    if name == carrier.name() {
        return Err(NestedWriteOfReason::SelfReference);
    }
    let method = service
        .methods()
        .find(|m| m.name() == name)
        .ok_or(NestedWriteOfReason::UnknownMethod)?;
    if method.is_client_streaming()
        || method.is_server_streaming()
        || idempotency_level(&method) == NO_SIDE_EFFECTS
    {
        return Err(NestedWriteOfReason::NotUnaryWrite);
    }
    Ok(method)
}

/// The `<Child>` of `repeated <Child>`, or of a `<Child>Set` message holding
/// only `repeated <Child> items = 1`; `None` for any other field.
fn children_of(field: &FieldDescriptor) -> Option<MessageDescriptor> {
    let Kind::Message(child) = field.kind() else {
        return None;
    };
    if field.is_map() {
        return None;
    }
    if field.is_list() {
        return Some(child);
    }
    let mut fields = child.fields();
    let (Some(items), None) = (fields.next(), fields.next()) else {
        return None;
    };
    let Kind::Message(item) = items.kind() else {
        return None;
    };
    (items.name() == "items"
        && items.number() == 1
        && items.is_list()
        && !items.is_map()
        && child.name() == format!("{}Set", item.name()))
    .then_some(item)
}

/// Whether `single` writes `child`: its request is `child`, or its request
/// has exactly one singular message field of type `child`. Two or more such
/// fields leave the slot ambiguous.
fn writes(single: &MethodDescriptor, child: &MessageDescriptor) -> bool {
    let input = single.input();
    input.full_name() == child.full_name()
        || input
            .fields()
            .filter(|field| {
                !field.is_list()
                    && !field.is_map()
                    && matches!(field.kind(), Kind::Message(m) if m.full_name() == child.full_name())
            })
            .count()
            == 1
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
