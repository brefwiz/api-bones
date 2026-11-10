// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! The steps of `../../../features/delegation_templates.feature`.
//!
//! Every scenario compiles one provider contract against the shipped
//! `bones/v1` files, reads it back through descriptor reflection and runs the
//! check `api-bones-protos` ships over the resulting pool.

use cucumber::{given, then, when};
use prost_reflect::{DescriptorPool, DynamicMessage, Kind, ReflectMessage, Value};

use super::shipped_resolver::{CONTRACT, compiler};
use crate::world::DelegationTemplatesWorld;

const TEMPLATES: &str = "bones.v1.delegation_templates";
const SERVICE: &str = "contract.v1.ProviderService";

/// The Unicode format characters a refusal must not carry raw, as inclusive
/// ranges: the ones a scenario can write into a name or value.
const FORMAT_RANGES: [(char, char); 6] = [
    ('\u{ad}', '\u{ad}'),
    ('\u{200b}', '\u{200f}'),
    ('\u{202a}', '\u{202e}'),
    ('\u{2060}', '\u{2064}'),
    ('\u{2066}', '\u{206f}'),
    ('\u{feff}', '\u{feff}'),
];

impl DelegationTemplatesWorld {
    fn source(&self) -> String {
        let singular = self
            .singular_capability
            .iter()
            .map(|capability| format!("  option (bones.v1.capability) = \"{capability}\";"));
        let capabilities = self.capabilities.iter().map(|capability| {
            format!("  option (bones.v1.provided_capabilities) = \"{capability}\";")
        });
        let broker_scopes = self.broker_scopes.iter().map(|(capability, scope)| {
            format!(
                "  option (bones.v1.provided_broker_scopes) = {{ capability: \"{capability}\" scope: \"{scope}\" }};"
            )
        });
        let groups = self.groups.iter().map(|(group, scope)| {
            let scopes: String = scope
                .iter()
                .map(|scope| format!(" scopes: \"{scope}\""))
                .collect();
            format!(
                "  option (bones.v1.capability_scopes) = {{ capability: \"{group}\"{scopes} }};"
            )
        });
        let dimensions = self
            .dimensions
            .iter()
            .map(|dimension| format!("  option (bones.v1.attributes) = {{ {dimension} }};"));
        let options: Vec<String> = singular
            .chain(capabilities)
            .chain(broker_scopes)
            .chain(groups)
            .chain(dimensions)
            .chain(template_options(&self.templates))
            .collect();
        let second = if self.second_rpc_scopes.is_empty() && self.second_templates.is_empty() {
            String::new()
        } else {
            service_source(
                "OtherService",
                &template_options(&self.second_templates).collect::<Vec<_>>(),
                &rpcs_source(&self.second_rpc_scopes, &[]),
            )
        };
        let templates_import = if self.without_templates_import {
            ""
        } else {
            "import \"bones/v1/delegation.proto\";\n"
        };
        format!(
            "syntax = \"proto3\";\npackage contract.v1;\n\
             import \"bones/v1/annotations.proto\";\n\
             {templates_import}\
             message Req {{}}\nmessage Res {{}}\n{}{second}",
            service_source(
                "ProviderService",
                &options,
                &rpcs_source(&self.rpc_scopes, &self.public_scopes)
            ),
        )
    }
}

/// Whether a refusal must not carry `c` raw: a control or format character.
fn is_raw(c: char) -> bool {
    c.is_control()
        || FORMAT_RANGES
            .iter()
            .any(|&(first, last)| (first..=last).contains(&c))
}

/// The templates the service declares, as read back from its options.
fn declared_templates(pool: Option<&DescriptorPool>) -> Vec<DynamicMessage> {
    let pool = pool.expect("the provider contract was not compiled");
    let extension = pool
        .get_extension_by_name(TEMPLATES)
        .unwrap_or_else(|| panic!("{TEMPLATES} is not declared"));
    let options = pool
        .get_service_by_name(SERVICE)
        .expect("the provider service")
        .options();
    let value = options.get_extension(&extension);
    value
        .as_list()
        .expect("a repeated option")
        .iter()
        .map(|template| template.as_message().expect("a message").clone())
        .collect()
}

