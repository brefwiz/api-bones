// SPDX-License-Identifier: MIT
//! The `batch_of` conformance check against fixture contracts built with
//! protox: the accepted spellings, each refusal reason, ordering, and the
//! error rendering.

use api_bones_automation::{BatchOfError, BatchOfReason, BatchOfRefusal, check_batch_of};
use protox::Compiler;

#[path = "support/resolver.rs"]
mod resolver;
use resolver::{PATH, Resolver};

fn descriptor_set(source: &str) -> Vec<u8> {
    let mut compiler = Compiler::with_file_resolver(Resolver(source.to_owned()));
    compiler.include_imports(true);
    compiler
        .open_file(PATH)
        .unwrap_or_else(|e| panic!("fixture does not compile: {e}\n{source}"));
    compiler.encode_file_descriptor_set()
}

/// A contract whose `Orders` service declares each `(method, batch_of)` pair.
fn contract(service: &str, methods: &[(&str, &str)]) -> String {
    let rpcs: String = methods
        .iter()
        .map(|(name, batch_of)| {
            if batch_of.is_empty() {
                format!("  rpc {name}(Req) returns (Res);\n")
            } else {
                format!(
                    "  rpc {name}(Req) returns (Res) {{ option (bones.v1.batch_of) = \"{batch_of}\"; }}\n"
                )
            }
        })
        .collect();
    format!(
        "syntax = \"proto3\";\npackage orders.v1;\nimport \"bones/v1/annotations.proto\";\n\
         message Req {{}}\nmessage Res {{}}\nservice {service} {{\n{rpcs}}}\n"
    )
}

fn refused(set: &[u8]) -> Vec<BatchOfRefusal> {
    match check_batch_of(set) {
        Err(BatchOfError::Refused(list)) => list,
        other => panic!("expected refusals, got {other:?}"),
    }
}

#[test]
fn bare_and_qualified_names_of_a_sibling_method_pass() {
    let source = contract(
        "Orders",
        &[
            ("One", ""),
            ("Many", "One"),
            ("Bulk", "orders.v1.Orders/One"),
        ],
    );
    assert_eq!(check_batch_of(&descriptor_set(&source)), Ok(()));
}

#[test]
fn an_empty_batch_of_is_skipped() {
    let source = contract("Orders", &[("Many", "")]);
    let with_empty = source.replace(
        "rpc Many(Req) returns (Res);",
        "rpc Many(Req) returns (Res) { option (bones.v1.batch_of) = \"\"; }",
    );
    assert_ne!(source, with_empty);
    assert_eq!(check_batch_of(&descriptor_set(&with_empty)), Ok(()));
}

#[test]
fn a_set_without_the_vocabulary_passes() {
    let source = "syntax = \"proto3\";\npackage orders.v1;\nmessage Req {}\nmessage Res {}\n\
                  service Orders { rpc One(Req) returns (Res); }\n";
    assert_eq!(check_batch_of(&descriptor_set(source)), Ok(()));
}

#[test]
fn an_undecodable_set_is_a_descriptors_error() {
    let err = check_batch_of(&[0xff, 0xff, 0xff]).unwrap_err();
    assert!(matches!(err, BatchOfError::Descriptors(_)), "{err:?}");
    assert!(err.to_string().starts_with("descriptor set is unusable: "));
}

#[test]
fn each_refusal_reason_is_reported_with_its_site() {
    let source = contract(
        "Orders",
        &[
            ("One", ""),
            ("Other", "billing.v1.Billing/One"),
            ("Missing", "Nope"),
            ("Loop", "Loop"),
        ],
    );
    let found = refused(&descriptor_set(&source));
    let reasons: Vec<_> = found
        .iter()
        .map(|r| (r.method.as_str(), r.reason))
        .collect();
    assert_eq!(
        reasons,
        vec![
            ("orders.v1.Orders/Other", BatchOfReason::OtherService),
            ("orders.v1.Orders/Missing", BatchOfReason::UnknownMethod),
            ("orders.v1.Orders/Loop", BatchOfReason::SelfReference),
        ]
    );
}

