// SPDX-License-Identifier: MIT
//! The `batch_of` conformance check against fixture contracts built with
//! protox: the accepted spellings, each refusal reason, ordering, and the
//! error rendering.

use api_bones_automation::{
    BatchOfError, BatchOfReason, BatchOfRefusal, check_batch_of, check_batch_of_against_pool,
};
use protox::Compiler;
use protox::prost::Message;
use protox::prost_reflect::DescriptorPool;

#[path = "support/resolver.rs"]
mod resolver;
use resolver::{PATH, Resolver};

#[path = "support/fixture.rs"]
mod fixture;
use fixture::descriptor_set;

#[test]
fn expected_batch_declarations_cannot_disappear_or_change_in_serialized_bytes() {
    let source = contract("Orders", &[("One", ""), ("Many", "One")]);
    let mut compiler = Compiler::with_file_resolver(Resolver(source));
    compiler.include_imports(true);
    compiler.open_file(PATH).expect("contract compiles");
    let pool = compiler.descriptor_pool();
    assert_eq!(
        check_batch_of_against_pool(&compiler.encode_file_descriptor_set(), &pool),
        Ok(())
    );
    let stripped = compiler.file_descriptor_set().encode_to_vec();
    let stripped_pool = DescriptorPool::decode(stripped.as_slice()).expect("standard descriptor");
    let service = stripped_pool
        .get_service_by_name("orders.v1.Orders")
        .expect("Orders service");
    let batch = service
        .methods()
        .find(|method| method.name() == "Many")
        .expect("Many method");
    assert_eq!(
        batch
            .options()
            .get_field_by_name("idempotency_level")
            .and_then(|value| value.as_enum_number()),
        Some(1)
    );
    let extension = stripped_pool
        .get_extension_by_name("bones.v1.batch_of")
        .expect("extension schema");
    assert!(!batch.options().has_extension(&extension));
    let no_declarations = descriptor_set(&contract("Orders", &[("One", ""), ("Many", "")]));
    let substituted = descriptor_set(&contract("Orders", &[("One", "Many"), ("Many", "")]));
    for bytes in [stripped, Vec::new(), no_declarations, substituted] {
        let error =
            check_batch_of_against_pool(&bytes, &pool).expect_err("declaration mismatch refused");
        assert_eq!(
            error.to_string(),
            "descriptor set is unusable: orders.v1.Orders/Many: batch_of declaration mismatch: expected Some(\"One\"), serialized None"
        );
    }
    let changed = descriptor_set(&contract("Orders", &[("One", ""), ("Many", "Other")]));
    assert_eq!(
        check_batch_of_against_pool(&changed, &pool)
            .expect_err("changed target refused")
            .to_string(),
        "descriptor set is unusable: orders.v1.Orders/Many: batch_of declaration mismatch: expected Some(\"One\"), serialized Some(\"Other\")"
    );
    let valid_pool = DescriptorPool::decode(compiler.encode_file_descriptor_set().as_slice())
        .expect("preserved descriptor pool");
    assert_eq!(
        check_batch_of_against_pool(&valid_pool.encode_to_vec(), &pool),
        Ok(())
    );
}

