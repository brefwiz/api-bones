// SPDX-License-Identifier: MIT
//! A provider's invitation reference namespaces survive compilation and read
//! back through descriptor reflection.

use api_bones_protos::files;
use prost_reflect::DescriptorPool;

#[path = "../../tests/shared/shipped_resolver.rs"]
mod shipped_resolver;
use shipped_resolver::{CONTRACT, compiler};

fn compile(source: &str) -> DescriptorPool {
    let mut compiler = compiler(source.to_owned(), files());
    compiler
        .open_file(CONTRACT)
        .unwrap_or_else(|e| panic!("contract failed to compile: {e}"));
    compiler.descriptor_pool()
}

fn namespaces_of(pool: &DescriptorPool, service: &str) -> Vec<String> {
    let extension = pool
        .get_extension_by_name("bones.v1.invitation_reference_namespaces")
        .expect("the extension is declared");
    assert_eq!(extension.number(), 5_102_381);
    let options = pool
        .get_service_by_name(service)
        .expect("service")
        .options();
    options
        .get_extension(&extension)
        .as_list()
        .expect("a repeated option")
        .iter()
        .map(|v| v.as_str().expect("a string").to_owned())
        .collect()
}

#[test]
fn declared_namespaces_read_back_in_order() {
    let pool = compile(
        "syntax = \"proto3\";\npackage p.v1;\nimport \"bones/v1/annotations.proto\";\n         message Req {}\n         service S {\n  option (bones.v1.invitation_reference_namespaces) = \"booking\";\n           option (bones.v1.invitation_reference_namespaces) = \"waitlist\";\n           rpc Open(Req) returns (Req);\n}\n",
    );
    assert_eq!(namespaces_of(&pool, "p.v1.S"), ["booking", "waitlist"]);
}

#[test]
fn an_undeclared_service_answers_for_no_namespace() {
    let pool = compile(
        "syntax = \"proto3\";\npackage p.v1;\nimport \"bones/v1/annotations.proto\";\n         message Req {}\n         service S {\n  rpc Open(Req) returns (Req);\n}\n",
    );
    assert_eq!(namespaces_of(&pool, "p.v1.S").len(), 0);
}
