// SPDX-License-Identifier: MIT
//! The arguments an error enum value declares with `bones.v1.error_args`
//! survive compilation and read back, in order, through descriptor reflection.

use api_bones_protos::error_args::{ErrorArgDecl, ErrorArgKind, declared_error_args};
use prost_reflect::{DescriptorPool, EnumValueDescriptor};

#[path = "../../tests/shared/shipped_resolver.rs"]
mod shipped_resolver;
use shipped_resolver::{CONTRACT, compiler};

fn compile(source: &str) -> DescriptorPool {
    let mut compiler = compiler(source.to_owned(), api_bones_protos::files());
    compiler
        .open_file(CONTRACT)
        .unwrap_or_else(|e| panic!("contract failed to compile: {e}"));
    compiler.descriptor_pool()
}

fn value(pool: &DescriptorPool, name: &str) -> EnumValueDescriptor {
    pool.get_enum_by_name("contract.v1.SeatError")
        .expect("error enum")
        .get_value_by_name(name)
        .unwrap_or_else(|| panic!("{name} is not declared"))
}

const ENUM: &str = "syntax = \"proto3\";\npackage contract.v1;\n\
    import \"bones/v1/annotations.proto\";\n\
    enum SeatError {\n\
      SEAT_ERROR_UNSPECIFIED = 0;\n\
      SEAT_ERROR_NO_ARGS = 1;\n\
      SEAT_ERROR_LIMIT = 2 [\n\
        (bones.v1.error_args) = { name: \"limit\" kind: ERROR_ARG_KIND_INT64 },\n\
        (bones.v1.error_args) = { name: \"plan\" kind: ERROR_ARG_KIND_STRING },\n\
        (bones.v1.error_args) = { name: \"ratio\" kind: ERROR_ARG_KIND_DOUBLE },\n\
        (bones.v1.error_args) = { name: \"ends_at\" kind: ERROR_ARG_KIND_TIMESTAMP }\n\
      ];\n\
      SEAT_ERROR_UNKIND = 3 [(bones.v1.error_args) = { name: \"seat\" }];\n\
    }\n";

#[test]
fn declared_arguments_read_back_in_declaration_order_with_their_kinds() {
    let pool = compile(ENUM);
    let declared = declared_error_args(&value(&pool, "SEAT_ERROR_LIMIT")).expect("declared");
    let expected = [
        ("limit", ErrorArgKind::Int64),
        ("plan", ErrorArgKind::String),
        ("ratio", ErrorArgKind::Double),
        ("ends_at", ErrorArgKind::Timestamp),
    ]
    .map(|(name, kind)| ErrorArgDecl {
        name: name.to_owned(),
        kind,
    });
    assert_eq!(declared, expected);
}

#[test]
fn a_value_declaring_nothing_declares_no_arguments() {
    let pool = compile(ENUM);
    assert_eq!(
        declared_error_args(&value(&pool, "SEAT_ERROR_NO_ARGS")),
        Ok(Vec::new())
    );
}

#[test]
fn a_pool_that_does_not_import_the_annotations_declares_no_arguments() {
    let pool = compile(
        "syntax = \"proto3\";\npackage contract.v1;\n\
         enum SeatError { SEAT_ERROR_UNSPECIFIED = 0; SEAT_ERROR_LIMIT = 1; }\n",
    );
    assert_eq!(
        declared_error_args(&value(&pool, "SEAT_ERROR_LIMIT")),
        Ok(Vec::new())
    );
}

#[test]
fn a_declaration_without_a_kind_is_refused_naming_the_value_and_argument() {
    let pool = compile(ENUM);
    let error = declared_error_args(&value(&pool, "SEAT_ERROR_UNKIND")).expect_err("refused");
    assert_eq!(error.value, "contract.v1.SeatError.SEAT_ERROR_UNKIND");
    assert_eq!(error.name, "seat");
    assert_eq!(error.kind, 0);
    assert_eq!(
        error.to_string(),
        "contract.v1.SeatError.SEAT_ERROR_UNKIND: argument `seat` has no valid kind (ErrorArgKind 0)"
    );
}
