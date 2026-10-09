// SPDX-License-Identifier: MIT
//! Runs `features/proto_vocabulary.feature`.
//!
//! The shipped proto vocabulary is library code callers embed directly; no
//! generated contract reaches it (SPEC.md `internal_behavior_owners`).

use cucumber::{World, given, then, when};
use prost_reflect::{DescriptorPool, DynamicMessage, ReflectMessage};
use protox::{
    Compiler, Error,
    file::{File, FileResolver, GoogleFileResolver},
};

const CONTRACT: &str = "contract/v1/contract.proto";

#[derive(Default, World)]
struct VocabularyWorld {
    source: String,
    outcome: Option<Result<DescriptorPool, String>>,
}

impl std::fmt::Debug for VocabularyWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VocabularyWorld").finish()
    }
}

struct ShippedAndContract(String);

impl FileResolver for ShippedAndContract {
    fn open_file(&self, name: &str) -> Result<File, Error> {
        if name == CONTRACT {
            return File::from_source(name, &self.0);
        }
        if let Some((_, bytes)) = api_bones_protos::files().find(|(path, _)| *path == name) {
            let source = std::str::from_utf8(bytes).expect("shipped protos are utf8");
            return File::from_source(name, source);
        }
        GoogleFileResolver::new().open_file(name)
    }
}

fn contract(message_options: &str, method_options: &str, field_options: &str) -> String {
    format!(
        "syntax = \"proto3\";\npackage contract.v1;\n\
         import \"bones/v1/annotations.proto\";\n\
         import \"google/api/field_behavior.proto\";\n\
         message Thing {{ {message_options} string id = 1 {field_options}; }}\n\
         message Req {{}}\n\
         service Svc {{ rpc Do(Req) returns (Thing) {{ {method_options} }} }}\n"
    )
}

fn pool(world: &VocabularyWorld) -> &DescriptorPool {
    world
        .outcome
        .as_ref()
        .expect("contract not compiled")
        .as_ref()
        .unwrap_or_else(|e| panic!("contract failed to compile: {e}"))
}

fn method_option(world: &VocabularyWorld, extension: &str) -> prost_reflect::Value {
    let pool = pool(world);
    let ext = pool.get_extension_by_name(extension).expect(extension);
    let options = pool
        .get_service_by_name("contract.v1.Svc")
        .expect("service")
        .methods()
        .next()
        .expect("method")
        .options();
    options.get_extension(&ext).into_owned()
}

fn resource(world: &VocabularyWorld) -> DynamicMessage {
    let pool = pool(world);
    let ext = pool
        .get_extension_by_name("bones.v1.resource")
        .expect("resource extension");
    let options = pool
        .get_message_by_name("contract.v1.Thing")
        .expect("message")
        .options();
    options
        .get_extension(&ext)
        .as_message()
        .expect("resource is a message")
        .clone()
}

fn text(msg: &DynamicMessage, field: &str) -> String {
    msg.get_field_by_name(field)
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| panic!("`{field}` is not a string field"))
}

#[given(
    expr = "a contract declaring a resource {string} identified by {string} with update {string}"
)]
fn given_resource(world: &mut VocabularyWorld, kind: String, identity: String, update: String) {
    world.source = contract(
        &format!(
            "option (bones.v1.resource) = {{ type: \"{kind}\", identity_field: \"{identity}\", \
             methods: {{ update: \"{update}\" }} }};"
        ),
        "",
        "",
    );
}

#[given(expr = "a contract declaring a resource {string} identified by {string} with no overrides")]
fn given_bare_resource(world: &mut VocabularyWorld, kind: String, identity: String) {
    world.source = contract(
        &format!(
            "option (bones.v1.resource) = {{ type: \"{kind}\", identity_field: \"{identity}\" }};"
        ),
        "",
        "",
    );
}

#[given("a contract using the shipped vocabulary")]
fn given_plain(world: &mut VocabularyWorld) {
    world.source = contract("", "", "");
}

#[given(expr = "a contract whose field is declared {string}")]
fn given_field_behavior(world: &mut VocabularyWorld, behavior: String) {
    world.source = contract(
        "",
        "",
        &format!("[(google.api.field_behavior) = {behavior}]"),
    );
}

#[given(expr = "a contract declaring a method with call shape {string}")]
fn given_shape(world: &mut VocabularyWorld, shape: String) {
    world.source = contract("", &format!("option (bones.v1.call_shape) = {shape};"), "");
}

#[given(expr = "a contract declaring a method with MCP title {string}")]
fn given_title(world: &mut VocabularyWorld, title: String) {
    world.source = contract(
        "",
        &format!("option (bones.v1.mcp) = {{ title: \"{title}\" }};"),
        "",
    );
}

#[given("a contract declaring a method with the retired MCP shape set")]
fn given_retired(world: &mut VocabularyWorld) {
    world.source = contract("", "option (bones.v1.mcp) = { shape: 1 };", "");
}

