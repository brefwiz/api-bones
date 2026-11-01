// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! Generates the Rust type of a stream response message from its proto, the way
//! a consumer's build does, so `stream_controlled!` is invoked over a real
//! generated type.

use std::path::PathBuf;

use protox::prost::Message;

const WATCH: &str = "contract/v1/watch.proto";
const STREAM: &str = "bones/v1/stream.proto";

fn main() {
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR is set by cargo"));
    let own = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("proto");
    let bones = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../api-bones-protos/proto");

    let mut compiler = protox::Compiler::new([&own, &bones]).expect("proto include paths");
    compiler.include_imports(true);
    compiler.open_files([WATCH]).expect("compile watch.proto");
    let descriptor_set = out_dir.join("watch.fds");
    std::fs::write(
        &descriptor_set,
        compiler.file_descriptor_set().encode_to_vec(),
    )
    .expect("write descriptor set");

    connectrpc_build::Config::new()
        .files(&[WATCH, STREAM])
        .descriptor_set(&descriptor_set)
        .generate_json(false)
        .emit_register_fn(false)
        .include_file("_connectrpc.rs")
        .compile()
        .expect("generate watch.proto");

    println!("cargo:rerun-if-changed=proto");
    println!("cargo:rerun-if-changed=../../api-bones-protos/proto/bones/v1/stream.proto");
    println!("cargo:rerun-if-env-changed=BREFWIZ_CODEGEN_INPUTS_SHA");
}
