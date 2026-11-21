// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! The Rust half of `../../../features/field_labels.feature`.
//!
//! The TypeScript half answers the same file; neither keeps its own copy of
//! the rows.

use crate::world::FieldLabelsWorld;
use cucumber::{given, then, when};
use prost_reflect::{DescriptorPool, FieldDescriptor, Kind, Value};

use super::shipped_resolver::{CONTRACT, compiler};

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

fn references_of(pool: &DescriptorPool) -> prost_reflect::DynamicMessage {
    let ext = pool
        .get_extension_by_name("bones.v1.references")
        .expect("references extension");
    let Value::Message(message) = the_field(pool).options().get_extension(&ext).into_owned() else {
        panic!("references is not a message");
    };
    message
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

#[given(expr = "a record field {string} of type {string} labelled with value format {string}")]
fn given_format(world: &mut FieldLabelsWorld, field: String, kind: String, format: String) {
    let options = format!("[(bones.v1.format) = {format}]");
    world.source = record_source(&field, &kind, &options);
}

#[given(
    expr = "a record field {string} of type {string} labelled with a reference to the field {string}"
)]
fn given_field_reference(
    world: &mut FieldLabelsWorld,
    field: String,
    kind: String,
    target: String,
) {
    let options = format!("[(bones.v1.references).field = {}]", quoted(&target));
    world.source = record_source(&field, &kind, &options);
}

#[given(
    expr = "a record field {string} of type {string} labelled as a principal of the caller's organization"
)]
fn given_principal_reference(world: &mut FieldLabelsWorld, field: String, kind: String) {
    let options = "[(bones.v1.references).principal = true]";
    world.source = record_source(&field, &kind, options);
}

#[given(
    expr = "a record field {string} of type {string} labelled with a reference to both a field and a principal"
)]
fn given_both_references(world: &mut FieldLabelsWorld, field: String, kind: String) {
    let options = "[(bones.v1.references) = { field: \"acme.v1.Event.id\" principal: true }]";
    world.source = record_source(&field, &kind, options);
}

#[given(
    expr = "a contract enum {string} whose value {string} is labelled output-only beside {string}"
)]
fn given_output_only_enum(
    world: &mut FieldLabelsWorld,
    name: String,
    labelled: String,
    siblings: String,
) {
    let mut values: Vec<String> = siblings
        .split(", ")
        .enumerate()
        .map(|(number, value)| format!("{value} = {number};"))
        .collect();
    values.push(format!(
        "{labelled} = {} [(bones.v1.output_only) = true];",
        values.len()
    ));
    world.source = format!(
        "syntax = \"proto3\";\npackage contract.v1;\n\
         import \"bones/v1/annotations.proto\";\n\
         enum {name} {{ {} }}\n\
         message Record {{ string id = 1; }}\n",
        values.join(" ")
    );
}

#[given(
    expr = "a contract enum {string} whose value {string} declares the error arguments {string} beside {string}"
)]
fn given_error_args_enum(
    world: &mut FieldLabelsWorld,
    name: String,
    declaring: String,
    arguments: String,
    siblings: String,
) {
    let mut values: Vec<String> = siblings
        .split(", ")
        .enumerate()
        .map(|(number, value)| format!("{value} = {number};"))
        .collect();
    let declarations: Vec<String> = arguments
        .split(", ")
        .map(|argument| {
            let (arg, kind) = argument.split_once(':').expect("name:kind");
            format!(
                "(bones.v1.error_args) = {{ name: \"{arg}\" kind: ERROR_ARG_KIND_{} }}",
                kind.to_uppercase()
            )
        })
        .collect();
    values.push(format!(
        "{declaring} = {} [{}];",
        values.len(),
        declarations.join(", ")
    ));
    world.source = format!(
        "syntax = \"proto3\";\npackage contract.v1;\n\
         import \"bones/v1/annotations.proto\";\n\
         enum {name} {{ {} }}\n\
         message Record {{ string id = 1; }}\n",
        values.join(" ")
    );
}

#[given("the shipped field vocabulary")]
fn given_vocabulary(world: &mut FieldLabelsWorld) {
    world.source = record_source("id", "string", "");
}

