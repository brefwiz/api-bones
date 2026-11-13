// SPDX-License-Identifier: MIT
//! The `nested_write_of` conformance check against fixture contracts built
//! with protox: both valid shapes and each refusal reason.

use api_bones_automation::{
    NestedWriteOfError, NestedWriteOfReason, NestedWriteOfRefusal, check_nested_write_of,
};

#[path = "support/resolver.rs"]
mod resolver;

#[path = "support/fixture.rs"]
mod fixture;
use fixture::descriptor_set;

/// A contract whose `Create` request carries `field` (a full field
/// declaration) and `rpcs` extra service lines, beside the `Put` child write.
fn contract(field: &str, rpcs: &str) -> String {
    format!(
        "syntax = \"proto3\";\npackage orders.v1;\nimport \"bones/v1/annotations.proto\";\n\
         message Line {{ string key = 1; }}\n\
         message LineSet {{ repeated Line items = 1; }}\n\
         message Pair {{ string key = 1; string label = 2; }}\n\
         message PairSet {{ repeated Pair items = 1; }}\n\
         message CreateReq {{ string name = 1; {field} }}\n\
         message Res {{}}\n\
         service Orders {{\n  rpc Put(Line) returns (Res);\n  rpc Create(CreateReq) returns (Res);\n{rpcs}}}\n\
         service Billing {{ rpc Put(Line) returns (Res); }}\n"
    )
}

fn refused(source: &str) -> Vec<NestedWriteOfRefusal> {
    match check_nested_write_of(&descriptor_set(source)) {
        Err(NestedWriteOfError::Refused(list)) => list,
        other => panic!("expected refusals, got {other:?}"),
    }
}

fn reason_of(field: &str, rpcs: &str) -> NestedWriteOfReason {
    let found = refused(&contract(field, rpcs));
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].field, "orders.v1.CreateReq.lines");
    found[0].reason
}

#[test]
fn the_create_shape_passes() {
    let source = contract(
        "repeated Line lines = 2 [(bones.v1.nested_write_of) = \"Put\"];",
        "",
    );
    assert_eq!(check_nested_write_of(&descriptor_set(&source)), Ok(()));
}

#[test]
fn the_update_shape_and_a_qualified_name_pass() {
    let source = contract(
        "optional LineSet lines = 2 [(bones.v1.nested_write_of) = \"orders.v1.Orders/Put\"];",
        "",
    );
    assert_eq!(check_nested_write_of(&descriptor_set(&source)), Ok(()));
}

#[test]
fn a_set_without_the_vocabulary_passes() {
    let source = "syntax = \"proto3\";\npackage orders.v1;\nmessage Req {}\nmessage Res {}\n\
                  service Orders { rpc One(Req) returns (Res); }\n";
    assert_eq!(check_nested_write_of(&descriptor_set(source)), Ok(()));
}

#[test]
fn an_empty_value_is_skipped() {
    let source = contract(
        "repeated Line lines = 2 [(bones.v1.nested_write_of) = \"\"];",
        "",
    );
    assert_eq!(check_nested_write_of(&descriptor_set(&source)), Ok(()));
}

#[test]
fn an_undecodable_set_is_a_descriptors_error() {
    let err = check_nested_write_of(&[0xff, 0xff, 0xff]).unwrap_err();
    assert!(matches!(err, NestedWriteOfError::Descriptors(_)), "{err:?}");
    assert!(err.to_string().starts_with("descriptor set is unusable: "));
}

#[test]
fn a_message_no_rpc_takes_is_refused() {
    let source = contract("", "")
        .replace(
            "message Res {}",
            "message Orphan { repeated Line lines = 1 [(bones.v1.nested_write_of) = \"Put\"]; }\nmessage Res {}",
        );
    let found = refused(&source);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].field, "orders.v1.Orphan.lines");
    assert_eq!(found[0].reason, NestedWriteOfReason::NoRequest);
}

#[test]
fn a_method_of_another_service_is_refused() {
    let field = "repeated Line lines = 2 [(bones.v1.nested_write_of) = \"orders.v1.Billing/Put\"];";
    assert_eq!(reason_of(field, ""), NestedWriteOfReason::OtherService);
}

