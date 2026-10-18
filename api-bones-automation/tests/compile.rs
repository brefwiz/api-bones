// SPDX-License-Identifier: MIT
//! The compiler against fixture contracts built with protox: one good
//! contract, then one mutation per refusal rule it enforces, and determinism.

use api_bones_automation::{
    CompileError, CompileInput, MANIFEST_FORMAT, Rule, compile, declaration_digest, read_header,
};
use prost_reflect::{DescriptorPool, DynamicMessage};
use protox::Compiler;

#[path = "support/resolver.rs"]
mod resolver;
use resolver::{PATH, Resolver};

const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

fn descriptor_set(source: &str) -> Vec<u8> {
    let mut compiler = Compiler::with_file_resolver(Resolver(source.to_owned()));
    compiler.include_imports(true);
    compiler
        .open_file(PATH)
        .unwrap_or_else(|e| panic!("fixture does not compile: {e}\n{source}"));
    compiler.encode_file_descriptor_set()
}

const GOOD: &str = r#"
syntax = "proto3";
package orders.v1;
import "bones/v1/annotations.proto";
import "bones/v1/automation_manifest.proto";

message OrderPlaced {
  option (bones.v1.automation_trigger) = { label: { text: "Order placed" } };
  string order_id = 1 [(bones.v1.classification) = DATA_CLASSIFICATION_INTERNAL];
  int64 quantity = 2 [(bones.v1.classification) = DATA_CLASSIFICATION_INTERNAL];
}
message FindRequest { string query = 1; }
message Found { string id = 1 [(bones.v1.classification) = DATA_CLASSIFICATION_INTERNAL]; string title = 2 [(bones.v1.classification) = DATA_CLASSIFICATION_INTERNAL]; }
message FindResponse { repeated Found orders = 1 [(bones.v1.classification) = DATA_CLASSIFICATION_INTERNAL]; }
message ShipRequest { string order_id = 1; int64 quantity = 2; bool express = 3; string note = 4; }
message ShipResponse { string shipment_id = 1; }

service Orders {
  option (bones.v1.automation_templates) = {
    name: "ship-on-order"
    label: { text: "Ship on order" }
    document: {
      trigger: { event: "orders.v1.OrderPlaced" }
      parameters: {
        name: "express"
        label: { text: "Express" }
        boolean: {}
        default_value: { boolean: true }
      }
      steps: {
        id: "ship"
        action: {
          action: { method: "orders.v1.Orders.Ship" }
          inputs: { field: "quantity" literal: { integer: 1 } }
          inputs: { field: "express" parameter: "express" }
          inputs: { field: "order_id" trigger_field: "order_id" }
        }
      }
    }
  };
  rpc Find(FindRequest) returns (FindResponse) {
    option (bones.v1.authz) = { kind: AUTHZ_KIND_AUTHENTICATED, requires: "orders:read" };
    option (bones.v1.automation_action) = {
      label: { text: "Find orders" }
      effect: ACTION_EFFECT_PURE
      resolves: { query_field: "query", results_field: "orders", value_field: "id", label_field: "title" }
    };
  }
  rpc Ship(ShipRequest) returns (ShipResponse) {
    option (bones.v1.authz) = { kind: AUTHZ_KIND_AUTHENTICATED, requires: "orders:ship" };
    option (bones.v1.automation_action) = {
      label: { text: "Ship an order" }
      effect: ACTION_EFFECT_COMPENSABLE
      idempotency: IDEMPOTENCY_MODE_NATURAL
    };
  }
}
"#;

fn sources(source: &str) -> Vec<(String, Vec<u8>)> {
    vec![(PATH.to_owned(), source.as_bytes().to_vec())]
}

fn compile_source(source: &str) -> Result<Vec<u8>, CompileError> {
    let set = descriptor_set(source);
    compile(&CompileInput {
        descriptor_set: &set,
        sources: &sources(source),
        product: "orders",
        version: "1.2.3",
        commit: COMMIT,
    })
}

/// `GOOD` with `from` replaced by `to`; the mutation must find its target.
fn mutated(from: &str, to: &str) -> String {
    assert!(GOOD.contains(from), "fixture lacks {from:?}");
    GOOD.replacen(from, to, 1)
}

fn refusals(source: &str) -> Vec<(Rule, String)> {
    match compile_source(source) {
        Err(CompileError::Refused(list)) => list.into_iter().map(|r| (r.rule, r.site)).collect(),
        other => panic!("expected refusals, got {other:?}"),
    }
}

