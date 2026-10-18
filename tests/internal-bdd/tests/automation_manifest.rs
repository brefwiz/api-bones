// SPDX-License-Identifier: MIT
//! Runs `features/automation_manifest.feature`.
//!
//! The compiler and the signature check are library code callers embed
//! directly; no generated contract reaches them (SPEC.md
//! `internal_behavior_owners`).

use api_bones_automation::signature::{
    AdmitError, SigningKey, TrustedKey, VerifyError, admit, sign, signed_manifest,
};
use api_bones_automation::{CompileError, CompileInput, compile, declaration_digest, read_header};
use cucumber::{World, given, then, when};
use protox::Compiler;

#[path = "../../../api-bones-automation/tests/support/resolver.rs"]
mod resolver;
use resolver::{PATH, Resolver};
const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

#[derive(Default, World)]
struct ManifestWorld {
    source: String,
    outcome: Option<Result<Vec<u8>, CompileError>>,
    manifest: Vec<u8>,
    envelope: Vec<u8>,
    trusted: Vec<TrustedKey>,
    admitted: Option<Result<(), AdmitError>>,
}

impl std::fmt::Debug for ManifestWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ManifestWorld").finish()
    }
}

const GOOD: &str = r#"
syntax = "proto3";
package orders.v1;
import "bones/v1/annotations.proto";
import "bones/v1/automation_manifest.proto";

message OrderPlaced {
  option (bones.v1.automation_trigger) = { label: { text: "Order placed" } };
  string order_id = 1 [(bones.v1.classification) = DATA_CLASSIFICATION_INTERNAL];
}
message ShipRequest { string order_id = 1; }
message ShipResponse { string shipment_id = 1; }

service Orders {
  option (bones.v1.automation_templates) = {
    name: "ship-on-order"
    label: { text: "Ship on order" }
    document: {
      trigger: { event: "orders.v1.OrderPlaced" }
      steps: {
        id: "ship"
        action: {
          action: { method: "orders.v1.Orders.Ship" }
          inputs: { field: "order_id" trigger_field: "order_id" }
        }
      }
    }
  };
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

fn mutate(from: &str, to: &str) -> String {
    assert!(GOOD.contains(from), "fixture lacks {from:?}");
    GOOD.replacen(from, to, 1)
}

fn defective(defect: &str) -> String {
    match defect {
        "a trigger without a label" => mutate("{ label: { text: \"Order placed\" } }", "{}"),
        "an action without an effect" => mutate("      effect: ACTION_EFFECT_COMPENSABLE\n", ""),
        "an action without a required grant" => mutate(", requires: \"orders:ship\"", ""),
        "an action that streams its response" => {
            mutate("returns (ShipResponse)", "returns (stream ShipResponse)")
        }
        "a template naming an undeclared trigger" => {
            mutate("orders.v1.OrderPlaced\" }", "orders.v1.Nope\" }")
        }
        "a template step on a person-reserved action" => mutate(
            "effect: ACTION_EFFECT_COMPENSABLE",
            "effect: ACTION_EFFECT_COMPENSABLE person_reserved: true",
        ),
        "an action that is person-reserved and planner-pinned" => mutate(
            "effect: ACTION_EFFECT_COMPENSABLE",
            "effect: ACTION_EFFECT_COMPENSABLE person_reserved: true planner_pinned: true",
        ),
        "a lasting action that does not declare its idempotency" => {
            mutate("      idempotency: IDEMPOTENCY_MODE_NATURAL\n", "")
        }
        "a resolver on an action that is not a read" => mutate(
            "effect: ACTION_EFFECT_COMPENSABLE",
            "effect: ACTION_EFFECT_COMPENSABLE resolves: { query_field: \"order_id\", results_field: \"shipment_id\", value_field: \"x\", label_field: \"y\" }",
        ),
        "a request that carries an untyped payload" => mutate(
            "message ShipRequest { string order_id = 1; }",
            "message ShipRequest { string order_id = 1; google.protobuf.Any extra = 2; }",
        )
        .replacen(
            "import \"bones/v1/annotations.proto\";",
            "import \"bones/v1/annotations.proto\";\nimport \"google/protobuf/any.proto\";",
            1,
        ),
        "a trigger field without a classification" => mutate(
            " [(bones.v1.classification) = DATA_CLASSIFICATION_INTERNAL]",
            "",
        ),
        "a template with no steps" => {
            let start = GOOD.find("      steps: {").expect("steps");
            let end = GOOD.find("    }\n  };\n  rpc Ship").expect("document end");
            format!("{}{}", &GOOD[..start], &GOOD[end..])
        }
        other => panic!("unknown defect {other}"),
    }
}

fn compile_world(world: &mut ManifestWorld, product: &str, version: &str) {
    let mut compiler = Compiler::with_file_resolver(Resolver(world.source.clone()));
    compiler.include_imports(true);
    compiler.open_file(PATH).expect("fixture compiles");
    let set = compiler.encode_file_descriptor_set();
    let sources = [(PATH.to_owned(), world.source.as_bytes().to_vec())];
    world.outcome = Some(compile(&CompileInput {
        descriptor_set: &set,
        sources: &sources,
        product,
        version,
        commit: COMMIT,
    }));
}

#[given("a valid orders contract")]
fn given_valid(world: &mut ManifestWorld) {
    world.source = GOOD.to_owned();
}

#[given(expr = "an orders contract with {}")]
fn given_defective(world: &mut ManifestWorld, defect: String) {
    world.source = defective(&defect);
}

#[when(expr = "the release {string} of product {string} is compiled")]
fn when_compiled(world: &mut ManifestWorld, version: String, product: String) {
    compile_world(world, &product, &version);
}

#[then(
    expr = "the manifest names product {string}, version {string} and the digest of the contract source"
)]
fn then_names(world: &mut ManifestWorld, product: String, version: String) {
    let bytes = world.outcome.as_ref().unwrap().as_ref().expect("compiled");
    let header = read_header(bytes).unwrap();
    assert_eq!(header.product, product);
    assert_eq!(header.version, version);
    assert_eq!(
        header.declaration_digest,
        declaration_digest(&[(PATH.to_owned(), world.source.as_bytes().to_vec())])
    );
}