#[test]
fn an_undeclared_method_is_refused() {
    let field = "repeated Line lines = 2 [(bones.v1.nested_write_of) = \"Nope\"];";
    assert_eq!(reason_of(field, ""), NestedWriteOfReason::UnknownMethod);
}

#[test]
fn naming_the_carrying_rpc_is_refused() {
    let field = "repeated Line lines = 2 [(bones.v1.nested_write_of) = \"Create\"];";
    assert_eq!(reason_of(field, ""), NestedWriteOfReason::SelfReference);
}

#[test]
fn a_named_method_that_is_not_a_unary_write_is_refused() {
    for (rpc, named) in [
        (
            "  rpc Read(Line) returns (Res) { option idempotency_level = NO_SIDE_EFFECTS; }\n",
            "Read",
        ),
        ("  rpc Upload(stream Line) returns (Res);\n", "Upload"),
        ("  rpc Follow(Line) returns (stream Res);\n", "Follow"),
        ("  rpc Chat(stream Line) returns (stream Res);\n", "Chat"),
        (
            "  rpc Read(Line) returns (Res) { option idempotency_level = NO_SIDE_EFFECTS; }\n",
            "orders.v1.Orders/Read",
        ),
    ] {
        let field = format!("repeated Line lines = 2 [(bones.v1.nested_write_of) = \"{named}\"];");
        assert_eq!(
            reason_of(&field, rpc),
            NestedWriteOfReason::NotUnaryWrite,
            "{named}"
        );
    }
}

#[test]
fn a_non_unary_write_is_reported_before_a_non_children_field() {
    let read = "  rpc Read(Line) returns (Res) { option idempotency_level = NO_SIDE_EFFECTS; }\n";
    for (field, rpc, named) in [
        ("map<string, Line> lines = 2", read, "Read"),
        ("optional Line lines = 2", read, "Read"),
        (
            "map<string, Line> lines = 2",
            "  rpc Chat(stream Line) returns (stream Res);\n",
            "Chat",
        ),
    ] {
        let field = format!("{field} [(bones.v1.nested_write_of) = \"{named}\"];");
        assert_eq!(
            reason_of(&field, rpc),
            NestedWriteOfReason::NotUnaryWrite,
            "{field}"
        );
    }
}

#[test]
fn a_streaming_carrier_naming_itself_is_a_self_reference() {
    let source = contract(
        "repeated Line lines = 2 [(bones.v1.nested_write_of) = \"Create\"];",
        "",
    )
    .replace(
        "rpc Create(CreateReq) returns (Res);",
        "rpc Create(stream CreateReq) returns (stream Res);",
    );
    let found = refused(&source);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].reason, NestedWriteOfReason::SelfReference);
}

#[test]
fn an_idempotent_or_unspecified_unary_write_passes() {
    for rpc in [
        "  rpc Save(Line) returns (Res) { option idempotency_level = IDEMPOTENT; }\n",
        "  rpc Save(Line) returns (Res);\n",
    ] {
        let source = contract(
            "repeated Line lines = 2 [(bones.v1.nested_write_of) = \"Save\"];",
            rpc,
        );
        assert_eq!(
            check_nested_write_of(&descriptor_set(&source)),
            Ok(()),
            "{rpc}"
        );
    }
}

#[test]
fn fields_that_are_not_children_are_refused() {
    let shapes = [
        "map<string, Line> lines = 2 [(bones.v1.nested_write_of) = \"Put\"];",
        "repeated string lines = 2 [(bones.v1.nested_write_of) = \"Put\"];",
        "Line lines = 2 [(bones.v1.nested_write_of) = \"Put\"];",
        "optional Pair lines = 2 [(bones.v1.nested_write_of) = \"Put\"];",
        "optional PairSet lines = 2 [(bones.v1.nested_write_of) = \"Put\"];",
    ];
    for field in shapes {
        // `PairSet` is rewritten to hold `Line` items, so its name no longer
        // matches its child.
        let source = contract(field, "");
        let source = if field.contains("PairSet") {
            source.replace(
                "message PairSet { repeated Pair items = 1; }",
                "message PairSet { repeated Line items = 1; }",
            )
        } else {
            source
        };
        assert_eq!(
            refused(&source)[0].reason,
            NestedWriteOfReason::NotChildren,
            "{field}"
        );
    }
}

