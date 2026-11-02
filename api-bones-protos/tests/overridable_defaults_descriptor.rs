// SPDX-License-Identifier: MIT
//! A message's `instance_of` and a field's `defaulted_from` options survive
//! compilation and read back through descriptor reflection.

use prost_reflect::DescriptorPool;

#[path = "../../tests/shared/shipped_resolver.rs"]
mod shipped_resolver;
use shipped_resolver::{CONTRACT, compiler};

fn compile_slot(message_options: &str, field_options: &str) -> DescriptorPool {
    let source = format!(
        "syntax = \"proto3\";\npackage contract.v1;\n\
         import \"bones/v1/annotations.proto\";\n\
         message ScheduleRule {{ string start_time = 1; }}\n\
         message Slot {{ {message_options}\n  string start_time = 1 {field_options}; }}\n"
    );
    let mut compiler = compiler(source, api_bones_protos::files());
    compiler
        .open_file(CONTRACT)
        .unwrap_or_else(|e| panic!("contract failed to compile: {e}"));
    compiler.descriptor_pool()
}

fn instance_of(pool: &DescriptorPool) -> Option<String> {
    let ext = pool
        .get_extension_by_name("bones.v1.instance_of")
        .expect("instance_of is declared");
    let options = pool
        .get_message_by_name("contract.v1.Slot")
        .expect("slot")
        .options();
    options.has_extension(&ext).then(|| {
        options
            .get_extension(&ext)
            .as_str()
            .map(str::to_owned)
            .expect("a string")
    })
}

fn defaulted_from(pool: &DescriptorPool) -> Option<String> {
    let ext = pool
        .get_extension_by_name("bones.v1.defaulted_from")
        .expect("defaulted_from is declared");
    let field = pool
        .get_message_by_name("contract.v1.Slot")
        .expect("slot")
        .get_field_by_name("start_time")
        .expect("field");
    let options = field.options();
    options.has_extension(&ext).then(|| {
        options
            .get_extension(&ext)
            .as_str()
            .map(str::to_owned)
            .expect("a string")
    })
}

#[test]
fn instance_of_and_defaulted_from_read_back_by_reflection() {
    let pool = compile_slot(
        "option (bones.v1.instance_of) = \"contract.v1.ScheduleRule\";",
        "[(bones.v1.defaulted_from) = \"contract.v1.ScheduleRule.start_time\"]",
    );
    assert_eq!(
        instance_of(&pool).as_deref(),
        Some("contract.v1.ScheduleRule")
    );
    assert_eq!(
        defaulted_from(&pool).as_deref(),
        Some("contract.v1.ScheduleRule.start_time")
    );
}

#[test]
fn a_message_and_field_without_the_options_read_back_neither() {
    let pool = compile_slot("", "");
    assert!(instance_of(&pool).is_none());
    assert!(defaulted_from(&pool).is_none());
}

#[test]
fn each_option_reads_back_without_the_other() {
    let pool = compile_slot(
        "option (bones.v1.instance_of) = \"contract.v1.ScheduleRule\";",
        "",
    );
    assert!(instance_of(&pool).is_some());
    assert!(defaulted_from(&pool).is_none());

    let pool = compile_slot(
        "",
        "[(bones.v1.defaulted_from) = \"contract.v1.ScheduleRule.start_time\"]",
    );
    assert!(instance_of(&pool).is_none());
    assert!(defaulted_from(&pool).is_some());
}
