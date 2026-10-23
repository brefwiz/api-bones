// SPDX-License-Identifier: MIT
//! Runs `features/proto_vocabulary.feature`.
//!
//! The shipped proto vocabulary is library code callers embed directly; no
//! generated contract reaches it (SPEC.md `internal_behavior_owners`).

use cucumber::{World, given, then, when};
use prost_reflect::prost::Message;
use prost_reflect::{DescriptorPool, DynamicMessage, ReflectMessage, Value};
use protox::{
    Compiler, Error,
    file::{File, FileResolver, GoogleFileResolver},
};

const CONTRACT: &str = "contract/v1/contract.proto";

#[derive(Default, World)]
struct VocabularyWorld {
    source: String,
    outcome: Option<Result<DescriptorPool, String>>,
    signed: Vec<u8>,
    served: Option<DynamicMessage>,
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
    surface_contract(message_options, method_options, field_options, "")
}

fn surface_contract(
    message_options: &str,
    method_options: &str,
    field_options: &str,
    service_options: &str,
) -> String {
    format!(
        "syntax = \"proto3\";\npackage contract.v1;\n\
         import \"bones/v1/annotations.proto\";\n\
         import \"bones/v1/automation_manifest.proto\";\n\
         import \"google/api/field_behavior.proto\";\n\
         message Thing {{ {message_options} string id = 1 {field_options}; }}\n\
         message Req {{ string id = 1; }}\n\
         service Svc {{ {service_options} rpc Do(Req) returns (Thing) {{ {method_options} }} }}\n"
    )
}

const TRIGGER: &str =
    "option (bones.v1.automation_trigger) = { label: { text: \"Departure confirmed\" } };";

const INTERNAL: &str = "[(bones.v1.classification) = DATA_CLASSIFICATION_INTERNAL]";

fn action_options(action: &str) -> String {
    format!(
        "option (bones.v1.authz) = {{ kind: AUTHZ_KIND_AUTHENTICATED, \
         requires: \"departure:confirm\" }}; \
         option (bones.v1.automation_action) = {{ {action} }};"
    )
}

