// SPDX-License-Identifier: MIT
//! Real compiled product manifests shared by document tests and library BDD.

use api_bones_automation::{
    CompileInput, DocumentError, DocumentInput, DocumentPhase, check_document, compile,
};
use prost_reflect::{DescriptorPool, DynamicMessage, Value, prost::Message};
use protox::Compiler;

use crate::resolver::{PATH, Resolver};

const TEMPLATE: &str = r#"
  option (bones.v1.automation_templates) = {
    name: "forward" label: { text: "Forward an order" }
    document: {
      trigger: { event: "orders.v1.Placed" }
      parameters: { name: "channel" label: { text: "Channel" } required: true text: {} }
      steps: { id: "send" action: {
        action: { product: "warehouse" method: "orders.v1.Orders.Send" }
        inputs: { field: "text" parameter: "channel" }
      } }
    }
  };
"#;

const SOURCE: &str = r#"
syntax = "proto3";
package orders.v1;
import "bones/v1/annotations.proto";
import "bones/v1/automation_manifest.proto";
enum State { STATE_UNSPECIFIED = 0; STATE_READY = 1; }
enum Other { OTHER_UNSPECIFIED = 0; OTHER_READY = 1; }
message Item { string id = 1 [(bones.v1.classification) = DATA_CLASSIFICATION_INTERNAL]; }
message OtherItem { string id = 1 [(bones.v1.classification) = DATA_CLASSIFICATION_INTERNAL]; }
message Placed {
  option (bones.v1.automation_trigger) = { label: { text: "Order placed" } };
  string text = 1 [(bones.v1.classification) = DATA_CLASSIFICATION_INTERNAL];
  repeated string texts = 2 [(bones.v1.classification) = DATA_CLASSIFICATION_INTERNAL];
  State state = 3 [(bones.v1.classification) = DATA_CLASSIFICATION_INTERNAL];
  Item item = 4 [(bones.v1.classification) = DATA_CLASSIFICATION_INTERNAL];
  map<string, string> labels = 5 [(bones.v1.classification) = DATA_CLASSIFICATION_INTERNAL];
}
message Request { string text = 1; repeated string texts = 2; State state = 3; Item item = 4; map<string, string> labels = 5; }
message Response { string text = 1; }
message FindRequest { string query = 1; }
message Candidate {
  string id = 1 [(bones.v1.classification) = DATA_CLASSIFICATION_INTERNAL];
  string title = 2 [(bones.v1.classification) = DATA_CLASSIFICATION_INTERNAL];
}
message FindResponse { repeated Candidate candidates = 1 [(bones.v1.classification) = DATA_CLASSIFICATION_INTERNAL]; }
service Orders {
  // TEMPLATE
  rpc Send(Request) returns (Response) {
    option (bones.v1.authz) = { kind: AUTHZ_KIND_AUTHENTICATED requires: "orders:send" };
    option (bones.v1.automation_action) = { label: { text: "Send" } effect: ACTION_EFFECT_COMPENSABLE idempotency: IDEMPOTENCY_MODE_NATURAL };
  }
  rpc Find(FindRequest) returns (FindResponse) {
    option (bones.v1.authz) = { kind: AUTHZ_KIND_AUTHENTICATED requires: "orders:read" };
    option (bones.v1.automation_action) = {
      label: { text: "Find" } effect: ACTION_EFFECT_PURE
      resolves: { query_field: "query" results_field: "candidates" value_field: "id" label_field: "title" }
    };
  }
}
"#;

pub struct Fixture {
    pub document: Vec<u8>,
    pub manifests: Vec<Vec<u8>>,
}

impl Fixture {
    pub fn check(&self, phase: DocumentPhase) -> Result<(), DocumentError> {
        let manifests: Vec<&[u8]> = self.manifests.iter().map(Vec::as_slice).collect();
        check_document(&DocumentInput {
            document: &self.document,
            product: "orders",
            manifests: &manifests,
            phase,
        })
    }
}

fn compiled(source: &str, product: &str) -> (Vec<u8>, DescriptorPool) {
    let mut compiler = Compiler::with_file_resolver(Resolver(source.to_owned()));
    compiler.include_imports(true);
    compiler.open_file(PATH).expect("fixture protos compile");
    let pool = compiler.descriptor_pool();
    let bytes = compile(&CompileInput {
        descriptor_set: &compiler.encode_file_descriptor_set(),
        sources: &[(PATH.to_owned(), source.as_bytes().to_vec())],
        product,
        version: "1.2.3",
        commit: "0123456789abcdef0123456789abcdef01234567",
    })
    .expect("fixture manifest compiles");
    (bytes, pool)
}

fn child(message: &DynamicMessage, field: &str) -> DynamicMessage {
    message
        .get_field_by_name(field)
        .unwrap()
        .as_message()
        .unwrap()
        .clone()
}

fn argument(pool: &DescriptorPool, name: &str, kind: &str) -> Value {
    let mut value = DynamicMessage::new(pool.get_message_by_name("bones.v1.Value").unwrap());
    value.set_field_by_name(
        kind,
        if kind == "text" {
            Value::String("dispatch".to_owned())
        } else {
            Value::Bool(false)
        },
    );
    let mut argument = DynamicMessage::new(pool.get_message_by_name("bones.v1.Argument").unwrap());
    argument.set_field_by_name("parameter", Value::String(name.to_owned()));
    argument.set_field_by_name("value", Value::Message(value));
    Value::Message(argument)
}