/// A contract whose `Orders` service declares each `(method, batch_of)` pair.
fn contract(service: &str, methods: &[(&str, &str)]) -> String {
    let rpcs: String = methods
        .iter()
        .map(|(name, batch_of)| {
            if batch_of.is_empty() {
                format!(
                    "  rpc {name}(Req) returns (Res) {{ option idempotency_level = NO_SIDE_EFFECTS; }}\n"
                )
            } else {
                format!(
                    "  rpc {name}(Req) returns (Res) {{ option (bones.v1.batch_of) = \"{batch_of}\"; option idempotency_level = NO_SIDE_EFFECTS; }}\n"
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
        "rpc Many(Req) returns (Res) { option idempotency_level = NO_SIDE_EFFECTS; }",
        "rpc Many(Req) returns (Res) { option (bones.v1.batch_of) = \"\"; option idempotency_level = NO_SIDE_EFFECTS; }",
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
           rpc One(Req) returns (Res) {{ option idempotency_level = NO_SIDE_EFFECTS; {single} }}\n\
           rpc Many(Req) returns (Res) {{ option (bones.v1.batch_of) = \"One\"; option idempotency_level = NO_SIDE_EFFECTS; {batch} }}\n\
         }}\n"
    )
}

/// A contract whose `Many` batches `One`, each carrying the given method option text.
fn level_contract(single: &str, batch: &str) -> String {
    format!(
        "syntax = \"proto3\";\npackage orders.v1;\nimport \"bones/v1/annotations.proto\";\n\
         message Req {{}}\nmessage Res {{}}\nservice Orders {{\n\
           rpc One(Req) returns (Res) {{ {single} }}\n\
           rpc Many(Req) returns (Res) {{ option (bones.v1.batch_of) = \"One\"; {batch} }}\n\
         }}\n"
    )
}

const NO_EFFECTS: &str = "option idempotency_level = NO_SIDE_EFFECTS;";
const IDEMPOTENT: &str = "option idempotency_level = IDEMPOTENT;";

#[test]
fn a_batch_keeping_its_singles_idempotency_level_passes() {
    for (single, batch) in [(NO_EFFECTS, NO_EFFECTS), (IDEMPOTENT, IDEMPOTENT), ("", "")] {
        let source = level_contract(single, batch);
        assert_eq!(check_batch_of(&descriptor_set(&source)), Ok(()), "{source}");
    }
}

#[test]
fn a_batch_with_another_level_than_its_single_is_refused() {
    for (single, batch, single_name, batch_name) in [
        (NO_EFFECTS, "", "NO_SIDE_EFFECTS", "IDEMPOTENCY_UNKNOWN"),
        (NO_EFFECTS, IDEMPOTENT, "NO_SIDE_EFFECTS", "IDEMPOTENT"),
        (IDEMPOTENT, NO_EFFECTS, "IDEMPOTENT", "NO_SIDE_EFFECTS"),
        ("", IDEMPOTENT, "IDEMPOTENCY_UNKNOWN", "IDEMPOTENT"),
    ] {
        let refusals = refused(&descriptor_set(&level_contract(single, batch)));
        assert_eq!(refusals.len(), 1);
        assert!(matches!(
            refusals[0].reason,
            BatchOfReason::IdempotencyDiffers { .. }
        ));
        assert_eq!(refusals[0].method, "orders.v1.Orders/Many");
        assert_eq!(
            refusals[0].to_string(),
            format!(
                "orders.v1.Orders/Many: batch_of \"One\" does not keep the idempotency level of the method it names (single {single_name}, batch {batch_name})"
            )
        );
    }
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

const EXISTS: &str = "option (bones.v1.errors) = \"ALREADY_EXISTS\";";
const OTHER_ERROR: &str = "option (bones.v1.errors) = \"NOT_FOUND\";";

/// One `bones.v1.errors` option line per code, in order.
fn errors(codes: &[&str]) -> String {
    let lines: Vec<String> = codes
        .iter()
        .map(|code| format!("option (bones.v1.errors) = \"{code}\";"))
        .collect();
    lines.join(" ")
}

#[test]
fn already_exists_declared_by_the_batch_and_its_single_passes() {
    let source = level_contract(EXISTS, EXISTS);
    assert_eq!(check_batch_of(&descriptor_set(&source)), Ok(()), "{source}");
}

#[test]
fn already_exists_declared_by_neither_passes() {
    for (single, batch) in [("", ""), (OTHER_ERROR, OTHER_ERROR), (OTHER_ERROR, "")] {
        let source = level_contract(single, batch);
        assert_eq!(check_batch_of(&descriptor_set(&source)), Ok(()), "{source}");
    }
}

#[test]
fn already_exists_declared_by_the_single_only_passes() {
    let source = level_contract(EXISTS, "");
    assert_eq!(check_batch_of(&descriptor_set(&source)), Ok(()), "{source}");
}

#[test]
fn already_exists_declared_by_the_batch_only_is_refused() {
    for single in ["", OTHER_ERROR] {
        let source = level_contract(single, EXISTS);
        let found = refused(&descriptor_set(&source));
        assert_eq!(found.len(), 1, "{source}");
        assert_eq!(
            found[0].reason,
            BatchOfReason::AlreadyExistsUndeclared,
            "{source}"
        );
        assert_eq!(found[0].method, "orders.v1.Orders/Many");
    }
}

#[test]
fn already_exists_found_anywhere_in_a_multi_code_errors_list() {
    let source = level_contract(
        &errors(&["NOT_FOUND", "ALREADY_EXISTS", "INVALID_ARGUMENT"]),
        &errors(&["ALREADY_EXISTS"]),
    );
    assert_eq!(check_batch_of(&descriptor_set(&source)), Ok(()), "{source}");
}

#[test]
fn already_exists_absent_from_a_multi_code_list_is_not_declared() {
    let source = level_contract(
        &errors(&["NOT_FOUND", "INVALID_ARGUMENT"]),
        &errors(&["INVALID_ARGUMENT", "ALREADY_EXISTS"]),
    );
    let found = refused(&descriptor_set(&source));
    assert_eq!(found.len(), 1, "{source}");
    assert_eq!(
        found[0].reason,
        BatchOfReason::AlreadyExistsUndeclared,
        "{source}"
    );
}

#[test]
fn an_already_exists_refusal_renders_through_display() {
    let source = level_contract(OTHER_ERROR, EXISTS);
    let err = check_batch_of(&descriptor_set(&source)).unwrap_err();
    assert_eq!(
        err.to_string(),
        "orders.v1.Orders/Many: batch_of \"One\" declares ALREADY_EXISTS but the method it names does not"
    );
}