fn declared_grant(pool: Option<&DescriptorPool>, index: usize) -> DynamicMessage {
    let templates = declared_templates(pool);
    let grants = field(templates.last().expect("a template"), "grants");
    grants
        .as_list()
        .expect("a repeated field")
        .get(index)
        .unwrap_or_else(|| panic!("the template has no grant {index}"))
        .as_message()
        .expect("a message")
        .clone()
}

/// The label text of the template `name`.
fn template_label(pool: Option<&DescriptorPool>, name: &str) -> String {
    let templates = declared_templates(pool);
    let template = templates
        .iter()
        .find(|template| text(template, "name") == name)
        .unwrap_or_else(|| panic!("the service ships no template {name}"));
    let label = field(template, "label");
    text(label.as_message().expect("a label"), "text")
}

/// The values of the selector on `dimension` in grant `index`, comma-joined.
fn selector_values(pool: Option<&DescriptorPool>, index: usize, dimension: &str) -> String {
    let grant = declared_grant(pool, index);
    let selectors = field(&grant, "selectors");
    let selector = selectors
        .as_list()
        .expect("a repeated field")
        .iter()
        .filter_map(Value::as_message)
        .find(|selector| text(selector, "dimension") == dimension)
        .unwrap_or_else(|| panic!("the grant has no selector on {dimension}"));
    let values: Vec<String> = field(selector, "values")
        .as_list()
        .expect("a repeated field")
        .iter()
        .map(|value| value.as_str().expect("a string").to_owned())
        .collect();
    values.join(", ")
}

/// The name of the approval mode of grant `index`.
fn approval_mode(pool: Option<&DescriptorPool>, index: usize) -> String {
    let grant = declared_grant(pool, index);
    let Kind::Enum(approval) = grant
        .descriptor()
        .get_field_by_name("approval_mode")
        .expect("approval_mode")
        .kind()
    else {
        panic!("approval_mode is not an enum");
    };
    let number = field(&grant, "approval_mode")
        .as_enum_number()
        .expect("an enum value");
    approval
        .get_value(number)
        .expect("a declared value")
        .name()
        .to_owned()
}

/// The budget count and window of grant `index`.
fn budget_of(pool: Option<&DescriptorPool>, index: usize) -> (Option<u32>, Option<u32>) {
    let grant = declared_grant(pool, index);
    let budget = field(&grant, "budget");
    let budget = budget.as_message().expect("a budget");
    (
        field(budget, "count").as_u32(),
        field(budget, "window_seconds").as_u32(),
    )
}

/// The field names of `message`, comma-joined.
fn grammar_fields(pool: Option<&DescriptorPool>, message: &str) -> String {
    let message = pool
        .expect("the provider contract was not compiled")
        .get_message_by_name(message)
        .unwrap_or_else(|| panic!("{message} is not declared"));
    let fields: Vec<String> = message.fields().map(|f| f.name().to_owned()).collect();
    fields.join(", ")
}

/// The number and the containing message of the extension `name`.
fn declared_option(pool: Option<&DescriptorPool>, name: &str) -> (u32, String) {
    let extension = pool
        .expect("the provider contract was not compiled")
        .get_extension_by_name(name)
        .unwrap_or_else(|| panic!("{name} is not declared"));
    (
        extension.number(),
        extension.containing_message().full_name().to_owned(),
    )
}

fn field(message: &DynamicMessage, name: &str) -> Value {
    message
        .get_field_by_name(name)
        .unwrap_or_else(|| panic!("{} has no field {name}", message.descriptor().full_name()))
        .into_owned()
}

fn text(message: &DynamicMessage, name: &str) -> String {
    field(message, name)
        .as_str()
        .unwrap_or_else(|| panic!("{name} is not a string"))
        .to_owned()
}

fn quoted(value: &str) -> String {
    format!("\"{value}\"")
}