#[given("the shipped stream control")]
fn given_stream_control(world: &mut FieldLabelsWorld) {
    world.source = "syntax = \"proto3\";\npackage contract.v1;\n\
         import \"bones/v1/stream.proto\";\n\
         message Record { string id = 1; }\n"
        .to_owned();
}

#[given("the shipped pagination")]
fn given_pagination(world: &mut FieldLabelsWorld) {
    world.source = "syntax = \"proto3\";\npackage contract.v1;\n\
         import \"bones/v1/pagination.proto\";\n\
         message Record { string id = 1; }\n"
        .to_owned();
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

#[when("the contract is compiled")]
fn when_compiled(world: &mut FieldLabelsWorld) {
    let mut compiler = compiler(world.source.clone(), api_bones_protos::files());
    world.refusal = compiler.open_file(CONTRACT).err().map(|e| {
        let mut chain = e.to_string();
        let mut cause = miette::Diagnostic::diagnostic_source(&e);
        while let Some(diagnostic) = cause {
            chain.push_str(": ");
            chain.push_str(&diagnostic.to_string());
            cause = diagnostic.diagnostic_source();
        }
        chain
    });
}

#[then(
    expr = "the contract is refused at compile because {string} and {string} are mutually exclusive in {string}"
)]
fn then_refused(world: &mut FieldLabelsWorld, first: String, second: String, option: String) {
    let message = world.refusal.as_ref().expect("the contract compiled");
    assert!(
        message.contains("a value is already set for oneof 'target'")
            && message.contains(&format!("(bones.v1.{option})")),
        "refusal does not name the {first:?}/{second:?} conflict in {option:?}: {message}"
    );
}

#[then(expr = "the field value format reads back as {string}")]
fn then_format(world: &mut FieldLabelsWorld, expected: String) {
    let pool = world.pool.as_ref().expect("the record was not read back");
    let ext = pool
        .get_extension_by_name("bones.v1.format")
        .expect("format extension");
    let Value::EnumNumber(number) = the_field(pool).options().get_extension(&ext).into_owned()
    else {
        panic!("format is not an enum value");
    };
    let name = pool
        .get_enum_by_name("bones.v1.ValueFormat")
        .expect("format enum")
        .get_value(number)
        .expect("a declared value")
        .name()
        .to_owned();
    assert_eq!(name, expected);
}

fn output_only_of(pool: &DescriptorPool, enum_name: &str, value: &str) -> Option<bool> {
    let ext = pool
        .get_extension_by_name("bones.v1.output_only")
        .expect("output_only extension");
    let options = pool
        .get_enum_by_name(enum_name)
        .unwrap_or_else(|| panic!("{enum_name} is not declared"))
        .get_value_by_name(value)
        .unwrap_or_else(|| panic!("{value} is not declared"))
        .options();
    options
        .has_extension(&ext)
        .then(|| options.get_extension(&ext).as_bool().expect("a bool"))
}

#[then(expr = "the enum value {string} of {string} reads back as output-only")]
fn then_output_only(world: &mut FieldLabelsWorld, value: String, enum_name: String) {
    let pool = world.pool.as_ref().expect("the record was not read back");
    assert_eq!(output_only_of(pool, &enum_name, &value), Some(true));
}

#[then(expr = "the enum value {string} of {string} is not output-only")]
fn then_not_output_only(world: &mut FieldLabelsWorld, value: String, enum_name: String) {
    let pool = world.pool.as_ref().expect("the record was not read back");
    assert_eq!(output_only_of(pool, &enum_name, &value), None);
}

#[then(expr = "the enum value {string} of {string} reads back the error arguments {string}")]
fn then_error_args(
    world: &mut FieldLabelsWorld,
    value: String,
    enum_name: String,
    expected: String,
) {
    let pool = world.pool.as_ref().expect("the record was not read back");
    let value = pool
        .get_enum_by_name(&enum_name)
        .unwrap_or_else(|| panic!("{enum_name} is not declared"))
        .get_value_by_name(&value)
        .unwrap_or_else(|| panic!("{value} is not declared"));
    let declared = api_bones_protos::error_args::declared_error_args(&value)
        .unwrap_or_else(|e| panic!("the declaration is refused: {e}"));
    let found: Vec<String> = declared
        .iter()
        .map(|decl| {
            let kind = format!("{:?}", decl.kind).to_lowercase();
            format!("{}:{kind}", decl.name)
        })
        .collect();
    assert_eq!(found.join(", "), expected);
}

