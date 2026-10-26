// SPDX-License-Identifier: MIT
//! A field's `meaning` and `classification` options survive compilation and
//! read back through descriptor reflection, each from its own extension.

use prost_reflect::{DescriptorPool, Value};

#[path = "../../tests/shared/shipped_resolver.rs"]
mod shipped_resolver;
use shipped_resolver::{CONTRACT, compiler};

fn compile_record(options: &str) -> DescriptorPool {
    let source = format!(
        "syntax = \"proto3\";\npackage contract.v1;\n\
         import \"bones/v1/annotations.proto\";\n\
         message Record {{ string start_date = 1 {options}; }}\n"
    );
    let mut compiler = compiler(source, api_bones_protos::files());
    compiler
        .open_file(CONTRACT)
        .unwrap_or_else(|e| panic!("contract failed to compile: {e}"));
    compiler.descriptor_pool()
}

fn read_back(pool: &DescriptorPool, extension: &str) -> Option<Value> {
    let ext = pool
        .get_extension_by_name(extension)
        .unwrap_or_else(|| panic!("{extension} is not declared"));
    let field = pool
        .get_message_by_name("contract.v1.Record")
        .expect("record")
        .get_field_by_name("start_date")
        .expect("field");
    let options = field.options();
    options
        .has_extension(&ext)
        .then(|| options.get_extension(&ext).into_owned())
}

#[test]
fn meaning_and_classification_read_back_by_reflection() {
    let pool = compile_record(
        "[(bones.v1.meaning) = \"schema:startDate\", \
          (bones.v1.classification) = DATA_CLASSIFICATION_INTERNAL]",
    );

    let meaning = read_back(&pool, "bones.v1.meaning").expect("meaning option");
    assert_eq!(meaning.as_str(), Some("schema:startDate"));

    let Some(Value::EnumNumber(number)) = read_back(&pool, "bones.v1.classification") else {
        panic!("classification option is not an enum value");
    };
    let name = pool
        .get_enum_by_name("bones.v1.DataClassification")
        .expect("classification enum")
        .get_value(number)
        .expect("a declared value")
        .name()
        .to_owned();
    assert_eq!(name, "DATA_CLASSIFICATION_INTERNAL");
}

#[test]
fn a_field_without_labels_reads_back_neither_option() {
    let pool = compile_record("");
    assert!(read_back(&pool, "bones.v1.meaning").is_none());
    assert!(read_back(&pool, "bones.v1.classification").is_none());
}
