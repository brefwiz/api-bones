// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! Generates the Rust types of the contract protos the way a consumer's build
//! does: the stream response message `stream_controlled!` is invoked over, and
//! the list service the pagination steps serve and call over the wire, and the
//! widgets service the batch steps serve and call through the batch runtime.

use std::path::PathBuf;

const WATCH: &str = "contract/v1/watch.proto";
const STREAM: &str = "bones/v1/stream.proto";
const PAGES: &str = "contract/v1/pages.proto";
const PAGINATION: &str = "bones/v1/pagination.proto";
const WIDGETS: &str = "contract/v1/widgets.proto";
const BATCH: &str = "bones/v1/batch.proto";
const ERRORS: &str = "bones/v1/errors.proto";
const FIELDS: &str = "bones/v1/fields.proto";
const FIELD_CONTRACT: &str = "contract/v1/fields.proto";

fn main() {
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR is set by cargo"));
    let own = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("proto");
    let bones = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../api-bones-protos/proto");

    let shared = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../shared/proto");
    let mut compiler = protox::Compiler::new([&own, &bones, &shared]).expect("proto include paths");
    compiler.include_imports(true);
    compiler
        .open_files([WATCH, PAGES, WIDGETS, FIELD_CONTRACT])
        .expect("compile the contract protos");
    let descriptor_set = out_dir.join("watch.fds");
    let descriptor_bytes = compiler.encode_file_descriptor_set();
    std::fs::write(&descriptor_set, &descriptor_bytes).expect("write descriptor set");

    let pool = compiler.descriptor_pool();
    let paging = api_bones_automation::emit_rust_paging(
        &pool,
        &["contract.v1.Numbers", "contract.v1.Widgets"],
        &|package| format!("crate::generated::{}", package.replace('.', "::")),
    )
    .expect("emit Rust paging glue");
    std::fs::write(out_dir.join("paged.rs"), paging).expect("write Rust paging glue");
    api_bones_automation::check_batch_of_against_pool(&descriptor_bytes, &pool)
        .expect("valid batch declarations");
    let mut methods = Vec::new();
    for service in pool.services() {
        for method in service.methods() {
            let idempotency = method
                .options()
                .get_field_by_name("idempotency_level")
                .and_then(|value| value.as_enum_number())
                .unwrap_or_default();
            let mut policy = serde_json::json!({
                "rpc": format!("/{}/{}", service.full_name(), method.name()),
                "procedure": if method.is_client_streaming() || method.is_server_streaming() { "streaming" } else { "unary" },
                "idempotency": match idempotency { 1 => "NO_SIDE_EFFECTS", 2 => "IDEMPOTENT", _ => "UNSPECIFIED" },
                "browserCache": { "scope": "NO_STORE", "maxAgeSeconds": 0 },
                "sensitivity": "UNSPECIFIED", "maxEncodedUrlBytes": 4096
            });
            if let Some(paging) = api_bones_automation::paging_policy(&pool, &method) {
                policy["paging"] = serde_json::to_value(paging).expect("serialize paging");
            }
            if let Some(batch) =
                api_bones_automation::batch_policy(&pool, &method).expect("canonical batch shape")
            {
                policy["batch"] = serde_json::to_value(batch).expect("serialize batching");
            }
            methods.push(policy);
        }
    }
    methods.sort_by(|a, b| a["rpc"].as_str().cmp(&b["rpc"].as_str()));
    let policy_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../typescript/connect-method-policy.json");
    let mut policy_bytes =
        serde_json::to_vec_pretty(&serde_json::json!({ "schemaVersion": 3, "methods": methods }))
            .expect("serialize policy");
    policy_bytes.push(b'\n');
    if std::fs::read(&policy_path).ok().as_deref() != Some(policy_bytes.as_slice()) {
        std::fs::write(policy_path, policy_bytes).expect("write generated method policy");
    }

    connectrpc_build::Config::new()
        .files(&[
            WATCH,
            STREAM,
            PAGES,
            PAGINATION,
            WIDGETS,
            BATCH,
            ERRORS,
            FIELDS,
            "bones/v1/annotations.proto",
            FIELD_CONTRACT,
        ])
        .descriptor_set(&descriptor_set)
        .generate_json(true)
        .emit_register_fn(false)
        .include_file("_connectrpc.rs")
        .compile()
        .expect("generate the contract protos");

    println!("cargo:rerun-if-changed=proto");
    println!("cargo:rerun-if-changed=../shared/proto/contract/v1/fields.proto");
    println!("cargo:rerun-if-changed=../../api-bones-protos/proto/bones/v1/stream.proto");
    println!("cargo:rerun-if-changed=../../api-bones-protos/proto/bones/v1/pagination.proto");
    println!("cargo:rerun-if-changed=../../api-bones-protos/proto/bones/v1/batch.proto");
    println!("cargo:rerun-if-changed=../../api-bones-protos/proto/bones/v1/errors.proto");
    println!("cargo:rerun-if-changed=../../api-bones-protos/proto/bones/v1/fields.proto");
    println!("cargo:rerun-if-changed=../../api-bones-protos/proto/bones/v1/annotations.proto");
    println!("cargo:rerun-if-env-changed=BREFWIZ_CODEGEN_INPUTS_SHA");
}