#[test]
fn a_set_with_an_extra_or_misnumbered_field_is_refused() {
    for set in [
        "message LineSet { repeated Line items = 1; string note = 2; }",
        "message LineSet { repeated Line items = 2; }",
        "message LineSet { repeated Line rows = 1; }",
        "message LineSet { Line items = 1; }",
    ] {
        let source = contract(
            "optional LineSet lines = 2 [(bones.v1.nested_write_of) = \"Put\"];",
            "",
        )
        .replace("message LineSet { repeated Line items = 1; }", set);
        assert_eq!(
            refused(&source)[0].reason,
            NestedWriteOfReason::NotChildren,
            "{set}"
        );
    }
}

#[test]
fn refusals_render_through_display() {
    let field = "repeated Line lines = 2 [(bones.v1.nested_write_of) = \"Nope\"];";
    let err = check_nested_write_of(&descriptor_set(&contract(field, ""))).unwrap_err();
    assert_eq!(
        err.to_string(),
        "orders.v1.CreateReq.lines: nested_write_of \"Nope\" names a method the service does not declare"
    );
}

#[test]
fn every_refusal_reason_renders_its_own_sentence() {
    let cases = [
        (
            NestedWriteOfReason::NoRequest,
            "is on a message no RPC takes as its input",
        ),
        (
            NestedWriteOfReason::OtherService,
            "names a method of another service",
        ),
        (
            NestedWriteOfReason::UnknownMethod,
            "names a method the service does not declare",
        ),
        (
            NestedWriteOfReason::SelfReference,
            "names the RPC that carries the field",
        ),
        (
            NestedWriteOfReason::NotUnaryWrite,
            "names a method that is not a unary write (it streams or declares NO_SIDE_EFFECTS)",
        ),
        (
            NestedWriteOfReason::NotChildren,
            "is on a field that is not `repeated <Child>` or `optional <Child>Set`",
        ),
        (
            NestedWriteOfReason::ReadRequest,
            "is on the request of a method that declares NO_SIDE_EFFECTS",
        ),
        (
            NestedWriteOfReason::ChildMismatch,
            "is on children of a message the named method does not write",
        ),
    ];
    for (reason, sentence) in cases {
        let refusal = NestedWriteOfRefusal {
            field: "orders.v1.CreateReq.lines".to_owned(),
            nested_write_of: "Put".to_owned(),
            reason,
        };
        assert_eq!(
            refusal.to_string(),
            format!("orders.v1.CreateReq.lines: nested_write_of \"Put\" {sentence}")
        );
    }
}

#[test]
fn several_refusals_render_joined() {
    let err = NestedWriteOfError::Refused(vec![
        NestedWriteOfRefusal {
            field: "a.v1.M.x".to_owned(),
            nested_write_of: "P".to_owned(),
            reason: NestedWriteOfReason::NoRequest,
        },
        NestedWriteOfRefusal {
            field: "a.v1.M.y".to_owned(),
            nested_write_of: "Q".to_owned(),
            reason: NestedWriteOfReason::SelfReference,
        },
    ]);
    assert_eq!(
        err.to_string(),
        "a.v1.M.x: nested_write_of \"P\" is on a message no RPC takes as its input; \
         a.v1.M.y: nested_write_of \"Q\" names the RPC that carries the field"
    );
}

#[test]
fn every_carrier_must_accept_the_name() {
    // Deliberate contract change: a second carrier that refuses the name
    // (`Create2` names itself) now refuses the field, instead of one
    // accepting carrier being enough. `head` makes `Create2` a write of `Line`.
    let field =
        "repeated Line lines = 2 [(bones.v1.nested_write_of) = \"Create2\"]; Line head = 3;";
    let source = contract(field, "  rpc Create2(CreateReq) returns (Res);\n");
    assert_eq!(
        reason_of_source(&source),
        NestedWriteOfReason::SelfReference
    );
}