/// One authenticated RPC per scope in `scopes` and one public RPC per scope in
/// `public`.
fn rpcs_source(scopes: &[String], public: &[String]) -> Vec<String> {
    let authenticated = scopes.iter().enumerate().map(|(index, scope)| {
        format!(
            "  rpc Op{index}(Req) returns (Res) {{\n    \
             option (bones.v1.authz) = {{ kind: AUTHZ_KIND_AUTHENTICATED requires: \"{scope}\" }};\n  }}"
        )
    });
    let open = public.iter().enumerate().map(|(index, scope)| {
        format!(
            "  rpc Public{index}(Req) returns (Res) {{\n    \
             option (bones.v1.authz) = {{ kind: AUTHZ_KIND_PUBLIC reason: \"open\" requires: \"{scope}\" }};\n  }}"
        )
    });
    authenticated.chain(open).collect()
}

fn service_source(name: &str, options: &[String], rpcs: &[String]) -> String {
    format!(
        "service {name} {{\n{}\n{}\n}}\n",
        options.join("\n"),
        rpcs.join("\n")
    )
}

fn template_options(templates: &[String]) -> impl Iterator<Item = String> + '_ {
    templates
        .iter()
        .map(|template| format!("  option (bones.v1.delegation_templates) = {{ {template} }};"))
}

/// One template literal; its name, label and grants may carry the names a table
/// cell stands in for a character.
fn template_source(name: &str, label: &str, grants: &[&str]) -> String {
    let grants: String = grants
        .iter()
        .map(|grant| format!(" grants: {{ {} }}", decode_named_characters(grant)))
        .collect();
    format!(
        "name: {} label: {{ text: {} }}{grants}",
        quoted(&decode_named_characters(name)),
        quoted(&decode_named_characters(label))
    )
}

/// The text with the names a table cell stands in for a character replaced by
/// that character's escape in a quoted text-format string.
fn decode_named_characters(grant: &str) -> String {
    grant
        .replace("<newline>", "\\n")
        .replace("<quote>", "\\\"")
        .replace("<soft-hyphen>", "\\302\\255")
        .replace("<escape>", "\\033")
        .replace("<bidi-override>", "\\342\\200\\256")
        .replace("<space>", " ")
        .replace("<empty>", "")
}

#[given(expr = "a provider with an RPC requiring the scope {string}")]
fn given_rpc_scope(world: &mut DelegationTemplatesWorld, scope: String) {
    world.rpc_scopes.push(scope);
}

#[given(expr = "a provider with a public RPC listing the scope {string}")]
fn given_public_rpc_scope(world: &mut DelegationTemplatesWorld, scope: String) {
    world.public_scopes.push(scope);
}

#[given(expr = "the provider provides the capability {string}")]
fn given_capability(world: &mut DelegationTemplatesWorld, capability: String) {
    world.capabilities.push(capability);
}

#[given(expr = "the provider declares the group {string} conferring the scope {string}")]
fn given_group(world: &mut DelegationTemplatesWorld, group: String, scope: String) {
    world.groups.push((group, vec![scope]));
}

#[given(expr = "the provider declares the group {string} conferring the scopes {string}")]
fn given_group_scopes(world: &mut DelegationTemplatesWorld, group: String, scopes: String) {
    let listed = scopes.split(',').map(|scope| scope.trim().to_owned());
    world.groups.push((group, listed.collect()));
}

#[given(
    expr = "the provider declares the dimension {string} matched by segment prefix on {string}"
)]
fn given_prefix_dimension(world: &mut DelegationTemplatesWorld, name: String, separator: String) {
    world.dimensions.push(format!(
        "name: {} type: ATTRIBUTE_TYPE_STRING polarity: ATTRIBUTE_POLARITY_NARROWABLE \
         matching: ATTRIBUTE_MATCHING_SEGMENT_PREFIX separator: {}",
        quoted(&name),
        quoted(&separator)
    ));
}

#[given(expr = "the provider declares the dimension {string} matched exactly")]
fn given_exact_dimension(world: &mut DelegationTemplatesWorld, name: String) {
    world.dimensions.push(format!(
        "name: {} type: ATTRIBUTE_TYPE_STRING polarity: ATTRIBUTE_POLARITY_NARROWABLE \
         matching: ATTRIBUTE_MATCHING_EXACT",
        quoted(&name)
    ));
}

