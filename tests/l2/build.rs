// SPDX-License-Identifier: MIT
//! Generates the Layer 1 types of the `Numbers` and `Widgets` contract
//! services from the protos the Rust lanes already own, so there is one proto
//! source and no copy.

use std::path::PathBuf;

const PAGES: &str = "contract/v1/pages.proto";
const WIDGETS: &str = "contract/v1/widgets.proto";
const EVENTS: &str = "contract/v1/events.proto";
const PAGINATION: &str = "bones/v1/pagination.proto";
const BATCH: &str = "bones/v1/batch.proto";
const ERRORS: &str = "bones/v1/errors.proto";
const ANNOTATIONS: &str = "bones/v1/annotations.proto";

fn main() {
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR is set by cargo"));
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let contract = manifest.join("../rust/proto");
    let bones = manifest.join("../../api-bones-protos/proto");
    let shared = manifest.join("../shared/proto");

    let mut compiler =
        protox::Compiler::new([&contract, &bones, &shared]).expect("proto include paths");
    compiler.include_imports(true);
    compiler
        .open_files([PAGES, WIDGETS, EVENTS])
        .expect("compile the contract protos");
    let descriptor_set = out_dir.join("l2.fds");
    std::fs::write(&descriptor_set, compiler.encode_file_descriptor_set())
        .expect("write descriptor set");

    connectrpc_build::Config::new()
        .files(&[
            PAGES,
            PAGINATION,
            WIDGETS,
            EVENTS,
            BATCH,
            ERRORS,
            ANNOTATIONS,
        ])
        .descriptor_set(&descriptor_set)
        .generate_json(true)
        .emit_register_fn(false)
        .include_file("_connectrpc.rs")
        .compile()
        .expect("generate the contract protos");

    println!("cargo:rerun-if-changed=../rust/proto");
    println!("cargo:rerun-if-changed=../shared/proto/contract/v1/events.proto");
    println!("cargo:rerun-if-changed=../../api-bones-protos/proto/bones/v1");
    println!("cargo:rerun-if-env-changed=BREFWIZ_CODEGEN_INPUTS_SHA");
}
