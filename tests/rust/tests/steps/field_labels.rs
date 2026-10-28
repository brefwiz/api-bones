// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! The Rust half of `../../../features/field_labels.feature`.
//!
//! The TypeScript half answers the same file; neither keeps its own copy of
//! the rows.

use crate::world::FieldLabelsWorld;
use cucumber::{given, then, when};
use prost_reflect::{DescriptorPool, FieldDescriptor, Kind, Value};

#[path = "../../../shared/shipped_resolver.rs"]
mod shipped_resolver;
use shipped_resolver::{CONTRACT, compiler};

fn record_source(field: &str, kind: &str, options: &str) -> String {
    format!(
        "syntax = \"proto3\";\npackage contract.v1;\n\
         import \"bones/v1/annotations.proto\";\n\
         message Record {{ {kind} {field} = 1 {options}; }}\n"
    )
}

fn the_field(pool: &DescriptorPool) -> FieldDescriptor {
    pool.get_message_by_name("contract.v1.Record")
        .expect("record")
        .fields()
        .next()
        .expect("field")
}

fn quoted(value: &str) -> String {
    format!("\"{value}\"")
}

#[given(
    expr = "a record field {string} of type {string} labelled with meaning {string} and classification {string}"
)]
fn given_both(
    world: &mut FieldLabelsWorld,
    field: String,
    kind: String,
    meaning: String,
    classification: String,
) {
    let options = format!(
        "[(bones.v1.meaning) = {}, (bones.v1.classification) = {classification}]",
        quoted(&meaning)
    );
    world.source = record_source(&field, &kind, &options);
}

#[given(
    expr = "a record field {string} of type {string} labelled with classification {string} only"
)]
fn given_classification_only(
    world: &mut FieldLabelsWorld,
    field: String,
    kind: String,
    classification: String,
) {
    let options = format!("[(bones.v1.classification) = {classification}]");
    world.source = record_source(&field, &kind, &options);
}

#[given(expr = "a record field {string} of type {string} labelled with meaning {string} only")]
fn given_meaning_only(world: &mut FieldLabelsWorld, field: String, kind: String, meaning: String) {
    let options = format!("[(bones.v1.meaning) = {}]", quoted(&meaning));
    world.source = record_source(&field, &kind, &options);
}

#[given("the shipped field vocabulary")]
fn given_vocabulary(world: &mut FieldLabelsWorld) {
    world.source = record_source("id", "string", "");
}

#[when("the record is read back through descriptor reflection")]
#[when("the vocabulary is read back through descriptor reflection")]
fn when_read_back(world: &mut FieldLabelsWorld) {
    let mut compiler = compiler(world.source.clone(), api_bones_protos::files());
    compiler
        .open_file(CONTRACT)
        .unwrap_or_else(|e| panic!("contract failed to compile: {e}"));
    world.pool = Some(compiler.descriptor_pool());
}

#[then(expr = "the field meaning reads back as {string}")]
fn then_meaning(world: &mut FieldLabelsWorld, expected: String) {
    let pool = world.pool.as_ref().expect("the record was not read back");
    let ext = pool
        .get_extension_by_name("bones.v1.meaning")
        .expect("meaning extension");
    let value = the_field(pool).options().get_extension(&ext).into_owned();
    assert_eq!(value.as_str(), Some(expected.as_str()));
}

#[then("the field carries no meaning")]
fn then_no_meaning(world: &mut FieldLabelsWorld) {
    let pool = world.pool.as_ref().expect("the record was not read back");
    let ext = pool
        .get_extension_by_name("bones.v1.meaning")
        .expect("meaning extension");
    assert!(
        !the_field(pool).options().has_extension(&ext),
        "the field carries a meaning"
    );
}

#[then(expr = "the field classification reads back as {string}")]
fn then_classification(world: &mut FieldLabelsWorld, expected: String) {
    let pool = world.pool.as_ref().expect("the record was not read back");
    let ext = pool
        .get_extension_by_name("bones.v1.classification")
        .expect("classification extension");
    let value = the_field(pool).options().get_extension(&ext).into_owned();
    let Value::EnumNumber(number) = value else {
        panic!("classification is not an enum value");
    };
    let name = pool
        .get_enum_by_name("bones.v1.DataClassification")
        .expect("classification enum")
        .get_value(number)
        .expect("a declared value")
        .name()
        .to_owned();
    assert_eq!(name, expected);
}

#[then("the field carries no classification")]
fn then_no_classification(world: &mut FieldLabelsWorld) {
    let pool = world.pool.as_ref().expect("the record was not read back");
    let ext = pool
        .get_extension_by_name("bones.v1.classification")
        .expect("classification extension");
    assert!(
        !the_field(pool).options().has_extension(&ext),
        "the field carries a classification"
    );
}

#[then(expr = "the field value type reads back as {string}")]
fn then_value_type(world: &mut FieldLabelsWorld, expected: String) {
    let pool = world.pool.as_ref().expect("the record was not read back");
    let kind = match the_field(pool).kind() {
        Kind::String => "string",
        Kind::Int32 => "int32",
        Kind::Int64 => "int64",
        Kind::Bool => "bool",
        other => panic!("the contract names no row for {other:?}"),
    };
    assert_eq!(kind, expected);
}

fn names(mut found: Vec<String>) -> String {
    found.sort();
    found.join(", ")
}

#[then(expr = "the field options it declares are {string}")]
fn then_field_options(world: &mut FieldLabelsWorld, expected: String) {
    let pool = world.pool.as_ref().expect("the record was not read back");
    let found = pool
        .all_extensions()
        .filter(|e| {
            e.package_name() == "bones.v1"
                && e.containing_message().full_name() == "google.protobuf.FieldOptions"
        })
        .map(|e| e.name().to_owned())
        .collect();
    assert_eq!(names(found), expected);
}

#[then(expr = "the classification enums it declares are {string}")]
fn then_classification_enums(world: &mut FieldLabelsWorld, expected: String) {
    let pool = world.pool.as_ref().expect("the record was not read back");
    let found = pool
        .all_enums()
        .filter(|e| e.package_name() == "bones.v1" && e.name().contains("Classification"))
        .map(|e| e.full_name().to_owned())
        .collect();
    assert_eq!(names(found), expected);
}