/// `contract` with `Billing` also taking `CreateReq` through `Bill`, its
/// `Put` as `billing_put`, and the two services in either declaration order.
fn two_carriers(named: &str, billing_put: &str, billing_first: bool) -> String {
    let field = format!("repeated Line lines = 2 [(bones.v1.nested_write_of) = \"{named}\"];");
    let source =
        contract(&field, "").replace("service Billing { rpc Put(Line) returns (Res); }\n", "");
    let orders_at = source.find("service Orders").unwrap();
    let (head, orders) = source.split_at(orders_at);
    let billing = format!(
        "service Billing {{ rpc Put({billing_put}) returns (Res); rpc Bill(CreateReq) returns (Res); }}\n"
    );
    if billing_first {
        format!("{head}{billing}{orders}")
    } else {
        format!("{head}{orders}{billing}")
    }
}

#[test]
fn two_carriers_that_both_resolve_a_matching_single_pass() {
    for billing_first in [false, true] {
        let source = two_carriers("Put", "Line", billing_first);
        assert_eq!(check_nested_write_of(&descriptor_set(&source)), Ok(()));
    }
}

#[test]
fn a_second_carrier_with_a_mismatching_single_is_refused_in_either_order() {
    for billing_first in [false, true] {
        let source = two_carriers("Put", "Pair", billing_first);
        assert_eq!(
            reason_of_source(&source),
            NestedWriteOfReason::ChildMismatch,
            "billing_first={billing_first}"
        );
    }
}

#[test]
fn a_qualified_name_of_another_service_refuses_in_either_order() {
    for named in ["orders.v1.Orders/Put", "orders.v1.Billing/Put"] {
        for billing_first in [false, true] {
            let source = two_carriers(named, "Line", billing_first);
            assert_eq!(
                reason_of_source(&source),
                NestedWriteOfReason::OtherService,
                "{named} billing_first={billing_first}"
            );
        }
    }
}

#[test]
fn a_single_whose_request_has_two_child_slots_is_refused() {
    let field = "repeated Line lines = 2 [(bones.v1.nested_write_of) = \"MoveLine\"];";
    let source = contract(field, "  rpc MoveLine(MoveLineReq) returns (Res);\n").replace(
        "message Res {}",
        "message MoveLineReq { Line from = 1; Line to = 2; }\nmessage Res {}",
    );
    assert_eq!(
        reason_of_source(&source),
        NestedWriteOfReason::ChildMismatch
    );
}

#[test]
fn when_every_carrier_refuses_the_first_reason_is_reported() {
    // `Orders.Create` refuses "Create" as itself; `Billing.Bill` has no such
    // method. Carriers sort by method name, so `Billing.Bill` reports first.
    let field = "repeated Line lines = 2 [(bones.v1.nested_write_of) = \"Create\"];";
    let source = contract(field, "").replace(
        "service Billing { rpc Put(Line) returns (Res); }",
        "service Billing { rpc Put(Line) returns (Res); rpc Bill(CreateReq) returns (Res); }",
    );
    assert_eq!(
        reason_of_source(&source),
        NestedWriteOfReason::UnknownMethod
    );
}

#[test]
fn a_qualified_self_reference_is_refused() {
    let field =
        "repeated Line lines = 2 [(bones.v1.nested_write_of) = \"orders.v1.Orders/Create\"];";
    assert_eq!(reason_of(field, ""), NestedWriteOfReason::SelfReference);
}

#[test]
fn a_qualified_unknown_method_is_refused() {
    let field = "repeated Line lines = 2 [(bones.v1.nested_write_of) = \"orders.v1.Orders/Nope\"];";
    assert_eq!(reason_of(field, ""), NestedWriteOfReason::UnknownMethod);
}

#[test]
fn a_set_of_scalar_items_is_refused() {
    let source = contract(
        "optional LineSet lines = 2 [(bones.v1.nested_write_of) = \"Put\"];",
        "",
    )
    .replace(
        "message LineSet { repeated Line items = 1; }",
        "message LineSet { repeated string items = 1; }",
    );
    let found = refused(&source);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].reason, NestedWriteOfReason::NotChildren);
}

