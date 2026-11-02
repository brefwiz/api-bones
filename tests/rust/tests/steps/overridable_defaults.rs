// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! The Rust half of `../../../features/overridable_defaults.feature`.
//!
//! The TypeScript half answers the same file; neither keeps its own copy of
//! the rows.

use crate::world::OverridableDefaultsWorld;
use cucumber::{given, then, when};
use prost_reflect::{DescriptorPool, ExtensionDescriptor, MessageDescriptor};

use super::shipped_resolver::{CONTRACT, compiler};

/// The last dotted segment of a fully-qualified name.
fn leaf(name: &str) -> &str {
    name.rsplit('.').next().expect("a name")
}

struct Fixture<'a> {
    instance: &'a str,
    message_options: &'a str,
    field: &'a str,
    field_options: &'a str,
    origin: &'a str,
    origin_field: &'a str,
}

impl Fixture<'_> {
    fn source(&self) -> String {
        format!(
            "syntax = \"proto3\";\npackage contract.v1;\n\
             import \"bones/v1/annotations.proto\";\n\
             message {origin} {{ string {origin_field} = 1; }}\n\
             message {instance} {{ {message_options}\n  string {field} = 1 {field_options}; }}\n",
            origin = leaf(self.origin),
            origin_field = self.origin_field,
            instance = self.instance,
            message_options = self.message_options,
            field = self.field,
            field_options = self.field_options,
        )
    }
}

fn the_instance(pool: &DescriptorPool, name: &str) -> MessageDescriptor {
    pool.get_message_by_name(&format!("contract.v1.{name}"))
        .expect("instance")
}

fn extension(pool: &DescriptorPool, name: &str) -> ExtensionDescriptor {
    pool.get_extension_by_name(name)
        .unwrap_or_else(|| panic!("{name} is not declared"))
}

#[given(expr = "an instance {string} of {string} whose field {string} is defaulted from {string}")]
fn given_instance(
    world: &mut OverridableDefaultsWorld,
    instance: String,
    origin: String,
    field: String,
    origin_field: String,
) {
    world.source = Fixture {
        instance: &instance,
        message_options: &format!("option (bones.v1.instance_of) = \"{origin}\";"),
        field: &field,
        field_options: &format!("[(bones.v1.defaulted_from) = \"{origin_field}\"]"),
        origin: &origin,
        origin_field: leaf(&origin_field),
    }
    .source();
    world.instance = instance;
}

#[given(expr = "a plain message {string} with a field {string}")]
fn given_plain(world: &mut OverridableDefaultsWorld, message: String, field: String) {
    world.source = Fixture {
        instance: &message,
        message_options: "",
        field: &field,
        field_options: "",
        origin: "contract.v1.Origin",
        origin_field: &field,
    }
    .source();
    world.instance = message;
}

#[given(expr = "an instance {string} of {string} whose field {string} has no default origin")]
fn given_no_default_origin(
    world: &mut OverridableDefaultsWorld,
    instance: String,
    origin: String,
    field: String,
) {
    world.source = Fixture {
        instance: &instance,
        message_options: &format!("option (bones.v1.instance_of) = \"{origin}\";"),
        field: &field,
        field_options: "",
        origin: &origin,
        origin_field: &field,
    }
    .source();
    world.instance = instance;
}

#[when("the instance is read back through descriptor reflection")]
fn when_read_back(world: &mut OverridableDefaultsWorld) {
    let mut compiler = compiler(world.source.clone(), api_bones_protos::files());
    compiler
        .open_file(CONTRACT)
        .unwrap_or_else(|e| panic!("contract failed to compile: {e}"));
    world.pool = Some(compiler.descriptor_pool());
}

#[then(expr = "the instance reads back as an instance of {string}")]
fn then_instance_of(world: &mut OverridableDefaultsWorld, expected: String) {
    let pool = world.pool.as_ref().expect("the instance was not read back");
    let ext = extension(pool, "bones.v1.instance_of");
    let value = the_instance(pool, &world.instance)
        .options()
        .get_extension(&ext)
        .into_owned();
    assert_eq!(value.as_str(), Some(expected.as_str()));
}

#[then("the message carries no instance origin")]
fn then_no_instance_of(world: &mut OverridableDefaultsWorld) {
    let pool = world.pool.as_ref().expect("the instance was not read back");
    let ext = extension(pool, "bones.v1.instance_of");
    assert!(
        !the_instance(pool, &world.instance)
            .options()
            .has_extension(&ext),
        "the message carries an instance origin"
    );
}

#[then(expr = "the field default reads back as originating from {string}")]
fn then_defaulted_from(world: &mut OverridableDefaultsWorld, expected: String) {
    let pool = world.pool.as_ref().expect("the instance was not read back");
    let ext = extension(pool, "bones.v1.defaulted_from");
    let field = the_instance(pool, &world.instance)
        .fields()
        .next()
        .expect("field");
    let value = field.options().get_extension(&ext).into_owned();
    assert_eq!(value.as_str(), Some(expected.as_str()));
}

#[then("the field carries no default origin")]
fn then_no_defaulted_from(world: &mut OverridableDefaultsWorld) {
    let pool = world.pool.as_ref().expect("the instance was not read back");
    let ext = extension(pool, "bones.v1.defaulted_from");
    let field = the_instance(pool, &world.instance)
        .fields()
        .next()
        .expect("field");
    assert!(
        !field.options().has_extension(&ext),
        "the field carries a default origin"
    );
}