#[then("compiling again gives the same bytes")]
fn then_deterministic(world: &mut ManifestWorld) {
    let first = world.outcome.take().unwrap().expect("compiled");
    compile_world(world, "orders", "1.2.3");
    assert_eq!(world.outcome.as_ref().unwrap().as_ref().unwrap(), &first);
}

#[then(expr = "the compilation is refused under rule {string}")]
fn then_refused(world: &mut ManifestWorld, name: String) {
    match world.outcome.as_ref().unwrap() {
        Err(CompileError::Refused(list)) => {
            assert!(list.iter().any(|r| r.rule.as_str() == name), "{list:?}");
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
}

fn key(keyid: &str) -> SigningKey {
    let seed = if keyid == "k1" { 7 } else { 8 };
    SigningKey::from_bytes(&[seed; 32])
}

#[given(
    expr = "a signed manifest of product {string} under key {string} valid from {int} to {int}"
)]
fn given_signed(world: &mut ManifestWorld, product: String, keyid: String, from: i64, to: i64) {
    world.source = GOOD.to_owned();
    compile_world(world, &product, "1.2.3");
    world.manifest = world.outcome.take().unwrap().expect("compiled");
    let signature = sign(&world.manifest, &keyid, &key(&keyid));
    world.envelope = signed_manifest(&world.manifest, &signature);
    world.trusted = vec![TrustedKey {
        keyid: keyid.clone(),
        key: key(&keyid).verifying_key(),
        not_before: from,
        not_after: to,
    }];
}

#[given("the manifest is altered after signing")]
fn given_altered(world: &mut ManifestWorld) {
    let signature = sign(&world.manifest, "k1", &key("k1"));
    let mut altered = world.manifest.clone();
    *altered.last_mut().unwrap() ^= 1;
    world.envelope = signed_manifest(&altered, &signature);
}

#[given(expr = "the layer trusts only key {string}")]
fn given_other_key(world: &mut ManifestWorld, keyid: String) {
    world.trusted = vec![TrustedKey {
        keyid: keyid.clone(),
        key: key(&keyid).verifying_key(),
        not_before: 0,
        not_after: i64::MAX,
    }];
}

#[when(expr = "the layer admits it at {int} for product {string} serving {}")]
fn when_admitted(world: &mut ManifestWorld, at: i64, product: String, digest: String) {
    let own = read_header(&world.manifest).unwrap().declaration_digest;
    let served = if digest == "its own digest" {
        own
    } else {
        "sha256:0000".to_owned()
    };
    world.admitted =
        Some(admit(&world.envelope, &world.trusted, at, &product, &served).map(|_| ()));
}

#[then("the manifest is admitted")]
fn then_admitted(world: &mut ManifestWorld) {
    world.admitted.as_ref().unwrap().as_ref().expect("admitted");
}

#[then(expr = "the admission is refused as {string}")]
fn then_admission_refused(world: &mut ManifestWorld, reason: String) {
    let error = world.admitted.as_ref().unwrap().as_ref().unwrap_err();
    let named = match error {
        AdmitError::Verify(VerifyError::KeyNotValid { .. }) => "key not valid",
        AdmitError::Verify(VerifyError::UnknownKey(_)) => "unknown key",
        AdmitError::Verify(VerifyError::BadSignature(_)) => "bad signature",
        AdmitError::ProductMismatch { .. } => "product mismatch",
        AdmitError::DigestMismatch { .. } => "digest mismatch",
        other => panic!("unexpected refusal {other:?}"),
    };
    assert_eq!(named, reason);
}

#[tokio::main]
async fn main() {
    let features = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/features/automation_manifest.feature"
    );
    ManifestWorld::run(features).await;
}
