// SPDX-License-Identifier: MIT
//! Generates the Layer 1 types of the `Numbers` and `Widgets` contract
//! services from the contract protos this crate owns under `proto/`, which the
//! Rust and TypeScript lanes read from here, so there is one proto source and
//! no copy. The shipped `bones/v1` protos come from `api-bones-protos`.

use std::path::{Path, PathBuf};

const PAGES: &str = "contract/v1/pages.proto";
const WIDGETS: &str = "contract/v1/widgets.proto";
const EVENTS: &str = "contract/v1/events.proto";
const SDK_EVENTS: &str = "contract/sdk/v1/events.proto";
const SDK_NUMBERS: &str = "contract/sdk/v1/numbers.proto";
const SDK_WIDGETS: &str = "contract/sdk/v1/widgets.proto";
const PAGINATION: &str = "bones/v1/pagination.proto";
const BATCH: &str = "bones/v1/batch.proto";
const ERRORS: &str = "bones/v1/errors.proto";
const ANNOTATIONS: &str = "bones/v1/annotations.proto";

fn main() {
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR is set by cargo"));
    let contract = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("proto");
    let bones = out_dir.join("bones-protos");
    write_bones_protos(&bones);

    let mut compiler = protox::Compiler::new([&contract, &bones]).expect("proto include paths");
    compiler.include_imports(true);
    compiler
        .open_files([PAGES, WIDGETS, EVENTS, SDK_EVENTS, SDK_NUMBERS, SDK_WIDGETS])
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
            SDK_EVENTS,
            SDK_NUMBERS,
            SDK_WIDGETS,
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

    println!("cargo:rerun-if-changed=proto");
    println!("cargo:rerun-if-env-changed=BREFWIZ_CODEGEN_INPUTS_SHA");
}

/// Writes the shipped `bones/v1` protos under `root`, at the import paths the
/// contract protos use.
fn write_bones_protos(root: &Path) {
    for (relative, bytes) in api_bones_protos::files() {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().expect("a proto path has a directory"))
            .expect("create the bones proto directory");
        std::fs::write(path, bytes).expect("write a bones proto");
    }
}
