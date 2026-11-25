// SPDX-License-Identifier: MIT
//! Generates the Rust service of `proto/bones/test/v1/fetch.proto`. The proto
//! carries well-known types only, so the generated code is the service trait
//! and its router registration; the messages come from `buffa-types`.

use std::path::PathBuf;

const FETCH: &str = "bones/test/v1/fetch.proto";

fn main() {
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR is set by cargo"));
    let own = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("proto");

    let mut compiler = protox::Compiler::new([&own]).expect("proto include path");
    compiler.include_imports(true);
    compiler
        .open_files([FETCH])
        .expect("compile the fetch test proto");
    let descriptor_set = out_dir.join("fetch.fds");
    std::fs::write(&descriptor_set, compiler.encode_file_descriptor_set())
        .expect("write descriptor set");

    connectrpc_build::Config::new()
        .files(&[FETCH])
        .descriptor_set(&descriptor_set)
        .generate_json(true)
        .emit_register_fn(false)
        .include_file("_connectrpc.rs")
        .compile()
        .expect("generate the fetch test service");

    println!("cargo:rerun-if-changed=proto");
    println!("cargo:rerun-if-env-changed=BREFWIZ_CODEGEN_INPUTS_SHA");
}
