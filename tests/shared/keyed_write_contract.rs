// SPDX-License-Identifier: MIT
//! Source for a contract whose batch method names its single through
//! `batch_of`, shared by the lanes that exercise keyed writes.

/// `none`, or comma-separated tokens: `KEYED` and `UNKEYED` become
/// `bones.v1.idempotency_key` true and false, an idempotency level name
/// becomes `idempotency_level`, any other token a `bones.v1.errors` variant.
fn options(spec: &str) -> String {
    if spec == "none" {
        return String::new();
    }
    spec.split(',')
        .map(|token| match token {
            "KEYED" => "option (bones.v1.idempotency_key) = true;".to_owned(),
            "UNKEYED" => "option (bones.v1.idempotency_key) = false;".to_owned(),
            "NO_SIDE_EFFECTS" | "IDEMPOTENT" => format!("option idempotency_level = {token};"),
            variant => format!("option (bones.v1.errors) = \"{variant}\";"),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn field(source: &str) -> &str {
    if source == "none" { "" } else { source }
}

/// A contract whose `Do` takes `Req` and whose `DoMany` takes `ManyReq` and
/// names `Do` through `batch_of`. `single` and `batch` are the [`options`]
/// specs of each method, `single_field` and `batch_field` the request field
/// source or `none`.
pub fn keyed_write_contract(
    single: &str,
    single_field: &str,
    batch: &str,
    batch_field: &str,
) -> String {
    format!(
        "syntax = \"proto3\";\npackage contract.v1;\n\
         import \"bones/v1/annotations.proto\";\n\
         message Req {{ string id = 1; {} }}\n\
         message Res {{ string id = 1; }}\n\
         message ManyReq {{ repeated Req items = 1; {} }}\n\
         service Svc {{\n\
           rpc Do(Req) returns (Res) {{ {} }}\n\
           rpc DoMany(ManyReq) returns (Res) {{ option (bones.v1.batch_of) = \"Do\"; {} }}\n\
         }}\n",
        field(single_field),
        field(batch_field),
        options(single),
        options(batch)
    )
}