#[given(expr = "the provider declares the dimension {string} matched exactly over UUIDs")]
fn given_uuid_dimension(world: &mut DelegationTemplatesWorld, name: String) {
    world.dimensions.push(format!(
        "name: {} type: ATTRIBUTE_TYPE_UUID polarity: ATTRIBUTE_POLARITY_NARROWABLE \
         matching: ATTRIBUTE_MATCHING_EXACT",
        quoted(&name)
    ));
}

#[given(
    expr = "the provider ships the template {string} labelled {string} with the grant {string}"
)]
fn given_template(
    world: &mut DelegationTemplatesWorld,
    name: String,
    label: String,
    grant: String,
) {
    world
        .templates
        .push(template_source(&name, &label, &[grant.as_str()]));
}

#[given(expr = "the provider ships the template {string} labelled {string} with no grant")]
fn given_template_without_grant(world: &mut DelegationTemplatesWorld, name: String, label: String) {
    world.templates.push(template_source(&name, &label, &[]));
}

#[given(
    expr = "the provider ships the template {string} labelled {string} with the grants {string} and {string}"
)]
fn given_template_with_two_grants(
    world: &mut DelegationTemplatesWorld,
    name: String,
    label: String,
    first: String,
    second: String,
) {
    world.templates.push(template_source(
        &name,
        &label,
        &[first.as_str(), second.as_str()],
    ));
}

#[given("the provider contract imports no delegation templates")]
fn given_no_templates_import(world: &mut DelegationTemplatesWorld) {
    world.without_templates_import = true;
}

#[given(expr = "the provider provides the capability {string} through the singular option")]
fn given_singular_capability(world: &mut DelegationTemplatesWorld, capability: String) {
    world.singular_capability = Some(capability);
}

#[given(expr = "the provider provides the broker scope {string} of the capability {string}")]
fn given_broker_scope(world: &mut DelegationTemplatesWorld, scope: String, capability: String) {
    world.broker_scopes.push((capability, scope));
}

#[given(expr = "the provider declares the group {string} conferring no scope")]
fn given_scopeless_group(world: &mut DelegationTemplatesWorld, group: String) {
    world.groups.push((group, Vec::new()));
}

#[given(expr = "a second service with an RPC requiring the scope {string}")]
fn given_second_service_scope(world: &mut DelegationTemplatesWorld, scope: String) {
    world.second_rpc_scopes.push(scope);
}

#[given(
    expr = "the second service ships the template {string} labelled {string} with the grant {string}"
)]
fn given_second_service_template(
    world: &mut DelegationTemplatesWorld,
    name: String,
    label: String,
    grant: String,
) {
    world
        .second_templates
        .push(template_source(&name, &label, &[grant.as_str()]));
}

#[when("the provider contract is validated")]
fn when_validated(world: &mut DelegationTemplatesWorld) {
    let mut compiler = compiler(world.source(), api_bones_protos::files());
    compiler
        .open_file(CONTRACT)
        .unwrap_or_else(|e| panic!("contract failed to compile: {e}"));
    let pool = compiler.descriptor_pool();
    world.outcome = Some(api_bones_protos::delegation_templates::check_delegation_templates(&pool));
    world.pool = Some(pool);
}

#[then(expr = "the service declares {int} delegation template(s)")]
fn then_count(world: &mut DelegationTemplatesWorld, expected: usize) {
    assert_eq!(declared_templates(world.pool.as_ref()).len(), expected);
}

#[then(expr = "the template {string} reads back labelled {string}")]
fn then_template_label(world: &mut DelegationTemplatesWorld, name: String, label: String) {
    assert_eq!(template_label(world.pool.as_ref(), &name), label);
}

#[then(expr = "its grant {int} reads back the scope {string}")]
fn then_grant_scope(world: &mut DelegationTemplatesWorld, index: usize, expected: String) {
    assert_eq!(
        text(&declared_grant(world.pool.as_ref(), index), "scope"),
        expected
    );
}

#[then(expr = "its grant {int} reads back the group {string}")]
fn then_grant_group(world: &mut DelegationTemplatesWorld, index: usize, expected: String) {
    assert_eq!(
        text(&declared_grant(world.pool.as_ref(), index), "group"),
        expected
    );
}

