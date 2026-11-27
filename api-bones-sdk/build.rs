// SPDX-License-Identifier: MIT
//! Generates the messages of the in-process boundary, `bones.sdk.v1`, and the
//! `bones.v1` messages they carry, from the protos `api-bones-protos` ships.
//! The generated code carries no JSON form: nothing crosses the boundary as
//! text.

use std::path::{Path, PathBuf};

const BOUNDARY: &str = "bones/sdk/v1/boundary.proto";
const PAGINATION: &str = "bones/v1/pagination.proto";
const ERRORS: &str = "bones/v1/errors.proto";
const ANNOTATIONS: &str = "bones/v1/annotations.proto";

fn main() {
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR is set by cargo"));
    let bones = out_dir.join("bones-protos");
    write_bones_protos(&bones);

    let mut compiler = protox::Compiler::new([&bones]).expect("proto include paths");
    compiler.include_imports(true);
    compiler
        .open_files([BOUNDARY])
        .expect("compile the boundary proto");
    let descriptor_set = out_dir.join("boundary.fds");
    std::fs::write(&descriptor_set, compiler.encode_file_descriptor_set())
        .expect("write descriptor set");

    connectrpc_build::Config::new()
        .files(&[BOUNDARY, PAGINATION, ERRORS, ANNOTATIONS])
        .descriptor_set(&descriptor_set)
        .generate_json(false)
        .emit_register_fn(false)
        .include_file("_connectrpc.rs")
        .compile()
        .expect("generate the boundary messages");

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=BREFWIZ_CODEGEN_INPUTS_SHA");
}

/// Writes the shipped protos under `root`, at the import paths the boundary
/// proto uses.
fn write_bones_protos(root: &Path) {
    for (relative, bytes) in api_bones_protos::files() {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().expect("a proto path has a directory"))
            .expect("create the proto directory");
        std::fs::write(path, bytes).expect("write a proto");
    }
}