#[test]
fn refusals_come_in_message_then_field_order() {
    // Declaration order, not alphabetical: `CreateReq` before `AReq`, and
    // `zed` before `alpha`.
    let source = contract(
        "repeated Line zed = 2 [(bones.v1.nested_write_of) = \"Nope\"];\n\
         repeated Line alpha = 3 [(bones.v1.nested_write_of) = \"Nope\"];",
        "  rpc Other(AReq) returns (Res);\n",
    )
    .replace(
        "message Res {}",
        "message AReq { repeated Line a = 1 [(bones.v1.nested_write_of) = \"Nope\"]; }\nmessage Res {}",
    );
    let fields: Vec<String> = refused(&source).into_iter().map(|r| r.field).collect();
    assert_eq!(
        fields,
        [
            "orders.v1.CreateReq.zed",
            "orders.v1.CreateReq.alpha",
            "orders.v1.AReq.a"
        ]
    );
}

#[test]
fn a_single_whose_request_carries_the_child_passes() {
    for (field, rpc) in [
        (
            "repeated Line lines = 2 [(bones.v1.nested_write_of) = \"PutLine\"];",
            "  rpc PutLine(PutLineReq) returns (Res);\n",
        ),
        (
            "optional LineSet lines = 2 [(bones.v1.nested_write_of) = \"PutLine\"];",
            "  rpc PutLine(PutLineReq) returns (Res);\n",
        ),
    ] {
        let source = contract(field, rpc).replace(
            "message Res {}",
            "message PutLineReq { string order_id = 1; Line line = 2; }\nmessage Res {}",
        );
        assert_eq!(
            check_nested_write_of(&descriptor_set(&source)),
            Ok(()),
            "{field}"
        );
    }
}

#[test]
fn children_of_a_message_the_single_does_not_write_are_refused() {
    // `Pair` is not `Line`; `PutLines` carries `Line` only repeated, and
    // `PutLineMap` only in a map, so neither request carries the child as a
    // singular field.
    for (field, rpc, req) in [
        (
            "repeated Pair lines = 2 [(bones.v1.nested_write_of) = \"Put\"];",
            "",
            "",
        ),
        (
            "optional PairSet lines = 2 [(bones.v1.nested_write_of) = \"Put\"];",
            "",
            "",
        ),
        (
            "repeated Line lines = 2 [(bones.v1.nested_write_of) = \"PutLines\"];",
            "  rpc PutLines(PutLinesReq) returns (Res);\n",
            "message PutLinesReq { string order_id = 1; repeated Line lines = 2; }\n",
        ),
        (
            "repeated Line lines = 2 [(bones.v1.nested_write_of) = \"PutLineMap\"];",
            "  rpc PutLineMap(PutLineMapReq) returns (Res);\n",
            "message PutLineMapReq { map<string, Line> lines = 1; }\n",
        ),
    ] {
        let source =
            contract(field, rpc).replace("message Res {}", &format!("{req}message Res {{}}"));
        assert_eq!(
            reason_of_source(&source),
            NestedWriteOfReason::ChildMismatch,
            "{field}"
        );
    }
}

#[test]
fn a_non_children_field_is_reported_before_a_child_mismatch() {
    let field = "map<string, Pair> lines = 2 [(bones.v1.nested_write_of) = \"Put\"];";
    assert_eq!(reason_of(field, ""), NestedWriteOfReason::NotChildren);
}

#[test]
fn a_request_a_read_takes_is_refused() {
    // `Create` is a write, but `Peek` takes the same request and declares
    // NO_SIDE_EFFECTS; the read is refused before the name is resolved.
    for named in ["Put", "Nope"] {
        let field = format!("repeated Line lines = 2 [(bones.v1.nested_write_of) = \"{named}\"];");
        let rpc =
            "  rpc Peek(CreateReq) returns (Res) { option idempotency_level = NO_SIDE_EFFECTS; }\n";
        assert_eq!(
            reason_of(&field, rpc),
            NestedWriteOfReason::ReadRequest,
            "{named}"
        );
    }
}

#[test]
fn a_request_only_a_read_takes_is_refused() {
    let source = contract(
        "repeated Line lines = 2 [(bones.v1.nested_write_of) = \"Put\"];",
        "",
    )
    .replace(
        "rpc Create(CreateReq) returns (Res);",
        "rpc Create(CreateReq) returns (Res) { option idempotency_level = NO_SIDE_EFFECTS; }",
    );
    assert_eq!(reason_of_source(&source), NestedWriteOfReason::ReadRequest);
}

fn reason_of_source(source: &str) -> NestedWriteOfReason {
    let found = refused(source);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].field, "orders.v1.CreateReq.lines");
    found[0].reason
}