#[then(expr = "its grant {int} reads back the selector {string} with the values {string}")]
fn then_grant_selector(
    world: &mut DelegationTemplatesWorld,
    index: usize,
    dimension: String,
    expected: String,
) {
    assert_eq!(
        selector_values(world.pool.as_ref(), index, &dimension),
        expected
    );
}

#[then(expr = "its grant {int} reads back the approval mode {string}")]
fn then_grant_approval(world: &mut DelegationTemplatesWorld, index: usize, expected: String) {
    assert_eq!(approval_mode(world.pool.as_ref(), index), expected);
}

#[then(expr = "its grant {int} reads back the budget {int} per {int} seconds")]
fn then_grant_budget(world: &mut DelegationTemplatesWorld, index: usize, count: u32, window: u32) {
    assert_eq!(
        budget_of(world.pool.as_ref(), index),
        (Some(count), Some(window))
    );
}

#[then("the delegation templates are accepted")]
fn then_accepted(world: &mut DelegationTemplatesWorld) {
    let refusals = world
        .outcome
        .as_ref()
        .expect("the provider contract was not validated")
        .as_ref()
        .err()
        .map(ToString::to_string)
        .unwrap_or_default();
    assert_eq!(refusals, "", "the provider's templates were refused");
}

#[then(expr = "the delegation templates are refused with {string}")]
fn then_refused(world: &mut DelegationTemplatesWorld, expected: String) {
    let refusal = world
        .outcome
        .as_ref()
        .expect("the provider contract was not validated")
        .as_ref()
        .expect_err("the provider's templates were accepted");
    assert!(
        refusal.to_string().contains(&expected),
        "refusals `{refusal}` do not name `{expected}`"
    );
}

#[then(expr = "the delegation templates are refused {int} time(s)")]
fn then_refusal_count(world: &mut DelegationTemplatesWorld, expected: usize) {
    let refusal = world
        .outcome
        .as_ref()
        .expect("the provider contract was not validated")
        .as_ref()
        .expect_err("the provider's templates were accepted");
    assert_eq!(refusal.refusals.len(), expected);
}

#[then(expr = "the refusal {string} is reported before {string}")]
fn then_refusal_order(world: &mut DelegationTemplatesWorld, earlier: String, later: String) {
    let refusal = world
        .outcome
        .as_ref()
        .expect("the provider contract was not validated")
        .as_ref()
        .expect_err("the provider's templates were accepted");
    let reasons = refusal.to_string();
    assert!(
        reasons.find(&earlier).is_some(),
        "refusals `{reasons}` do not name `{earlier}`"
    );
    assert!(
        reasons.find(&later).is_some(),
        "refusals `{reasons}` do not name `{later}`"
    );
    assert!(
        reasons.find(&earlier) < reasons.find(&later),
        "refusals `{reasons}` do not report `{earlier}` before `{later}`"
    );
}

#[then("the refusal shows no control or format character")]
fn then_refusal_shows_no_raw_character(world: &mut DelegationTemplatesWorld) {
    let refusal = world
        .outcome
        .as_ref()
        .expect("the provider contract was not validated")
        .as_ref()
        .expect_err("the provider's templates were accepted");
    assert!(
        refusal
            .refusals
            .iter()
            .all(|each| !each.to_string().chars().any(is_raw)),
        "a refusal in `{}` shows a raw control or format character",
        refusal.to_string().escape_debug()
    );
}

#[then(expr = "the grant grammar message {string} declares exactly the fields {string}")]
fn then_grammar_fields(world: &mut DelegationTemplatesWorld, message: String, expected: String) {
    assert_eq!(grammar_fields(world.pool.as_ref(), &message), expected);
}

#[then(expr = "the service option {string} is declared with the number {int}")]
fn then_option_number(world: &mut DelegationTemplatesWorld, name: String, number: u32) {
    assert_eq!(
        declared_option(world.pool.as_ref(), &name),
        (number, "google.protobuf.ServiceOptions".to_owned())
    );
}
