// SPDX-License-Identifier: MIT
//! The `idempotency_key` conformance check against fixture contracts built
//! with protox: a keyed single and batch, each refusal reason, unmarked
//! methods, and the error rendering.

use api_bones_automation::{
    IdempotencyKeyError, IdempotencyKeyReason, IdempotencyKeyRefusal, check_idempotency_key,
};

#[path = "support/resolver.rs"]
mod resolver;

#[path = "support/fixture.rs"]
mod fixture;
use fixture::descriptor_set;

const KEYED: &str = "option (bones.v1.idempotency_key) = true;";
const IDEMPOTENT: &str = "option idempotency_level = IDEMPOTENT;";
const NO_EFFECTS: &str = "option idempotency_level = NO_SIDE_EFFECTS;";
const EXISTS: &str = "option (bones.v1.errors) = \"ALREADY_EXISTS\";";
const KEY_FIELD: &str = "string idempotency_key = 2;";

/// A contract whose `Do` has a request holding `key_field` and carries
/// `options`.
fn contract(key_field: &str, options: &str) -> String {
    format!(
        "syntax = \"proto3\";\npackage orders.v1;\nimport \"bones/v1/annotations.proto\";\n\
         message Res {{}}\nmessage Req {{ string id = 1; {key_field} }}\n\
         service Orders {{ rpc Do(Req) returns (Res) {{ {options} }} }}\n"
    )
}

fn only_refusal(source: &str) -> IdempotencyKeyRefusal {
    match check_idempotency_key(&descriptor_set(source)) {
        Err(IdempotencyKeyError::Refused(list)) => {
            assert_eq!(list.len(), 1, "{source}");
            assert_eq!(list[0].method, "orders.v1.Orders/Do");
            list[0].clone()
        }
        other => panic!("expected a refusal, got {other:?}\n{source}"),
    }
}

#[test]
fn a_keyed_write_meeting_every_rule_passes() {
    let source = contract(KEY_FIELD, &format!("{KEYED} {IDEMPOTENT} {EXISTS}"));
    assert_eq!(check_idempotency_key(&descriptor_set(&source)), Ok(()));
}

#[test]
fn a_keyed_write_without_a_top_level_singular_string_key_is_refused() {
    for key_field in [
        "",
        "string key = 2;",
        "optional string idempotency_key = 2;",
        "repeated string idempotency_key = 2;",
        "bytes idempotency_key = 2;",
        "int64 idempotency_key = 2;",
        "oneof keying { string idempotency_key = 2; }",
        "message Meta { string idempotency_key = 1; } Meta meta = 2;",
    ] {
        let source = contract(key_field, &format!("{KEYED} {IDEMPOTENT} {EXISTS}"));
        let refusal = only_refusal(&source);
        assert_eq!(
            refusal.reason,
            IdempotencyKeyReason::KeyFieldMissing,
            "{source}"
        );
        assert_eq!(
            refusal.to_string(),
            "orders.v1.Orders/Do: is keyed but its request has no top-level string idempotency_key"
        );
    }
}

#[test]
fn a_keyed_write_not_declared_idempotent_is_refused() {
    let source = contract(KEY_FIELD, &format!("{KEYED} {EXISTS}"));
    let refusal = only_refusal(&source);
    assert_eq!(
        refusal.reason,
        IdempotencyKeyReason::NotIdempotent { level: 0 }
    );
    assert_eq!(
        refusal.to_string(),
        "orders.v1.Orders/Do: is keyed but declares IDEMPOTENCY_UNKNOWN, not IDEMPOTENT"
    );
}

#[test]
fn a_keyed_read_is_refused() {
    let source = contract(KEY_FIELD, &format!("{KEYED} {NO_EFFECTS} {EXISTS}"));
    let refusal = only_refusal(&source);
    assert_eq!(refusal.reason, IdempotencyKeyReason::KeyedRead);
    assert_eq!(
        refusal.to_string(),
        "orders.v1.Orders/Do: is keyed but declares NO_SIDE_EFFECTS"
    );
}

#[test]
fn a_keyed_write_not_declaring_already_exists_is_refused() {
    for errors in ["", "option (bones.v1.errors) = \"INVALID_ARGUMENT\";"] {
        let source = contract(KEY_FIELD, &format!("{KEYED} {IDEMPOTENT} {errors}"));
        let refusal = only_refusal(&source);
        assert_eq!(refusal.reason, IdempotencyKeyReason::ConflictUndeclared);
        assert_eq!(
            refusal.to_string(),
            "orders.v1.Orders/Do: is keyed but does not declare ALREADY_EXISTS for a reused key"
        );
    }
}

#[test]
fn the_key_field_is_reported_before_the_read_the_level_and_the_code() {
    assert_eq!(
        only_refusal(&contract("", &format!("{KEYED} {NO_EFFECTS}"))).reason,
        IdempotencyKeyReason::KeyFieldMissing
    );
    assert_eq!(
        only_refusal(&contract(KEY_FIELD, &format!("{KEYED} {NO_EFFECTS}"))).reason,
        IdempotencyKeyReason::KeyedRead
    );
    assert_eq!(
        only_refusal(&contract(KEY_FIELD, KEYED)).reason,
        IdempotencyKeyReason::NotIdempotent { level: 0 }
    );
    assert_eq!(
        only_refusal(&contract(KEY_FIELD, &format!("{KEYED} {IDEMPOTENT}"))).reason,
        IdempotencyKeyReason::ConflictUndeclared
    );
}

#[test]
fn an_unmarked_method_is_not_checked_whatever_its_request_holds() {
    for options in ["", "option (bones.v1.idempotency_key) = false;", NO_EFFECTS] {
        for key_field in ["", KEY_FIELD, "optional string idempotency_key = 2;"] {
            let source = contract(key_field, options);
            assert_eq!(
                check_idempotency_key(&descriptor_set(&source)),
                Ok(()),
                "{source}"
            );
        }
    }
}

#[test]
fn a_set_without_the_vocabulary_passes_and_garbage_is_a_descriptors_error() {
    let source = "syntax = \"proto3\";\npackage orders.v1;\nmessage Req {}\nmessage Res {}\n\
                  service Orders { rpc Do(Req) returns (Res); }\n";
    assert_eq!(check_idempotency_key(&descriptor_set(source)), Ok(()));
    let err = check_idempotency_key(&[0xff, 0xff, 0xff]).unwrap_err();
    assert!(
        matches!(err, IdempotencyKeyError::Descriptors(_)),
        "{err:?}"
    );
    assert!(err.to_string().starts_with("descriptor set is unusable: "));
}
