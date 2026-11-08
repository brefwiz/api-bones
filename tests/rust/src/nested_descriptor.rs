// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! The nested-write declaration read back from the generated descriptor.

use prost_reflect::{DescriptorPool, ExtensionDescriptor, FieldDescriptor, Kind};

use crate::CONTRACT_DESCRIPTOR;

fn pool() -> DescriptorPool {
    DescriptorPool::decode(CONTRACT_DESCRIPTOR).expect("generated descriptor")
}

fn extension(pool: &DescriptorPool) -> ExtensionDescriptor {
    pool.get_extension_by_name("bones.v1.nested_write_of")
        .expect("nested_write_of extension")
}

fn field(pool: &DescriptorPool, message: &str, name: &str) -> FieldDescriptor {
    pool.get_message_by_name(message)
        .unwrap_or_else(|| panic!("message {message}"))
        .get_field_by_name(name)
        .unwrap_or_else(|| panic!("field {message}.{name}"))
}

fn declared(pool: &DescriptorPool, field: &FieldDescriptor) -> Option<String> {
    let extension = extension(pool);
    let options = field.options();
    options
        .has_extension(&extension)
        .then(|| {
            options
                .get_extension(&extension)
                .as_str()
                .map(str::to_owned)
        })
        .flatten()
}

#[test]
fn extension_carries_the_reserved_number() {
    assert_eq!(extension(&pool()).number(), 5_102_373);
}

#[test]
fn create_and_update_children_name_the_single_child_write() {
    let pool = pool();
    for message in [
        "contract.v1.CreateWidgetRequest",
        "contract.v1.UpdateWidgetRequest",
    ] {
        assert_eq!(
            declared(&pool, &field(&pool, message, "parts")).as_deref(),
            Some("PutWidgetPart"),
            "{message}.parts"
        );
    }
    let service = pool
        .get_service_by_name("contract.v1.WidgetAssembly")
        .expect("WidgetAssembly service");
    assert!(
        service
            .methods()
            .any(|method| method.name() == "PutWidgetPart"),
        "the named single child write is a method of the same service"
    );
}

#[test]
fn create_children_are_a_repeated_child() {
    let pool = pool();
    let parts = field(&pool, "contract.v1.CreateWidgetRequest", "parts");
    assert!(parts.is_list());
    match parts.kind() {
        Kind::Message(child) => assert_eq!(child.full_name(), "contract.v1.WidgetPart"),
        other => panic!("create parts is {other:?}, not a message"),
    }
}

#[test]
fn update_children_are_an_optional_set_of_one_repeated_field() {
    let pool = pool();
    let parts = field(&pool, "contract.v1.UpdateWidgetRequest", "parts");
    assert!(!parts.is_list());
    assert!(parts.supports_presence());
    let Kind::Message(set) = parts.kind() else {
        panic!("update parts is not a message");
    };
    assert_eq!(set.full_name(), "contract.v1.WidgetPartSet");
    let fields: Vec<_> = set.fields().collect();
    assert_eq!(fields.len(), 1, "the set declares exactly one field");
    let items = &fields[0];
    assert_eq!((items.name(), items.number()), ("items", 1));
    assert!(items.is_list());
    match items.kind() {
        Kind::Message(child) => assert_eq!(child.full_name(), "contract.v1.WidgetPart"),
        other => panic!("set items is {other:?}, not a message"),
    }
}

#[test]
fn a_field_without_the_option_reads_it_as_absent() {
    let pool = pool();
    for (message, name) in [
        ("contract.v1.CreateWidgetRequest", "name"),
        ("contract.v1.UpdateWidgetRequest", "id"),
        ("contract.v1.PutWidgetPartRequest", "part"),
        ("contract.v1.WidgetPartSet", "items"),
    ] {
        assert_eq!(
            declared(&pool, &field(&pool, message, name)),
            None,
            "{message}.{name}"
        );
    }
}
