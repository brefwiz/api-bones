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
    source(&format!("string id = 1; {key_field}"), "", "", options)
}

/// A contract whose `Do` takes `Req` holding exactly `fields` and carries
/// `options`, and, when `wrap_fields` is not empty, whose keyed `Wrapped` takes
/// `Wrap` holding `wrap_fields` and carries `wrap_options`. `Mid` reaches `Req`
/// through a cycle and `Loop` reaches `Wrap` through one.
fn source(fields: &str, wrap_fields: &str, wrap_options: &str, options: &str) -> String {
    let wrapped = if wrap_fields.is_empty() {
        String::new()
    } else {
        format!("rpc Wrapped(Wrap) returns (Res) {{ {wrap_options} }}")
    };
    format!(
        "syntax = \"proto3\";\npackage orders.v1;\nimport \"bones/v1/annotations.proto\";\n\
         message Res {{}}\nmessage Req {{ {fields} }}\n\
         message Mid {{ Req inner = 1; Mid next = 2; }}\n\
         message Loop {{ Wrap back = 1; }}\n\
         message Wrap {{ {wrap_fields} string idempotency_key = 9; }}\n\
         service Orders {{ rpc Do(Req) returns (Res) {{ {options} }} {wrapped} }}\n"
    )
}

fn only_refusal(source: &str) -> IdempotencyKeyRefusal {
    refusal_of(source, "orders.v1.Orders/Do")
}

fn refusal_of(source: &str, method: &str) -> IdempotencyKeyRefusal {
    match check_idempotency_key(&descriptor_set(source)) {
        Err(IdempotencyKeyError::Refused(list)) => {
            assert_eq!(list.len(), 1, "{source}");
            assert_eq!(list[0].method, method);
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

const INVALID: &str = "option (bones.v1.errors) = \"INVALID_ARGUMENT\";";
const KEYED_REQ: &str = "string id = 1; string idempotency_key = 2;";
const GOOD: &str = "option (bones.v1.idempotency_key) = true; option idempotency_level = IDEMPOTENT; \
                    option (bones.v1.errors) = \"ALREADY_EXISTS\";";

#[test]
fn a_keyed_request_embedded_in_a_keyed_request_needs_invalid_argument() {
    for wrap_fields in [
        "Req item = 1;",
        "repeated Req items = 1;",
        "map<string, Req> by_name = 1;",
        "Mid mid = 1;",
        "repeated Mid mids = 1;",
        "oneof pick { Req item = 1; }",
        "Wrap next = 3;",
        "Loop loop = 3;",
    ] {
        let refused = source(KEYED_REQ, wrap_fields, GOOD, GOOD);
        let refusal = refusal_of(&refused, "orders.v1.Orders/Wrapped");
        assert_eq!(refusal.reason, IdempotencyKeyReason::EmbeddedKeyUndeclared);
        assert_eq!(
            refusal.to_string(),
            "orders.v1.Orders/Wrapped: is keyed and embeds the request of a keyed method but does not declare INVALID_ARGUMENT for a non-empty embedded key"
        );
        let declared = source(KEYED_REQ, wrap_fields, &format!("{GOOD} {INVALID}"), GOOD);
        assert_eq!(
            check_idempotency_key(&descriptor_set(&declared)),
            Ok(()),
            "{declared}"
        );
    }
}

#[test]
fn a_keyed_request_embedding_no_keyed_request_needs_no_invalid_argument() {
    let plain = source(KEYED_REQ, "string name = 1;", "", GOOD);
    assert_eq!(check_idempotency_key(&descriptor_set(&plain)), Ok(()));
    let unmarked_inner = source(KEYED_REQ, "Mid mid = 1;", "", "");
    assert_eq!(
        check_idempotency_key(&descriptor_set(&unmarked_inner)),
        Ok(())
    );
}

#[test]
fn only_the_top_level_key_field_makes_a_request_keyed() {
    let wrapped = source(
        KEYED_REQ,
        "Req item = 1;",
        &format!("{GOOD} {INVALID}"),
        GOOD,
    );
    assert_eq!(check_idempotency_key(&descriptor_set(&wrapped)), Ok(()));
    let missing = wrapped.replace("string idempotency_key = 9;", "");
    assert_eq!(
        refusal_of(&missing, "orders.v1.Orders/Wrapped").reason,
        IdempotencyKeyReason::KeyFieldMissing
    );
}

#[test]
fn a_request_holding_only_the_key_declares_no_already_exists() {
    let only_key = "string idempotency_key = 1;";
    let without =
        "option (bones.v1.idempotency_key) = true; option idempotency_level = IDEMPOTENT;";
    assert_eq!(
        check_idempotency_key(&descriptor_set(&source(only_key, "", "", without))),
        Ok(())
    );
    let refusal = only_refusal(&source(only_key, "", "", GOOD));
    assert_eq!(refusal.reason, IdempotencyKeyReason::ConflictImpossible);
    assert_eq!(
        refusal.to_string(),
        "orders.v1.Orders/Do: is keyed with a request holding nothing but the key but declares ALREADY_EXISTS, which cannot occur"
    );
}

#[test]
fn a_request_holding_any_other_field_declares_already_exists() {
    for other in [
        "string id = 2;",
        "bytes body = 2;",
        "repeated string ids = 2;",
        "Res res = 2;",
        "int64 count = 2;",
    ] {
        let fields = format!("string idempotency_key = 1; {other}");
        let without =
            "option (bones.v1.idempotency_key) = true; option idempotency_level = IDEMPOTENT;";
        let refusal = only_refusal(&source(&fields, "", "", without));
        assert_eq!(refusal.reason, IdempotencyKeyReason::ConflictUndeclared);
        assert_eq!(
            check_idempotency_key(&descriptor_set(&source(&fields, "", "", GOOD))),
            Ok(())
        );
    }
}

#[test]
fn the_conflict_rule_is_reported_before_the_embedded_key_rule() {
    let without_conflict =
        "option (bones.v1.idempotency_key) = true; option idempotency_level = IDEMPOTENT;";
    let source = source(KEYED_REQ, "Req item = 1;", without_conflict, GOOD);
    assert_eq!(
        refusal_of(&source, "orders.v1.Orders/Wrapped").reason,
        IdempotencyKeyReason::ConflictUndeclared
    );
}