#[when("the contract is compiled against the shipped files")]
fn when_compiled(world: &mut VocabularyWorld) {
    let mut compiler = Compiler::with_file_resolver(ShippedAndContract(world.source.clone()));
    world.outcome = Some(match compiler.open_file(CONTRACT) {
        Ok(_) => Ok(compiler.descriptor_pool()),
        Err(e) => Err(e.to_string()),
    });
}

#[then(expr = "the shipped files include {string}")]
fn then_shipped(_world: &mut VocabularyWorld, path: String) {
    assert!(
        api_bones_protos::files().any(|(p, bytes)| p == path && !bytes.is_empty()),
        "{path} is not shipped"
    );
}

#[then(expr = "the resource type is {string}")]
fn then_type(world: &mut VocabularyWorld, expected: String) {
    assert_eq!(text(&resource(world), "type"), expected);
}

#[then(expr = "the resource identity field is {string}")]
fn then_identity(world: &mut VocabularyWorld, expected: String) {
    assert_eq!(text(&resource(world), "identity_field"), expected);
}

#[then(expr = "the resource update method is {string}")]
fn then_update(world: &mut VocabularyWorld, expected: String) {
    let resource = resource(world);
    let methods = resource
        .get_field_by_name("methods")
        .and_then(|v| v.as_message().cloned())
        .expect("methods is a message");
    assert_eq!(text(&methods, "update"), expected);
}

#[then("the resource declares no method names")]
fn then_no_methods(world: &mut VocabularyWorld) {
    let resource = resource(world);
    if let Some(methods) = resource.get_field_by_name("methods") {
        let methods = methods.as_message().expect("methods is a message").clone();
        for field in methods.descriptor().fields() {
            assert_eq!(text(&methods, field.name()), "", "{}", field.name());
        }
    }
}

#[then(expr = "the extension {string} has number {int}")]
fn then_extension_number(world: &mut VocabularyWorld, name: String, number: u32) {
    let ext = pool(world)
        .get_extension_by_name(&name)
        .unwrap_or_else(|| panic!("{name} is not declared"));
    assert_eq!(ext.number(), number);
}

#[then(expr = "the enum {string} declares {string} as {int}")]
fn then_enum_value(world: &mut VocabularyWorld, name: String, value: String, number: i32) {
    let found = pool(world)
        .get_enum_by_name(&name)
        .unwrap_or_else(|| panic!("{name} is not declared"))
        .get_value_by_name(&value)
        .unwrap_or_else(|| panic!("{value} is not declared"));
    assert_eq!(found.number(), number);
}

#[then(expr = "the enum {string} declares exactly {int} values")]
fn then_enum_count(world: &mut VocabularyWorld, name: String, count: usize) {
    let found = pool(world)
        .get_enum_by_name(&name)
        .unwrap_or_else(|| panic!("{name} is not declared"));
    assert_eq!(found.values().count(), count);
}

#[then(expr = "the field behavior reads back as {string}")]
fn then_field_behavior(world: &mut VocabularyWorld, expected: String) {
    let pool = pool(world);
    let ext = pool
        .get_extension_by_name("google.api.field_behavior")
        .expect("field_behavior extension");
    let options = pool
        .get_message_by_name("contract.v1.Thing")
        .expect("message")
        .get_field_by_name("id")
        .expect("field")
        .options();
    let value = options.get_extension(&ext);
    let list = value.as_list().expect("field_behavior is repeated");
    let behavior = pool
        .get_enum_by_name("google.api.FieldBehavior")
        .expect("FieldBehavior");
    let names: Vec<_> = list
        .iter()
        .map(|v| {
            behavior
                .get_value(v.as_enum_number().expect("enum"))
                .expect("known")
                .name()
                .to_owned()
        })
        .collect();
    assert_eq!(names, [expected]);
}

#[then(expr = "the method call shape is {string}")]
fn then_shape(world: &mut VocabularyWorld, expected: String) {
    let number = method_option(world, "bones.v1.call_shape")
        .as_enum_number()
        .expect("call_shape is an enum");
    let shape = pool(world)
        .get_enum_by_name("bones.v1.CallShape")
        .expect("CallShape");
    assert_eq!(
        shape.get_value(number).expect("known value").name(),
        expected
    );
}

#[then(expr = "the method MCP title is {string}")]
fn then_mcp_title(world: &mut VocabularyWorld, expected: String) {
    let mcp = method_option(world, "bones.v1.mcp");
    let mcp = mcp.as_message().expect("mcp is a message");
    assert_eq!(text(mcp, "title"), expected);
}

#[then(expr = "the MCP projection has only the field {string}")]
fn then_only_field(world: &mut VocabularyWorld, expected: String) {
    let projection = pool(world)
        .get_message_by_name("bones.v1.McpProjection")
        .expect("McpProjection");
    let fields: Vec<String> = projection.fields().map(|f| f.name().to_owned()).collect();
    assert_eq!(fields, [expected]);
}

#[then("compilation fails")]
fn then_fails(world: &mut VocabularyWorld) {
    let outcome = world.outcome.as_ref().expect("contract not compiled");
    assert!(outcome.is_err(), "the contract compiled but must not");
}

#[tokio::main]
async fn main() {
    let features = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/features/proto_vocabulary.feature"
    );
    VocabularyWorld::run(features).await;
}