fn template_option(name: &str, method: &str, event: &str) -> String {
    format!(
        "option (bones.v1.automation_templates) = {{ name: \"{name}\", \
         label: {{ text: \"Notify on confirmation\" }}, \
         document: {{ trigger: {{ event: \"{event}\" }}, \
         steps: {{ id: \"notify\", action: {{ action: {{ method: \"{method}\" }}, \
         inputs: {{ field: \"id\", trigger_field: \"id\" }} }} }} }} }};"
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

fn message_option(world: &VocabularyWorld, extension: &str) -> DynamicMessage {
    let pool = pool(world);
    let ext = pool.get_extension_by_name(extension).expect(extension);
    pool.get_message_by_name("contract.v1.Thing")
        .expect("message")
        .options()
        .get_extension(&ext)
        .as_message()
        .expect("option is a message")
        .clone()
}

fn action(world: &VocabularyWorld) -> DynamicMessage {
    method_option(world, "bones.v1.automation_action")
        .as_message()
        .expect("automation_action is a message")
        .clone()
}

fn field_options(world: &VocabularyWorld) -> DynamicMessage {
    pool(world)
        .get_message_by_name("contract.v1.Thing")
        .expect("message")
        .get_field_by_name("id")
        .expect("field")
        .options()
}

fn enum_name(pool: &DescriptorPool, enum_name: &str, value: &Value) -> String {
    let number = value.as_enum_number().expect("enum value");
    pool.get_enum_by_name(enum_name)
        .unwrap_or_else(|| panic!("{enum_name} is not declared"))
        .get_value(number)
        .unwrap_or_else(|| panic!("{number} is not a value of {enum_name}"))
        .name()
        .to_owned()
}

fn label_text(msg: &DynamicMessage) -> String {
    let label = msg.get_field_by_name("label").expect("label");
    text(label.as_message().expect("label is a message"), "text")
}

fn messages(msg: &DynamicMessage, field: &str) -> Vec<DynamicMessage> {
    msg.get_field_by_name(field)
        .unwrap_or_else(|| panic!("`{field}` is not a field"))
        .as_list()
        .unwrap_or_else(|| panic!("`{field}` is not repeated"))
        .iter()
        .map(|v| v.as_message().expect("message element").clone())
        .collect()
}

fn child(msg: &DynamicMessage, field: &str) -> DynamicMessage {
    msg.get_field_by_name(field)
        .and_then(|v| v.as_message().cloned())
        .unwrap_or_else(|| panic!("`{field}` is not a message field"))
}

fn template(world: &VocabularyWorld, name: &str) -> DynamicMessage {
    let pool = pool(world);
    let ext = pool
        .get_extension_by_name("bones.v1.automation_templates")
        .expect("automation_templates extension");
    let options = pool
        .get_service_by_name("contract.v1.Svc")
        .expect("service")
        .options();
    options
        .get_extension(&ext)
        .as_list()
        .expect("automation_templates is repeated")
        .iter()
        .map(|v| v.as_message().expect("template").clone())
        .find(|t| text(t, "name") == name)
        .unwrap_or_else(|| panic!("no template `{name}`"))
}

fn first_step_action(world: &VocabularyWorld, name: &str) -> DynamicMessage {
    let document = child(&template(world, name), "document");
    let step = messages(&document, "steps")
        .into_iter()
        .next()
        .expect("the template has a step");
    child(&step, "action")
}

fn served(world: &VocabularyWorld) -> &DynamicMessage {
    world.served.as_ref().expect("no manifest served")
}

fn new_message(pool: &DescriptorPool, name: &str) -> DynamicMessage {
    DynamicMessage::new(
        pool.get_message_by_name(name)
            .unwrap_or_else(|| panic!("{name} is not declared")),
    )
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

#[given(expr = "a contract declaring a method that batches {string}")]
fn given_batch_of(world: &mut VocabularyWorld, single: String) {
    world.source = contract(
        "",
        &format!("option (bones.v1.batch_of) = \"{single}\";"),
        "",
    );
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

#[given(expr = "a contract declaring its event a trigger labelled {string}")]
fn given_trigger(world: &mut VocabularyWorld, label: String) {
    world.source = contract(
        &format!("option (bones.v1.automation_trigger) = {{ label: {{ text: \"{label}\" }} }};"),
        "",
        "",
    );
}

#[given(expr = "a contract declaring its RPC an action labelled {string} with effect {string}")]
fn given_action(world: &mut VocabularyWorld, label: String, effect: String) {
    world.source = contract(
        "",
        &action_options(&format!(
            "label: {{ text: \"{label}\" }}, effect: {effect}, \
             idempotency: IDEMPOTENCY_MODE_KEYED, idempotency_key_fields: \"id\""
        )),
        "",
    );
}

#[given(expr = "a contract declaring its RPC an action with the guardrail {string}")]
fn given_guardrail(world: &mut VocabularyWorld, flag: String) {
    world.source = contract(
        "",
        &action_options(&format!(
            "label: {{ text: \"Confirm a departure\" }}, \
             effect: ACTION_EFFECT_POINT_OF_NO_RETURN, \
             idempotency: IDEMPOTENCY_MODE_NATURAL, {flag}: true"
        )),
        "",
    );
}

#[given(expr = "a contract declaring a read action resolving entities by {string} into {string}")]
fn given_resolver(world: &mut VocabularyWorld, query: String, results: String) {
    world.source = contract(
        "",
        &action_options(&format!(
            "label: {{ text: \"Find departures\" }}, effect: ACTION_EFFECT_PURE, \
             resolves: {{ query_field: \"{query}\", results_field: \"{results}\", \
             value_field: \"id\", label_field: \"title\" }}"
        )),
        "",
    );
}

#[given(expr = "a contract whose field is classified {string}")]
fn given_classified(world: &mut VocabularyWorld, classification: String) {
    world.source = contract(
        TRIGGER,
        "",
        &format!("[(bones.v1.classification) = {classification}]"),
    );
}

#[given(expr = "a contract shipping a template {string} that calls {string} when {string} fires")]
fn given_template(world: &mut VocabularyWorld, name: String, method: String, event: String) {
    world.source = surface_contract(
        TRIGGER,
        &action_options(
            "label: { text: \"Confirm a departure\" }, effect: ACTION_EFFECT_COMPENSABLE, \
             idempotency: IDEMPOTENCY_MODE_KEYED, idempotency_key_fields: \"id\"",
        ),
        INTERNAL,
        &template_option(&name, &method, &event),
    );
}

#[given(expr = "a contract shipping a template whose step is of kind {string}")]
fn given_dispatch_step(world: &mut VocabularyWorld, kind: String) {
    world.source = surface_contract(
        TRIGGER,
        "",
        INTERNAL,
        &format!(
            "option (bones.v1.automation_templates) = {{ name: \"raw\", \
             label: {{ text: \"Raw\" }}, document: {{ trigger: {{ event: \"contract.v1.Thing\" }}, \
             steps: {{ id: \"raw\", {kind}: {{}} }} }} }};"
        ),
    );
}

#[when("the contract is compiled against the shipped files")]
fn when_compiled(world: &mut VocabularyWorld) {
    let mut compiler = Compiler::with_file_resolver(ShippedAndContract(world.source.clone()));
    world.outcome = Some(match compiler.open_file(CONTRACT) {
        Ok(_) => Ok(compiler.descriptor_pool()),
        Err(e) => Err(e.to_string()),
    });
}

#[when(
    expr = "a manifest for product {string} is compiled from the contract and served with its signature"
)]
fn when_manifest_served(world: &mut VocabularyWorld, product: String) {
    let pool = pool(world).clone();
    let thing = pool
        .get_message_by_name("contract.v1.Thing")
        .expect("message");
    let method = pool
        .get_service_by_name("contract.v1.Svc")
        .expect("service")
        .methods()
        .next()
        .expect("method");

    let trigger_ext = pool
        .get_extension_by_name("bones.v1.automation_trigger")
        .expect("automation_trigger");
    let mut trigger = new_message(&pool, "bones.v1.ManifestTrigger");
    trigger.set_field_by_name("event", Value::String(thing.full_name().to_owned()));
    trigger.set_field_by_name(
        "declaration",
        thing.options().get_extension(&trigger_ext).into_owned(),
    );

    let authz = method_option(world, "bones.v1.authz");
    let grants = authz
        .as_message()
        .expect("authz is a message")
        .get_field_by_name("requires")
        .expect("requires")
        .into_owned();
    let mut action = new_message(&pool, "bones.v1.ManifestAction");
    action.set_field_by_name("method", Value::String(method.full_name().to_owned()));
    action.set_field_by_name(
        "declaration",
        method_option(world, "bones.v1.automation_action"),
    );
    action.set_field_by_name("grants", grants);

    let templates = pool
        .get_service_by_name("contract.v1.Svc")
        .expect("service")
        .options()
        .get_extension(
            &pool
                .get_extension_by_name("bones.v1.automation_templates")
                .expect("automation_templates"),
        )
        .into_owned();

    let schema = DynamicMessage::decode(
        pool.get_message_by_name("google.protobuf.FileDescriptorSet")
            .expect("FileDescriptorSet"),
        pool.encode_to_vec().as_slice(),
    )
    .expect("schema decodes");

    let mut manifest = new_message(&pool, "bones.v1.AutomationManifest");
    manifest.set_field_by_name("format", Value::U32(1));
    manifest.set_field_by_name("product", Value::String(product));
    manifest.set_field_by_name("version", Value::String("1.0.0".to_owned()));
    manifest.set_field_by_name("triggers", Value::List(vec![Value::Message(trigger)]));
    manifest.set_field_by_name("actions", Value::List(vec![Value::Message(action)]));
    manifest.set_field_by_name("templates", templates);
    manifest.set_field_by_name("schema", Value::Message(schema));
    world.signed = manifest.encode_to_vec();

    let mut envelope = new_message(&pool, "bones.v1.SignedAutomationManifest");
    envelope.set_field_by_name("manifest", Value::Bytes(world.signed.clone().into()));
    envelope.set_field_by_name(
        "signature",
        Value::Bytes(
            b"detached signature over the manifest bytes"
                .to_vec()
                .into(),
        ),
    );
    let wire = envelope.encode_to_vec();

    let received = DynamicMessage::decode(
        pool.get_message_by_name("bones.v1.SignedAutomationManifest")
            .expect("SignedAutomationManifest"),
        wire.as_slice(),
    )
    .expect("envelope decodes");
    world.served = Some(received);
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

#[then(expr = "the method batches {string}")]
fn then_batch_of(world: &mut VocabularyWorld, expected: String) {
    let value = method_option(world, "bones.v1.batch_of");
    assert_eq!(value.as_str().expect("batch_of is a string"), expected);
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

#[then(expr = "the trigger label is {string}")]
fn then_trigger_label(world: &mut VocabularyWorld, expected: String) {
    assert_eq!(
        label_text(&message_option(world, "bones.v1.automation_trigger")),
        expected
    );
}

#[then(expr = "the action label is {string}")]
fn then_action_label(world: &mut VocabularyWorld, expected: String) {
    assert_eq!(label_text(&action(world)), expected);
}

#[then(expr = "the action effect is {string}")]
fn then_action_effect(world: &mut VocabularyWorld, expected: String) {
    let effect = action(world)
        .get_field_by_name("effect")
        .expect("effect")
        .into_owned();
    assert_eq!(
        enum_name(pool(world), "bones.v1.ActionEffect", &effect),
        expected
    );
}

#[then(expr = "the action's RPC requires exactly {string}")]
fn then_action_requires(world: &mut VocabularyWorld, expected: String) {
    let authz = method_option(world, "bones.v1.authz");
    let requires: Vec<String> = authz
        .as_message()
        .expect("authz is a message")
        .get_field_by_name("requires")
        .expect("requires")
        .as_list()
        .expect("requires is repeated")
        .iter()
        .map(|v| v.as_str().expect("scope").to_owned())
        .collect();
    assert_eq!(requires, [expected]);
}

#[then("the action declaration has no field naming a scope, grant or requirement")]
fn then_action_has_no_scope(world: &mut VocabularyWorld) {
    let declaration = pool(world)
        .get_message_by_name("bones.v1.AutomationAction")
        .expect("AutomationAction");
    for field in declaration.fields() {
        for banned in ["scope", "grant", "require"] {
            assert!(
                !field.name().contains(banned),
                "AutomationAction declares `{}`, a second authority",
                field.name()
            );
        }
    }
}

#[then(expr = "the action guardrail {string} is set")]
fn then_guardrail_set(world: &mut VocabularyWorld, flag: String) {
    let set = action(world)
        .get_field_by_name(&flag)
        .and_then(|v| v.as_bool())
        .unwrap_or_else(|| panic!("`{flag}` is not a flag"));
    assert!(set, "`{flag}` is not set");
}

#[then(expr = "the action guardrail {string} is not set")]
fn then_guardrail_unset(world: &mut VocabularyWorld, flag: String) {
    let set = action(world)
        .get_field_by_name(&flag)
        .and_then(|v| v.as_bool())
        .unwrap_or_else(|| panic!("`{flag}` is not a flag"));
    assert!(!set, "`{flag}` is set");
}

#[then(expr = "the action resolves search text in {string} to candidates in {string}")]
fn then_resolves(world: &mut VocabularyWorld, query: String, results: String) {
    let resolves = child(&action(world), "resolves");
    assert_eq!(text(&resolves, "query_field"), query);
    assert_eq!(text(&resolves, "results_field"), results);
    assert_eq!(text(&resolves, "value_field"), "id");
    assert_eq!(text(&resolves, "label_field"), "title");
}

#[then(expr = "the field classification reads back as {string}")]
fn then_classification(world: &mut VocabularyWorld, expected: String) {
    let pool = pool(world);
    let ext = pool
        .get_extension_by_name("bones.v1.classification")
        .expect("classification extension");
    let value = field_options(world).get_extension(&ext).into_owned();
    assert_eq!(
        enum_name(pool, "bones.v1.DataClassification", &value),
        expected
    );
}

#[then("the field carries no classification")]
fn then_unclassified(world: &mut VocabularyWorld) {
    let ext = pool(world)
        .get_extension_by_name("bones.v1.classification")
        .expect("classification extension");
    assert!(
        !field_options(world).has_extension(&ext),
        "the field carries a classification"
    );
}

#[then(expr = "no classified value of {string} has number 0")]
fn then_zero_is_unclassified(world: &mut VocabularyWorld, name: String) {
    let found = pool(world)
        .get_enum_by_name(&name)
        .unwrap_or_else(|| panic!("{name} is not declared"));
    let zero: Vec<_> = found
        .values()
        .filter(|v| v.number() == 0)
        .map(|v| v.name().to_owned())
        .collect();
    assert_eq!(zero, ["DATA_CLASSIFICATION_UNSPECIFIED"]);
}

#[then(expr = "the template {string} is triggered by {string}")]
fn then_template_trigger(world: &mut VocabularyWorld, name: String, event: String) {
    let document = child(&template(world, &name), "document");
    assert_eq!(text(&child(&document, "trigger"), "event"), event);
}

#[then(expr = "the first step of template {string} calls {string}")]
fn then_template_step(world: &mut VocabularyWorld, name: String, method: String) {
    let step = first_step_action(world, &name);
    assert_eq!(text(&child(&step, "action"), "method"), method);
}

#[then(expr = "the first step of template {string} binds {string} from the trigger field {string}")]
fn then_template_binding(world: &mut VocabularyWorld, name: String, field: String, source: String) {
    let step = first_step_action(world, &name);
    let binding = messages(&step, "inputs")
        .into_iter()
        .next()
        .expect("the step binds an input");
    assert_eq!(text(&binding, "field"), field);
    assert_eq!(text(&binding, "trigger_field"), source);
}

#[then("the served manifest bytes are the bytes that were signed")]
fn then_served_bytes(world: &mut VocabularyWorld) {
    let bytes = served(world)
        .get_field_by_name("manifest")
        .and_then(|v| v.as_bytes().cloned())
        .expect("manifest bytes");
    assert_eq!(bytes.as_ref(), world.signed.as_slice());
}

fn served_manifest(world: &VocabularyWorld) -> DynamicMessage {
    let bytes = served(world)
        .get_field_by_name("manifest")
        .and_then(|v| v.as_bytes().cloned())
        .expect("manifest bytes");
    DynamicMessage::decode(
        pool(world)
            .get_message_by_name("bones.v1.AutomationManifest")
            .expect("AutomationManifest"),
        bytes,
    )
    .expect("served manifest decodes")
}

#[then(expr = "the served manifest names product {string}")]
fn then_served_product(world: &mut VocabularyWorld, product: String) {
    assert_eq!(text(&served_manifest(world), "product"), product);
}

#[then(
    expr = "the served manifest carries the trigger {string}, the action {string} and the template {string}"
)]
fn then_served_surface(world: &mut VocabularyWorld, event: String, method: String, name: String) {
    let manifest = served_manifest(world);
    let events: Vec<String> = messages(&manifest, "triggers")
        .iter()
        .map(|t| text(t, "event"))
        .collect();
    let methods: Vec<String> = messages(&manifest, "actions")
        .iter()
        .map(|a| text(a, "method"))
        .collect();
    let names: Vec<String> = messages(&manifest, "templates")
        .iter()
        .map(|t| text(t, "name"))
        .collect();
    assert_eq!(events, [event]);
    assert_eq!(methods, [method]);
    assert_eq!(names, [name]);
}

#[then(expr = "the action {string} in the served manifest has the grant {string}")]
fn then_served_grant(world: &mut VocabularyWorld, method: String, grant: String) {
    let action = messages(&served_manifest(world), "actions")
        .into_iter()
        .find(|a| text(a, "method") == method)
        .unwrap_or_else(|| panic!("no action `{method}`"));
    let grants: Vec<String> = action
        .get_field_by_name("grants")
        .expect("grants")
        .as_list()
        .expect("grants is repeated")
        .iter()
        .map(|v| v.as_str().expect("scope").to_owned())
        .collect();
    assert_eq!(grants, [grant]);
}

#[then(expr = "the served manifest schema classifies {string} field {string} as {string}")]
fn then_served_classification(
    world: &mut VocabularyWorld,
    message: String,
    field: String,
    expected: String,
) {
    let schema = child(&served_manifest(world), "schema").encode_to_vec();
    let schema = DescriptorPool::decode(schema.as_slice()).expect("schema is a descriptor set");
    let ext = schema
        .get_extension_by_name("bones.v1.classification")
        .expect("the schema carries the classification vocabulary");
    let value = schema
        .get_message_by_name(&message)
        .unwrap_or_else(|| panic!("the schema has no `{message}`"))
        .get_field_by_name(&field)
        .unwrap_or_else(|| panic!("`{message}` has no field `{field}`"))
        .options()
        .get_extension(&ext)
        .into_owned();
    assert_eq!(
        enum_name(&schema, "bones.v1.DataClassification", &value),
        expected
    );
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