fn template_source(case: &str) -> String {
    let mut template = TEMPLATE.to_owned();
    match case {
        "matching trigger" => template = template.replace("parameter: \"channel\"", "trigger_field: \"text\""),
        "unknown action" => template = template.replace("Orders.Send", "Orders.Missing"),
        "unknown product" => template = template.replace("product: \"warehouse\"", "product: \"absent\""),
        "unknown trigger" => template = template.replace("event: \"orders.v1.Placed\"", "product: \"warehouse\" event: \"orders.v1.Absent\""),
        "cross-product trigger" => template = template.replace("trigger: { event:", "trigger: { product: \"warehouse\" event:"),
        "resolver without resolves" => template = template.replace("required: true text: {}", "required: true entity: { resolver: { product: \"warehouse\" method: \"orders.v1.Orders.Send\" } }"),
        "matching resolver" => template = template.replace("required: true text: {}", "required: true entity: { resolver: { product: \"warehouse\" method: \"orders.v1.Orders.Find\" } }"),
        "unknown parameter" => template = template.replace("parameter: \"channel\"", "parameter: \"missing\""),
        "unknown target field" => template = template.replace("field: \"text\"", "field: \"missing\""),
        "unknown source field" => template = template.replace("parameter: \"channel\"", "trigger_field: \"missing\""),
        "matching repeated" | "repeated mismatch" => {
            template = template.replace("field: \"text\" parameter: \"channel\"", "field: \"texts\" trigger_field: \"texts\"");
        }
        "matching enum" | "different enum" => {
            template = template.replace("field: \"text\" parameter: \"channel\"", "field: \"state\" trigger_field: \"state\"");
        }
        "matching message" | "different message" | "changed message" => {
            template = template.replace("field: \"text\" parameter: \"channel\"", "field: \"item\" trigger_field: \"item\"");
        }
        "matching map" | "map mismatch" => {
            template = template.replace("field: \"text\" parameter: \"channel\"", "field: \"labels\" trigger_field: \"labels\"");
        }
        "matching output" | "output mismatch" | "later output" => {
            let earlier = r#"steps: { id: "first" action: { action: { product: "warehouse" method: "orders.v1.Orders.Send" } } }"#;
            template = template.replace("      steps:", &format!("      {earlier}\n      steps:"))
                .replace("parameter: \"channel\"", "step_output: { step: \"first\" field: \"text\" }");
            if case == "later output" { template = template.replace("step: \"first\"", "step: \"send\""); }
        }
        "literal mismatch" => template = template.replace("parameter: \"channel\"", "literal: { boolean: true }"),
        "default value" => template = template.replace("required: true text: {}", "required: true text: {} default_value: { text: \"dispatch\" }"),
        "optional parameter" => template = template.replace("required: true", "required: false"),
        "open required parameter" | "argument value" | "wrong argument" | "unknown argument" | "duplicate argument" | "missing manifest" | "duplicate manifest" | "invalid manifest" | "unsupported manifest" | "invalid document" | "local reference" | "wrong binding type" | "person-reserved action" => {}
        other => panic!("unknown document fixture {other:?}"),
    }
    if case == "local reference" {
        template = template.replace("product: \"warehouse\" ", "");
    }
    template
}

fn remote_source(case: &str) -> String {
    let source = SOURCE.replace("  // TEMPLATE", "");
    let replacement = match case {
        "wrong binding type" => (
            "message Request { string text",
            "message Request { int64 text",
        ),
        "person-reserved action" => (
            "effect: ACTION_EFFECT_COMPENSABLE",
            "person_reserved: true effect: ACTION_EFFECT_COMPENSABLE",
        ),
        "repeated mismatch" => ("repeated string texts", "string texts"),
        "different enum" => ("State state = 3;", "Other state = 3;"),
        "different message" => ("Item item = 4;", "OtherItem item = 4;"),
        "changed message" => ("message Item { string id", "message Item { int64 id"),
        "map mismatch" => (
            "map<string, string> labels = 5;",
            "map<string, bool> labels = 5;",
        ),
        "output mismatch" => (
            "message Response { string text",
            "message Response { bool text",
        ),
        _ => return source,
    };
    source.replace(replacement.0, replacement.1)
}

pub fn fixture(case: &str) -> Fixture {
    let template = template_source(case);
    let (own, pool) = compiled(&SOURCE.replace("  // TEMPLATE", &template), "orders");
    let (remote, _) = compiled(&remote_source(case), "warehouse");
    let manifest = DynamicMessage::decode(
        pool.get_message_by_name("bones.v1.AutomationManifest")
            .unwrap(),
        own.as_slice(),
    )
    .unwrap();
    let templates = manifest.get_field_by_name("templates").unwrap();
    let mut document = child(
        templates.as_list().unwrap()[0].as_message().unwrap(),
        "document",
    );
    let arguments = match case {
        "argument value" => vec![argument(&pool, "channel", "text")],
        "wrong argument" => vec![argument(&pool, "channel", "boolean")],
        "unknown argument" => vec![argument(&pool, "missing", "text")],
        "duplicate argument" => vec![argument(&pool, "channel", "text"); 2],
        _ => Vec::new(),
    };
    document.set_field_by_name("arguments", Value::List(arguments));
    let mut fixture = Fixture {
        document: document.encode_to_vec(),
        manifests: vec![own, remote],
    };
    match case {
        "missing manifest" => {
            fixture.manifests.pop();
        }
        "duplicate manifest" => fixture.manifests.push(fixture.manifests[1].clone()),
        "invalid manifest" => fixture.manifests[1] = vec![0xff],
        "unsupported manifest" => {
            let mut manifest = DynamicMessage::decode(
                pool.get_message_by_name("bones.v1.AutomationManifest")
                    .unwrap(),
                fixture.manifests[1].as_slice(),
            )
            .unwrap();
            manifest.set_field_by_name("format", Value::U32(99));
            fixture.manifests[1] = manifest.encode_to_vec();
        }
        "invalid document" => fixture.document = vec![0xff],
        _ => {}
    }
    fixture
}
