// SPDX-License-Identifier: MIT
//! Compiles a two-emitter error contract against the shipped `bones/v1` files
//! and hands the descriptor set to the catalog build helper, as an emitting
//! crate's build script does after its own codegen.

use api_bones_i18n::emitters::{Emitter, embed_and_check_error_catalogs};

#[path = "../shared/shipped_resolver.rs"]
mod shipped_resolver;

const CONTRACT_SOURCE: &str = "syntax = \"proto3\";\npackage contract.v1;\n\
    import \"bones/v1/annotations.proto\";\n\
    enum OriginError {\n\
      ORIGIN_ERROR_UNSPECIFIED = 0;\n\
      ORIGIN_ERROR_LIMIT = 1 [\n\
        (bones.v1.error_args) = { name: \"limit\" kind: ERROR_ARG_KIND_INT64 }\n\
      ];\n\
    }\n\
    enum EdgeError {\n\
      EDGE_ERROR_UNSPECIFIED = 0;\n\
      EDGE_ERROR_DENIED = 1 [\n\
        (bones.v1.error_args) = { name: \"seat\" kind: ERROR_ARG_KIND_STRING }\n\
      ];\n\
    }\n";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut compiler =
        shipped_resolver::compiler(CONTRACT_SOURCE.to_owned(), api_bones_protos::files());
    compiler.include_imports(true);
    compiler.open_file(shipped_resolver::CONTRACT)?;
    embed_and_check_error_catalogs(
        "catalogs",
        &compiler.encode_file_descriptor_set(),
        &[
            Emitter::new("acme-origin", "contract.v1.OriginError", "ORIGIN_ERROR_"),
            Emitter::new("acme-edge", "contract.v1.EdgeError", "EDGE_ERROR_"),
        ],
    )?;
    println!("cargo:rerun-if-env-changed=BREFWIZ_CODEGEN_INPUTS_SHA");
    println!("cargo:rerun-if-changed=build.rs");
    Ok(())
}