fn refused_with(rule: Rule, source: &str) {
    let found = refusals(source);
    assert!(
        found.iter().any(|(r, _)| *r == rule),
        "expected {rule:?}, got {found:?}"
    );
}

fn manifest() -> DynamicMessage {
    let bytes = compile_source(GOOD).unwrap();
    let pool = DescriptorPool::decode(
        Compiler::with_file_resolver(Resolver(GOOD.to_owned()))
            .include_imports(true)
            .open_file("bones/v1/automation_manifest.proto")
            .unwrap()
            .encode_file_descriptor_set()
            .as_slice(),
    )
    .unwrap();
    DynamicMessage::decode(
        pool.get_message_by_name("bones.v1.AutomationManifest")
            .unwrap(),
        bytes.as_slice(),
    )
    .unwrap()
}

fn list(message: &DynamicMessage, field: &str) -> Vec<DynamicMessage> {
    message
        .get_field_by_name(field)
        .unwrap()
        .as_list()
        .unwrap()
        .iter()
        .map(|v| v.as_message().unwrap().clone())
        .collect()
}

fn text(message: &DynamicMessage, field: &str) -> String {
    message
        .get_field_by_name(field)
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn the_good_contract_compiles_into_a_complete_manifest() {
    let bytes = compile_source(GOOD).unwrap();
    let header = read_header(&bytes).unwrap();
    assert_eq!(header.format, MANIFEST_FORMAT);
    assert_eq!(header.product, "orders");
    assert_eq!(header.version, "1.2.3");
    assert_eq!(header.commit, COMMIT);
    assert_eq!(
        header.declaration_digest,
        declaration_digest(&sources(GOOD))
    );

    let m = manifest();
    let triggers = list(&m, "triggers");
    assert_eq!(triggers.len(), 1);
    assert_eq!(text(&triggers[0], "event"), "orders.v1.OrderPlaced");
    let actions = list(&m, "actions");
    let methods: Vec<String> = actions.iter().map(|a| text(a, "method")).collect();
    assert_eq!(methods, ["orders.v1.Orders.Find", "orders.v1.Orders.Ship"]);
    let grants: Vec<String> = actions[1]
        .get_field_by_name("grants")
        .unwrap()
        .as_list()
        .unwrap()
        .iter()
        .map(|g| g.as_str().unwrap().to_owned())
        .collect();
    assert_eq!(grants, ["orders:ship"]);
    assert!(m.has_field_by_name("schema"));
}

#[test]
fn a_template_is_resolved_to_its_own_product_with_ordered_inputs() {
    let templates = list(&manifest(), "templates");
    assert_eq!(templates.len(), 1);
    let document = templates[0]
        .get_field_by_name("document")
        .unwrap()
        .as_message()
        .unwrap()
        .clone();
    let trigger = document
        .get_field_by_name("trigger")
        .unwrap()
        .as_message()
        .unwrap()
        .clone();
    assert_eq!(text(&trigger, "product"), "orders");
    let step = list(&document, "steps").remove(0);
    let action = step
        .get_field_by_name("action")
        .unwrap()
        .as_message()
        .unwrap()
        .clone();
    let reference = action
        .get_field_by_name("action")
        .unwrap()
        .as_message()
        .unwrap()
        .clone();
    assert_eq!(text(&reference, "product"), "orders");
    let fields: Vec<String> = list(&action, "inputs")
        .iter()
        .map(|i| text(i, "field"))
        .collect();
    assert_eq!(fields, ["express", "order_id", "quantity"]);
}

#[test]
fn equal_input_gives_equal_bytes_whatever_the_order_of_sources() {
    let set = descriptor_set(GOOD);
    let extra = ("other/v1/unused.proto".to_owned(), b"x".to_vec());
    let one = vec![sources(GOOD).remove(0), extra.clone()];
    let two = vec![extra, sources(GOOD).remove(0)];
    let run = |s: &[(String, Vec<u8>)]| {
        compile(&CompileInput {
            descriptor_set: &set,
            sources: s,
            product: "orders",
            version: "1.2.3",
            commit: COMMIT,
        })
        .unwrap()
    };
    let first = run(&one);
    assert_eq!(first, run(&one));
    assert_eq!(first, run(&two));
    assert_eq!(first, compile_source(GOOD).unwrap());
}

#[test]
fn a_changed_declaration_changes_the_manifest() {
    let other = mutated("Order placed", "Order was placed");
    assert_ne!(
        compile_source(GOOD).unwrap(),
        compile_source(&other).unwrap()
    );
}

#[test]
fn unusable_input_is_refused_before_compiling() {
    let set = descriptor_set(GOOD);
    let src = sources(GOOD);
    let run = |product: &str, version: &str, commit: &str| {
        compile(&CompileInput {
            descriptor_set: &set,
            sources: &src,
            product,
            version,
            commit,
        })
        .unwrap_err()
    };
    for (product, version, commit) in [
        ("", "1.2.3", COMMIT),
        ("orders", "", COMMIT),
        ("orders", "v1.2.3", COMMIT),
        ("orders", "1.2.3", "abc"),
        (
            "orders",
            "1.2.3",
            "0123456789ABCDEF0123456789abcdef01234567",
        ),
    ] {
        assert!(
            matches!(run(product, version, commit), CompileError::Input(_)),
            "{product} {version} {commit}"
        );
    }
}

#[test]
fn a_set_that_does_not_decode_is_refused() {
    let err = compile(&CompileInput {
        descriptor_set: &[0xff, 0xff, 0xff],
        sources: &[],
        product: "orders",
        version: "1.2.3",
        commit: COMMIT,
    })
    .unwrap_err();
    assert!(matches!(err, CompileError::Descriptors(_)), "{err:?}");
}

#[test]
fn a_set_without_declarations_is_refused() {
    let plain = "syntax = \"proto3\";\npackage orders.v1;\nmessage Nothing {}\n";
    assert_eq!(
        compile_source(plain).unwrap_err(),
        CompileError::NoDeclarations
    );
    let imported = "syntax = \"proto3\";\npackage orders.v1;\nimport \"bones/v1/annotations.proto\";\nmessage Nothing {}\n";
    assert_eq!(
        compile_source(imported).unwrap_err(),
        CompileError::NoDeclarations
    );
}

#[test]
fn a_declaring_file_without_a_source_is_refused() {
    let set = descriptor_set(GOOD);
    let err = compile(&CompileInput {
        descriptor_set: &set,
        sources: &[],
        product: "orders",
        version: "1.2.3",
        commit: COMMIT,
    })
    .unwrap_err();
    assert_eq!(err, CompileError::MissingSource(PATH.to_owned()));
}

#[test]
fn t1_a_trigger_needs_a_label() {
    let source = mutated("{ label: { text: \"Order placed\" } }", "{}");
    assert_eq!(
        refusals(&source)[0],
        (Rule::T1, "trigger orders.v1.OrderPlaced".into())
    );
}

#[test]
fn a1_an_action_needs_a_label_and_an_effect() {
    refused_with(
        Rule::A1,
        &mutated("      effect: ACTION_EFFECT_COMPENSABLE\n", ""),
    );
    refused_with(Rule::A1, &mutated("label: { text: \"Ship an order\" }", ""));
}

#[test]
fn a2_an_action_must_be_authenticated_with_requires() {
    refused_with(Rule::A2, &mutated(", requires: \"orders:ship\"", ""));
    refused_with(
        Rule::A2,
        &mutated(
            "kind: AUTHZ_KIND_AUTHENTICATED, requires: \"orders:ship\"",
            "kind: AUTHZ_KIND_PUBLIC",
        ),
    );
}

#[test]
fn a3_an_action_must_be_unary() {
    refused_with(
        Rule::A3,
        &mutated("returns (ShipResponse)", "returns (stream ShipResponse)"),
    );
    refused_with(
        Rule::A3,
        &mutated("Ship(ShipRequest)", "Ship(stream ShipRequest)"),
    );
}

const CLS: &str = "[(bones.v1.classification) = DATA_CLASSIFICATION_INTERNAL]";
const NATURAL: &str = "idempotency: IDEMPOTENCY_MODE_NATURAL";

fn keyed(fields: &str) -> String {
    mutated(
        NATURAL,
        &format!("idempotency: IDEMPOTENCY_MODE_KEYED {fields}"),
    )
}

#[test]
fn a4_a_lasting_action_declares_how_a_repeat_behaves() {
    let source = mutated(&format!("      {NATURAL}\n"), "");
    assert_eq!(
        refusals(&source),
        [(Rule::A4, "action orders.v1.Orders.Ship".to_owned())]
    );
    assert!(compile_source(GOOD).is_ok());
}

#[test]
fn a4_a_keyed_action_names_top_level_request_fields() {
    refused_with(Rule::A4, &keyed(""));
    refused_with(Rule::A4, &keyed("idempotency_key_fields: \"nope\""));
    refused_with(Rule::A4, &keyed("idempotency_key_fields: \"order_id.x\""));
    refused_with(
        Rule::A4,
        &keyed("idempotency_key_fields: \"order_id\" idempotency_key_fields: \"nope\""),
    );
    assert!(
        compile_source(&keyed(
            "idempotency_key_fields: \"order_id\" idempotency_key_fields: \"note\""
        ))
        .is_ok()
    );
}

fn resolving(from: &str, to: &str) -> String {
    mutated(from, to)
}

#[test]
fn a5_resolves_belongs_to_a_read_action_and_names_real_fields() {
    refused_with(
        Rule::A5,
        &mutated(
            "effect: ACTION_EFFECT_COMPENSABLE",
            "effect: ACTION_EFFECT_COMPENSABLE resolves: { query_field: \"order_id\", results_field: \"shipment_id\", value_field: \"x\", label_field: \"y\" }",
        ),
    );
    for (from, to) in [
        ("query_field: \"query\"", "query_field: \"nope\""),
        ("query_field: \"query\"", "query_field: \"query.x\""),
        ("results_field: \"orders\"", "results_field: \"nope\""),
        ("results_field: \"orders\"", "results_field: \"orders.id\""),
        ("value_field: \"id\"", "value_field: \"nope\""),
        ("label_field: \"title\"", "label_field: \"\""),
    ] {
        refused_with(Rule::A5, &resolving(from, to));
    }
    let scalar = resolving(
        "message FindResponse { repeated Found orders = 1",
        "message FindResponse { string count = 2; repeated Found orders = 1",
    )
    .replace("results_field: \"orders\"", "results_field: \"count\"");
    refused_with(Rule::A5, &scalar);
    let single = resolving(
        "message FindResponse { repeated Found orders = 1",
        "message FindResponse { Found first = 2; repeated Found orders = 1",
    )
    .replace("results_field: \"orders\"", "results_field: \"first\"");
    refused_with(Rule::A5, &single);
}

fn with_import(extra: &str, from: &str, to: &str) -> String {
    mutated(from, to).replacen(
        "import \"bones/v1/annotations.proto\";",
        &format!("import \"bones/v1/annotations.proto\";\nimport \"{extra}\";"),
        1,
    )
}

#[test]
fn a6_no_untyped_payload_is_reachable_from_a_request_or_response() {
    let any = with_import(
        "google/protobuf/any.proto",
        "message ShipRequest { string order_id = 1;",
        "message ShipRequest { google.protobuf.Any extra = 6; string order_id = 1;",
    );
    assert_eq!(
        refusals(&any),
        [(Rule::A6, "action orders.v1.Orders.Ship".to_owned())]
    );
    let nested_struct = with_import(
        "google/protobuf/struct.proto",
        "message ShipResponse { string shipment_id = 1; }",
        "message Wrap { google.protobuf.Struct s = 1; }\nmessage ShipResponse { string shipment_id = 1; repeated Wrap wraps = 2; }",
    );
    refused_with(Rule::A6, &nested_struct);
    let in_map = with_import(
        "google/protobuf/struct.proto",
        "message ShipResponse { string shipment_id = 1; }",
        "message ShipResponse { string shipment_id = 1; map<string, google.protobuf.Value> m = 2; }",
    );
    refused_with(Rule::A6, &in_map);
    let typed = with_import(
        "google/protobuf/timestamp.proto",
        "message ShipRequest { string order_id = 1;",
        "message Node { Node next = 1; google.protobuf.Timestamp at = 2; }\nmessage ShipRequest { Node node = 6; string order_id = 1;",
    );
    assert!(compile_source(&typed).is_ok());
}

#[test]
fn c1_a_covered_field_carries_a_classification() {
    let trigger = mutated(&format!("int64 quantity = 2 {CLS};"), "int64 quantity = 2;");
    assert_eq!(
        refusals(&trigger),
        [(Rule::C1, "trigger orders.v1.OrderPlaced".to_owned())]
    );
    let unspecified = mutated(
        &format!("int64 quantity = 2 {CLS};"),
        "int64 quantity = 2 [(bones.v1.classification) = DATA_CLASSIFICATION_UNSPECIFIED];",
    );
    refused_with(Rule::C1, &unspecified);
    let response = mutated(&format!("string title = 2 {CLS};"), "string title = 2;");
    assert_eq!(
        refusals(&response),
        [(Rule::C1, "action orders.v1.Orders.Find".to_owned())]
    );
}

#[test]
fn c1_descends_into_own_messages_only() {
    let nested = |detail: &str| {
        mutated(
            &format!("int64 quantity = 2 {CLS};"),
            &format!("int64 quantity = 2 {CLS};\n  {detail}"),
        )
        .replacen(
            "message FindRequest",
            "message Detail { string code = 1; Detail again = 2; }\nmessage FindRequest",
            1,
        )
    };
    refused_with(Rule::C1, &nested(&format!("Detail detail = 3 {CLS};")));
    refused_with(
        Rule::C1,
        &nested(&format!("map<string, Detail> by_code = 3 {CLS};")),
    );
    refused_with(
        Rule::C1,
        &nested(&format!("repeated Detail details = 3 {CLS};")),
    );
    let foreign = with_import(
        "google/protobuf/timestamp.proto",
        &format!("int64 quantity = 2 {CLS};"),
        &format!(
            "int64 quantity = 2 {CLS};\n  google.protobuf.Timestamp at = 3 {CLS};\n  map<string, string> labels = 4 {CLS};"
        ),
    );
    assert!(compile_source(&foreign).is_ok());
    let unclassified_map = foreign.replace(&format!("labels = 4 {CLS}"), "labels = 4");
    refused_with(Rule::C1, &unclassified_map);
}

#[test]
fn a7_an_action_cannot_be_person_reserved_and_planner_pinned() {
    refused_with(
        Rule::A7,
        &mutated(
            "label: { text: \"Find orders\" }",
            "label: { text: \"Find orders\" } person_reserved: true planner_pinned: true",
        ),
    );
    let only_pinned = mutated(
        "label: { text: \"Find orders\" }",
        "label: { text: \"Find orders\" } planner_pinned: true",
    );
    assert!(compile_source(&only_pinned).is_ok());
}

#[test]
fn d1_a_document_names_a_declared_trigger_and_actions() {
    refused_with(
        Rule::D1,
        &mutated(
            "orders.v1.OrderPlaced\" }\n      parameters",
            "orders.v1.Nope\" }\n      parameters",
        ),
    );
    refused_with(
        Rule::D1,
        &mutated("orders.v1.Orders.Ship\" }", "orders.v1.Orders.Nope\" }"),
    );
    refused_with(
        Rule::D1,
        &mutated("      trigger: { event: \"orders.v1.OrderPlaced\" }\n", ""),
    );
}

#[test]
fn references_to_another_product_are_left_to_the_layer() {
    let source = mutated(
        "trigger: { event: \"orders.v1.OrderPlaced\" }",
        "trigger: { product: \"billing\" event: \"billing.v1.Anything\" }",
    );
    assert!(compile_source(&source).is_ok());
    let action = mutated(
        "action: { method: \"orders.v1.Orders.Ship\" }",
        "action: { product: \"billing\" method: \"billing.v1.Billing.Charge\" }",
    );
    assert!(compile_source(&action).is_ok());
}

#[test]
fn d2_a_person_reserved_action_cannot_be_a_step() {
    refused_with(
        Rule::D2,
        &mutated(
            "label: { text: \"Ship an order\" }",
            "label: { text: \"Ship an order\" } person_reserved: true",
        ),
    );
}

#[test]
fn d3_steps_read_only_earlier_steps_and_ids_are_unique() {
    let second =
        "steps: { id: \"ship\" action: { action: { method: \"orders.v1.Orders.Find\" } } }";
    let duplicate = mutated(
        "    }\n  };\n  rpc Find",
        &format!("      {second}\n    }}\n  }};\n  rpc Find"),
    );
    refused_with(Rule::D3, &duplicate);

    let step = |id: &str, source: &str| {
        format!(
            "steps: {{ id: \"{id}\" action: {{ action: {{ method: \"orders.v1.Orders.Find\" }} {source} }} }}"
        )
    };
    let forward = mutated(
        "    }\n  };\n  rpc Find",
        &format!(
            "      {}\n      {}\n    }}\n  }};\n  rpc Find",
            step(
                "early",
                "inputs: { field: \"query\" step_output: { step: \"late\" field: \"orders\" } }"
            ),
            step("late", ""),
        ),
    );
    refused_with(Rule::D3, &forward);

    let missing = mutated(
        "    }\n  };\n  rpc Find",
        &format!(
            "      {}\n    }}\n  }};\n  rpc Find",
            step(
                "only",
                "inputs: { field: \"query\" step_output: { step: \"ghost\" field: \"x\" } }"
            ),
        ),
    );
    refused_with(Rule::D3, &missing);

    let earlier = mutated(
        "    }\n  };\n  rpc Find",
        &format!(
            "      {}\n      {}\n    }}\n  }};\n  rpc Find",
            step("first", ""),
            step(
                "second",
                "inputs: { field: \"query\" step_output: { step: \"first\" field: \"orders\" } }"
            ),
        ),
    );
    refused_with(Rule::D4, &earlier);
    let matching = mutated(
        "    }\n  };\n  rpc Find",
        "      steps: { id: \"s1\" action: { action: { method: \"orders.v1.Orders.Ship\" } } }\n      steps: { id: \"s2\" action: { action: { method: \"orders.v1.Orders.Ship\" } inputs: { field: \"note\" step_output: { step: \"s1\" field: \"shipment_id\" } } } }\n    }\n  };\n  rpc Find",
    );
    assert!(compile_source(&matching).is_ok());
}

#[test]
fn d4_bindings_name_real_fields_once_and_agree_in_type() {
    let binding = |extra: &str| {
        mutated(
            "          inputs: { field: \"quantity\" literal: { integer: 1 } }\n",
            &format!(
                "          inputs: {{ field: \"quantity\" literal: {{ integer: 1 }} }}\n          {extra}\n"
            ),
        )
    };
    refused_with(
        Rule::D4,
        &binding("inputs: { field: \"quantity\" literal: { integer: 2 } }"),
    );
    refused_with(
        Rule::D4,
        &binding("inputs: { field: \"nope\" literal: { integer: 2 } }"),
    );
    refused_with(
        Rule::D4,
        &binding("inputs: { field: \"note\" literal: { integer: 2 } }"),
    );
    refused_with(
        Rule::D4,
        &binding("inputs: { field: \"note\" trigger_field: \"quantity\" }"),
    );
    refused_with(
        Rule::D4,
        &binding("inputs: { field: \"note\" trigger_field: \"missing\" }"),
    );
    refused_with(
        Rule::D4,
        &binding("inputs: { field: \"note\" parameter: \"express\" }"),
    );
    refused_with(
        Rule::D4,
        &mutated(
            "field: \"express\" parameter: \"express\"",
            "field: \"note\" parameter: \"express\"",
        ),
    );
    assert!(
        compile_source(&binding(
            "inputs: { field: \"note\" literal: { text: \"hi\" } }"
        ))
        .is_ok()
    );
}

#[test]
fn d5_parameters_are_unique_and_well_formed() {
    let parameter = |extra: &str| {
        mutated(
            "      steps: {",
            &format!("      parameters: {{ {extra} }}\n      steps: {{"),
        )
    };
    refused_with(
        Rule::D5,
        &parameter("name: \"express\" label: { text: \"Again\" } boolean: {}"),
    );
    refused_with(
        Rule::D5,
        &parameter("name: \"c\" label: { text: \"C\" } choice: {}"),
    );
    refused_with(
        Rule::D5,
        &parameter(
            "name: \"c\" label: { text: \"C\" } choice: { choices: { value: \"a\" } choices: { value: \"a\" } }",
        ),
    );
    refused_with(
        Rule::D5,
        &mutated("parameter: \"express\"", "parameter: \"ghost\""),
    );
    refused_with(
        Rule::D5,
        &parameter(
            "name: \"o\" label: { text: \"O\" } entity: { resolver: { method: \"orders.v1.Orders.Ship\" } }",
        ),
    );
    assert!(
        compile_source(&parameter(
            "name: \"o\" label: { text: \"O\" } entity: { resolver: { method: \"orders.v1.Orders.Find\" } }"
        ))
        .is_ok()
    );
    for ok in [
        "name: \"t\" label: { text: \"T\" } text: {}",
        "name: \"n\" label: { text: \"N\" } integer: {} default_value: { integer: 3 }",
        "name: \"c\" label: { text: \"C\" } choice: { choices: { value: \"a\" } choices: { value: \"b\" } } default_value: { text: \"a\" }",
    ] {
        assert!(compile_source(&parameter(ok)).is_ok(), "{ok}");
    }
}

#[test]
fn d6_defaults_match_their_type_and_templates_carry_no_arguments() {
    refused_with(
        Rule::D6,
        &mutated(
            "default_value: { boolean: true }",
            "default_value: { integer: 1 }",
        ),
    );
    refused_with(
        Rule::D6,
        &mutated(
            "      steps: {",
            "      arguments: { parameter: \"express\" value: { boolean: true } }\n      steps: {",
        ),
    );
    refused_with(
        Rule::D6,
        &mutated(
            "      steps: {",
            "      parameters: { name: \"x\" label: { text: \"X\" } default_value: { text: \"a\" } }\n      steps: {",
        ),
    );
}

#[test]
fn u1_a_closed_choice_must_be_set() {
    refused_with(
        Rule::U1,
        &mutated(
            "      steps: {",
            "      parameters: { name: \"x\" label: { text: \"X\" } }\n      steps: {",
        ),
    );
    refused_with(
        Rule::U1,
        &mutated(
            "      steps: {",
            "      steps: { id: \"bare\" }\n      steps: {",
        ),
    );
    refused_with(
        Rule::U1,
        &mutated(
            "inputs: { field: \"quantity\" literal: { integer: 1 } }",
            "inputs: { field: \"quantity\" }",
        ),
    );
}

#[test]
fn m1_a_template_is_shaped_as_the_contract_requires() {
    refused_with(
        Rule::M1,
        &mutated("    label: { text: \"Ship on order\" }\n", ""),
    );
    refused_with(Rule::M1, &mutated("    name: \"ship-on-order\"\n", ""));
    refused_with(
        Rule::M1,
        &mutated(
            "name: \"express\"\n        label: { text: \"Express\" }",
            "name: \"express\"",
        ),
    );
    let steps_start = GOOD.find("      steps: {").unwrap();
    let steps_end = GOOD.find("    }\n  };\n  rpc Find").unwrap();
    let no_steps = format!("{}{}", &GOOD[..steps_start], &GOOD[steps_end..]);
    refused_with(Rule::M1, &no_steps);
}

#[test]
fn two_templates_cannot_share_a_name() {
    let twice = mutated(
        "  rpc Find",
        "  option (bones.v1.automation_templates) = { name: \"ship-on-order\" label: { text: \"Again\" } };\n  rpc Find",
    );
    refused_with(Rule::M1, &twice);
}

#[test]
fn every_refusal_is_reported_in_one_error_and_renders() {
    let source = mutated("{ label: { text: \"Order placed\" } }", "{}").replacen(
        "      effect: ACTION_EFFECT_COMPENSABLE\n",
        "",
        1,
    );
    let err = compile_source(&source).unwrap_err();
    let CompileError::Refused(list) = &err else {
        panic!("{err:?}")
    };
    assert!(list.len() >= 2);
    let rendered = err.to_string();
    assert!(rendered.contains("T1 at trigger orders.v1.OrderPlaced"));
    assert!(rendered.contains("A1 at action orders.v1.Orders.Ship"));
    assert!(rendered.contains("; "));
    let _: &dyn std::error::Error = &err;
}

#[test]
fn every_error_variant_renders() {
    for (error, needle) in [
        (CompileError::Descriptors("d".into()), "unusable: d"),
        (CompileError::NoDeclarations, "no trigger"),
        (CompileError::Input("i".into()), "invalid: i"),
        (CompileError::MissingSource("f".into()), "file f"),
    ] {
        assert!(error.to_string().contains(needle), "{error}");
    }
}

#[test]
fn every_rule_names_itself() {
    let names: Vec<&str> = [
        Rule::D1,
        Rule::D2,
        Rule::D3,
        Rule::D4,
        Rule::D5,
        Rule::D6,
        Rule::T1,
        Rule::A1,
        Rule::A2,
        Rule::A3,
        Rule::A4,
        Rule::A5,
        Rule::A6,
        Rule::A7,
        Rule::C1,
        Rule::U1,
        Rule::M1,
    ]
    .iter()
    .map(|r| r.as_str())
    .collect();
    assert_eq!(
        names,
        [
            "D1", "D2", "D3", "D4", "D5", "D6", "T1", "A1", "A2", "A3", "A4", "A5", "A6", "A7",
            "C1", "U1", "M1"
        ]
    );
}

#[test]
fn the_manifest_header_refuses_what_it_cannot_read() {
    use api_bones_automation::ManifestError;
    assert!(matches!(
        read_header(&[0xff, 0xff]),
        Err(ManifestError::Undecodable(_))
    ));
    assert_eq!(
        read_header(&[]).unwrap_err(),
        ManifestError::UnknownFormat(0)
    );
    assert_eq!(
        read_header(&[0x08, 0x02]).unwrap_err(),
        ManifestError::UnknownFormat(2)
    );
}

#[test]
fn nested_trigger_paths_resolve_through_singular_messages() {
    let nested = GOOD
        .replace(
            "DATA_CLASSIFICATION_INTERNAL];\n}\nmessage FindRequest",
            "DATA_CLASSIFICATION_INTERNAL];\n  Detail detail = 3 [(bones.v1.classification) = DATA_CLASSIFICATION_INTERNAL];\n}\nmessage Detail { string code = 1 [(bones.v1.classification) = DATA_CLASSIFICATION_INTERNAL]; }\nmessage FindRequest",
        )
        .replace(
            "inputs: { field: \"order_id\" trigger_field: \"order_id\" }",
            "inputs: { field: \"order_id\" trigger_field: \"detail.code\" }",
        );
    assert!(compile_source(&nested).is_ok());
    for bad in ["detail.nope", "order_id.deeper", "quantity.x"] {
        let source = nested.replace("detail.code", bad);
        refused_with(Rule::D4, &source);
    }
    let listed = nested.replace("  Detail detail = 3", "  repeated Detail detail = 3");
    refused_with(Rule::D4, &listed);
}

#[test]
fn declarations_missing_their_parts_are_refused_not_skipped() {
    refused_with(
        Rule::A2,
        &mutated(
            "    option (bones.v1.authz) = { kind: AUTHZ_KIND_AUTHENTICATED, requires: \"orders:ship\" };\n",
            "",
        ),
    );
    let bare_template = mutated(
        "  rpc Find",
        "  option (bones.v1.automation_templates) = { name: \"bare\" label: { text: \"Bare\" } };\n  rpc Find",
    );
    let found = refusals(&bare_template);
    assert!(
        found.contains(&(Rule::D1, "template bare".to_owned())),
        "{found:?}"
    );
    assert!(
        found.contains(&(Rule::M1, "template bare".to_owned())),
        "{found:?}"
    );
    refused_with(
        Rule::D1,
        &mutated(
            "      steps: {",
            "      steps: { id: \"x\" action: {} }\n      steps: {",
        ),
    );
    refused_with(
        Rule::D1,
        &mutated(
            "      steps: {",
            "      parameters: { name: \"o\" label: { text: \"O\" } entity: {} }\n      steps: {",
        ),
    );
    let boolean_literal = mutated(
        "inputs: { field: \"quantity\" literal: { integer: 1 } }",
        "inputs: { field: \"quantity\" literal: { boolean: true } }",
    );
    refused_with(Rule::D4, &boolean_literal);
}

#[test]
fn templates_are_ordered_by_name_in_the_manifest() {
    let start = GOOD
        .find("  option (bones.v1.automation_templates)")
        .unwrap();
    let end = GOOD.find("  };\n  rpc Find").unwrap() + "  };\n".len();
    let first = GOOD[start..end].replace("ship-on-order", "a-ship");
    let source = format!("{}{}{}", &GOOD[..start], first, &GOOD[start..]);
    let bytes = compile_source(&source).unwrap();
    let pool = DescriptorPool::decode(
        Compiler::with_file_resolver(Resolver(source.clone()))
            .include_imports(true)
            .open_file("bones/v1/automation_manifest.proto")
            .unwrap()
            .encode_file_descriptor_set()
            .as_slice(),
    )
    .unwrap();
    let manifest = DynamicMessage::decode(
        pool.get_message_by_name("bones.v1.AutomationManifest")
            .unwrap(),
        bytes.as_slice(),
    )
    .unwrap();
    let names: Vec<String> = list(&manifest, "templates")
        .iter()
        .map(|t| text(t, "name"))
        .collect();
    assert_eq!(names, ["a-ship", "ship-on-order"]);
}