#[then(expr = "the enum value options it declares are {string}")]
fn then_enum_value_options(world: &mut FieldLabelsWorld, expected: String) {
    let pool = world.pool.as_ref().expect("the record was not read back");
    let found = pool
        .all_extensions()
        .filter(|e| {
            e.package_name() == "bones.v1"
                && e.containing_message().full_name() == "google.protobuf.EnumValueOptions"
        })
        .map(|e| e.name().to_owned())
        .collect();
    assert_eq!(names(found), expected);
}

#[then(expr = "the field reference reads back as the field {string}")]
fn then_field_reference(world: &mut FieldLabelsWorld, expected: String) {
    let pool = world.pool.as_ref().expect("the record was not read back");
    let message = references_of(pool);
    assert!(
        !message.has_field_by_name("principal"),
        "a principal is set"
    );
    let value = message.get_field_by_name("field").expect("field target");
    assert_eq!(value.as_str(), Some(expected.as_str()));
}

#[then("the field reference reads back as a principal of the caller's organization")]
fn then_principal_reference(world: &mut FieldLabelsWorld) {
    let pool = world.pool.as_ref().expect("the record was not read back");
    let message = references_of(pool);
    assert!(!message.has_field_by_name("field"), "a field is set");
    let value = message
        .get_field_by_name("principal")
        .expect("principal target");
    assert_eq!(value.as_bool(), Some(true));
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

#[then(expr = "every field of the message {string} is classified {string}")]
fn then_every_field_classified(world: &mut FieldLabelsWorld, message: String, expected: String) {
    let pool = world.pool.as_ref().expect("the record was not read back");
    let ext = pool
        .get_extension_by_name("bones.v1.classification")
        .expect("classification extension");
    let enum_type = pool
        .get_enum_by_name("bones.v1.DataClassification")
        .expect("classification enum");
    let message = pool
        .get_message_by_name(&message)
        .unwrap_or_else(|| panic!("{message} is not declared"));
    assert!(message.fields().count() > 0, "the message has no fields");
    for field in message.fields() {
        let Value::EnumNumber(number) = field.options().get_extension(&ext).into_owned() else {
            panic!("{} carries no classification", field.full_name());
        };
        let name = enum_type.get_value(number).expect("a declared value");
        assert_eq!(name.name(), expected, "{}", field.full_name());
    }
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

fn field_names(message: &prost_reflect::MessageDescriptor) -> String {
    message
        .fields()
        .map(|f| f.name().to_owned())
        .collect::<Vec<_>>()
        .join(", ")
}

#[then(expr = "the message {string} declares the oneof {string} holding exactly {string}")]
fn then_oneof(world: &mut FieldLabelsWorld, message: String, oneof: String, expected: String) {
    let pool = world.pool.as_ref().expect("the record was not read back");
    let message = pool
        .get_message_by_name(&message)
        .unwrap_or_else(|| panic!("{message} is not declared"));
    assert_eq!(message.oneofs().count(), 1, "one oneof");
    let signal = message.oneofs().next().expect("oneof");
    assert_eq!(signal.name(), oneof);
    let members = signal
        .fields()
        .map(|f| f.name().to_owned())
        .collect::<Vec<_>>()
        .join(", ");
    assert_eq!(members, expected);
    assert_eq!(
        field_names(&message),
        members,
        "every field belongs to the oneof"
    );
}

#[then(expr = "the enum {string} declares exactly the values {string}")]
fn then_enum_values(world: &mut FieldLabelsWorld, name: String, expected: String) {
    let pool = world.pool.as_ref().expect("the record was not read back");
    let found = pool
        .get_enum_by_name(&name)
        .unwrap_or_else(|| panic!("{name} is not declared"))
        .values()
        .map(|v| format!("{}={}", v.name(), v.number()))
        .collect::<Vec<_>>()
        .join(", ");
    assert_eq!(found, expected);
}

#[then(expr = "the message {string} declares exactly the fields {string}")]
fn then_fields(world: &mut FieldLabelsWorld, message: String, expected: String) {
    let pool = world.pool.as_ref().expect("the record was not read back");
    let message = pool
        .get_message_by_name(&message)
        .unwrap_or_else(|| panic!("{message} is not declared"));
    assert_eq!(field_names(&message), expected);
}
