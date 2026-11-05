// SPDX-License-Identifier: MIT
//! Generates the canonical field messages from the shipped proto contract.
use std::path::PathBuf;

fn main() {
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo OUT_DIR"));
    let staged = out.join("proto");
    for (name, bytes) in api_bones_protos::files() {
        let path = staged.join(name);
        std::fs::create_dir_all(path.parent().expect("proto parent"))
            .expect("stage proto directory");
        std::fs::write(path, bytes).expect("stage proto");
    }
    let mut compiler = protox::Compiler::new([&staged]).expect("proto includes");
    compiler.include_imports(true);
    compiler
        .open_file("bones/v1/fields.proto")
        .expect("compile fields contract");
    compiler
        .open_file("bones/v1/errors.proto")
        .expect("compile violations contract");
    let descriptor = out.join("fields.fds");
    std::fs::write(&descriptor, compiler.encode_file_descriptor_set()).expect("write descriptor");
    connectrpc_build::Config::new()
        .files(&[
            "bones/v1/fields.proto",
            "bones/v1/annotations.proto",
            "bones/v1/errors.proto",
        ])
        .descriptor_set(&descriptor)
        .generate_json(false)
        .emit_register_fn(false)
        .include_file("_fields.rs")
        .compile()
        .expect("generate canonical field messages");
    println!("cargo:rerun-if-env-changed=BREFWIZ_CODEGEN_INPUTS_SHA");
}