#[test]
fn refusals_come_back_in_service_then_method_order() {
    let source = "syntax = \"proto3\";\npackage orders.v1;\n\
        import \"bones/v1/annotations.proto\";\nmessage Req {}\nmessage Res {}\n\
        service Alpha {\n\
          rpc A1(Req) returns (Res) { option (bones.v1.batch_of) = \"Gone\"; }\n\
          rpc A2(Req) returns (Res) { option (bones.v1.batch_of) = \"A2\"; }\n\
        }\n\
        service Beta {\n\
          rpc B1(Req) returns (Res) { option (bones.v1.batch_of) = \"Gone\"; }\n\
        }\n";
    let found = refused(&descriptor_set(source));
    let sites: Vec<_> = found.iter().map(|r| r.method.as_str()).collect();
    assert_eq!(
        sites,
        vec![
            "orders.v1.Alpha/A1",
            "orders.v1.Alpha/A2",
            "orders.v1.Beta/B1"
        ]
    );
}

#[test]
fn refusals_render_through_display() {
    let source = contract(
        "Orders",
        &[
            ("Other", "billing.v1.Billing/One"),
            ("Missing", "Nope"),
            ("Loop", "Loop"),
        ],
    );
    let err = check_batch_of(&descriptor_set(&source)).unwrap_err();
    assert_eq!(
        err.to_string(),
        "orders.v1.Orders/Other: batch_of \"billing.v1.Billing/One\" names a method of another service; \
         orders.v1.Orders/Missing: batch_of \"Nope\" names a method the service does not declare; \
         orders.v1.Orders/Loop: batch_of \"Loop\" names the RPC itself"
    );
}

/// A contract whose `Many` batches `One`, each with the given `authz` option text.
fn authz_contract(single: &str, batch: &str) -> String {
    format!(
        "syntax = \"proto3\";\npackage orders.v1;\nimport \"bones/v1/annotations.proto\";\n\
         message Req {{}}\nmessage Res {{}}\nservice Orders {{\n\
           rpc One(Req) returns (Res) {{ {single} }}\n\
           rpc Many(Req) returns (Res) {{ option (bones.v1.batch_of) = \"One\"; {batch} }}\n\
         }}\n"
    )
}

const READ: &str =
    "option (bones.v1.authz) = { kind: AUTHZ_KIND_AUTHENTICATED, requires: \"orders:read\" };";
const WRITE: &str =
    "option (bones.v1.authz) = { kind: AUTHZ_KIND_AUTHENTICATED, requires: \"orders:write\" };";
const PUBLIC: &str = "option (bones.v1.authz) = { kind: AUTHZ_KIND_PUBLIC, reason: \"open\" };";
const READ_WRITE: &str = "option (bones.v1.authz) = { kind: AUTHZ_KIND_AUTHENTICATED, requires: \"orders:read\", requires: \"orders:write\" };";
const WRITE_READ: &str = "option (bones.v1.authz) = { kind: AUTHZ_KIND_AUTHENTICATED, requires: \"orders:write\", requires: \"orders:read\" };";

#[test]
fn equal_authorization_passes_whatever_the_requires_order() {
    for (single, batch) in [
        (READ, READ),
        (PUBLIC, PUBLIC),
        ("", ""),
        (READ_WRITE, WRITE_READ),
    ] {
        let source = authz_contract(single, batch);
        assert_eq!(check_batch_of(&descriptor_set(&source)), Ok(()), "{source}");
    }
}

#[test]
fn differing_authorization_is_refused() {
    for (single, batch) in [
        (READ, PUBLIC),
        (PUBLIC, READ),
        (READ, WRITE),
        (READ, READ_WRITE),
        (READ, ""),
        ("", READ),
        ("", PUBLIC),
    ] {
        let source = authz_contract(single, batch);
        let found = refused(&descriptor_set(&source));
        assert_eq!(found.len(), 1, "{source}");
        assert_eq!(found[0].reason, BatchOfReason::AuthzDiffers, "{source}");
        assert_eq!(found[0].method, "orders.v1.Orders/Many");
    }
}

#[test]
fn an_authorization_refusal_renders_through_display() {
    let source = authz_contract(READ, PUBLIC);
    let err = check_batch_of(&descriptor_set(&source)).unwrap_err();
    assert_eq!(
        err.to_string(),
        "orders.v1.Orders/Many: batch_of \"One\" does not keep the authorization of the method it names"
    );
}
