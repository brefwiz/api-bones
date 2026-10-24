// SPDX-License-Identifier: MIT
//! The reference compiler: a product's descriptor set to its serialized
//! `AutomationManifest`.

use core::fmt;
use std::collections::{BTreeMap, BTreeSet};

use prost_reflect::{
    DescriptorPool, DynamicMessage, ExtensionDescriptor, FieldDescriptor, Kind, MessageDescriptor,
    MethodDescriptor, Value, prost::Message,
};

use crate::digest::declaration_digest;
use crate::manifest::MANIFEST_FORMAT;
use crate::pool::{shipped, transcode};

/// The enum number of `AUTHZ_KIND_AUTHENTICATED`.
const AUTHZ_KIND_AUTHENTICATED: i32 = 4;
/// The enum number of `ACTION_EFFECT_PURE`.
const EFFECT_PURE: i32 = 1;
/// The enum number of `IDEMPOTENCY_MODE_KEYED`.
const IDEMPOTENCY_KEYED: i32 = 2;
/// Untyped payloads: a field of one of these types makes a generic dispatcher.
const UNTYPED: [&str; 3] = [
    "google.protobuf.Any",
    "google.protobuf.Struct",
    "google.protobuf.Value",
];

/// What a release compiles from.
#[derive(Debug, Clone, Copy)]
pub struct CompileInput<'a> {
    /// A serialized `FileDescriptorSet` holding every declaring file and every
    /// file they import, extension options included (`protoc
    /// --include_imports`, or `buf build`).
    pub descriptor_set: &'a [u8],
    /// `(import path, source bytes)` of the files of the product's repository
    /// at `commit`. Files the build resolves from outside the repository are
    /// not passed; files of the set that are not listed here stay out of the
    /// declaration digest.
    pub sources: &'a [(String, Vec<u8>)],
    /// The service segment of the SVID the product's workloads present.
    pub product: &'a str,
    /// The released version, as tagged, without a leading `v`.
    pub version: &'a str,
    /// The full lowercase hex commit SHA the release was built from.
    pub commit: &'a str,
}

/// The rule a refusal applies. `D1`-`D6` are the document rules of the
/// manifest contract; `T1`, `A1`-`A7` and `C1` the declaration rules of
/// `bones/v1/annotations.proto`; `U1` an unset closed choice; `M1` a template
/// that is not shaped as the contract requires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Rule {
    D1,
    D2,
    D3,
    D4,
    D5,
    D6,
    T1,
    A1,
    A2,
    A3,
    A4,
    A5,
    A6,
    A7,
    C1,
    U1,
    M1,
}

impl Rule {
    /// The rule's name as the contract writes it.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::D1 => "D1",
            Self::D2 => "D2",
            Self::D3 => "D3",
            Self::D4 => "D4",
            Self::D5 => "D5",
            Self::D6 => "D6",
            Self::T1 => "T1",
            Self::A1 => "A1",
            Self::A2 => "A2",
            Self::A3 => "A3",
            Self::A4 => "A4",
            Self::A5 => "A5",
            Self::A6 => "A6",
            Self::A7 => "A7",
            Self::C1 => "C1",
            Self::U1 => "U1",
            Self::M1 => "M1",
        }
    }
}

/// One declaration or template the compiler refuses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub rule: Rule,
    /// Where: `trigger <event>`, `action <method>` or `template <name> ...`.
    pub site: String,
    pub detail: String,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at {}: {}",
            self.rule.as_str(),
            self.site,
            self.detail
        )
    }
}

/// Why nothing was compiled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileError {
    /// The descriptor set does not decode into a complete pool.
    Descriptors(String),
    /// The set declares no trigger, action or template.
    NoDeclarations,
    /// `product`, `version` or `commit` is not well formed.
    Input(String),
    /// A file that carries a declaration has no entry in `sources`.
    MissingSource(String),
    /// Every refusal found, in a stable order.
    Refused(Vec<Refusal>),
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Descriptors(why) => write!(f, "descriptor set is unusable: {why}"),
            Self::NoDeclarations => f.write_str("no trigger, action or template is declared"),
            Self::Input(why) => write!(f, "release input is invalid: {why}"),
            Self::MissingSource(file) => {
                write!(f, "declaring file {file} has no source in the repository")
            }
            Self::Refused(refusals) => write_refusals(f, refusals),
        }
    }
}

/// Write `refusals` separated by `"; "`.
pub(crate) fn write_refusals<T: fmt::Display>(
    f: &mut fmt::Formatter<'_>,
    refusals: &[T],
) -> fmt::Result {
    for (i, refusal) in refusals.iter().enumerate() {
        if i > 0 {
            f.write_str("; ")?;
        }
        fmt::Display::fmt(refusal, f)?;
    }
    Ok(())
}

impl std::error::Error for CompileError {}

/// Compile the manifest of `input` and return its serialized bytes.
///
/// Equal input gives equal bytes: every repeated field is ordered as the
/// contract states, and references to the product itself are resolved.
///
/// # Errors
///
/// [`CompileError`] listing every rule the input breaks.
pub fn compile(input: &CompileInput<'_>) -> Result<Vec<u8>, CompileError> {
    check_release(input)?;
    let pool = DescriptorPool::decode(input.descriptor_set)
        .map_err(|e| CompileError::Descriptors(e.to_string()))?;
    let Some(ext) = Extensions::find(&pool) else {
        return Err(CompileError::NoDeclarations);
    };
    let surface = Surface::read(&pool, &ext);
    if surface.triggers.is_empty() && surface.actions.is_empty() && surface.templates.is_empty() {
        return Err(CompileError::NoDeclarations);
    }
    let own_files: BTreeSet<&str> = input.sources.iter().map(|(p, _)| p.as_str()).collect();
    let mut checker = Checker::new(input.product, &surface, &ext, own_files);
    checker.declarations();
    let mut templates = checker.templates();
    if !checker.refusals.is_empty() {
        return Err(CompileError::Refused(checker.refusals));
    }

    for file in &surface.declaring_files {
        if !input.sources.iter().any(|(path, _)| path == file) {
            return Err(CompileError::MissingSource(file.clone()));
        }
    }
    let (schema, schema_files) = schema(input.descriptor_set, &pool, &surface.declaring_files);
    let in_repo: Vec<(String, Vec<u8>)> = input
        .sources
        .iter()
        .filter(|(path, _)| schema_files.contains(path))
        .cloned()
        .collect();

    templates.sort_by_cached_key(|t| string(t, "name"));
    let mut manifest = DynamicMessage::new(shipped("bones.v1.AutomationManifest"));
    manifest.set_field_by_name("format", Value::U32(MANIFEST_FORMAT));
    manifest.set_field_by_name("product", Value::String(input.product.to_owned()));
    manifest.set_field_by_name("version", Value::String(input.version.to_owned()));
    manifest.set_field_by_name("commit", Value::String(input.commit.to_owned()));
    manifest.set_field_by_name(
        "declaration_digest",
        Value::String(declaration_digest(&in_repo)),
    );
    manifest.set_field_by_name("triggers", messages(surface.manifest_triggers()));
    manifest.set_field_by_name("actions", messages(surface.manifest_actions()));
    manifest.set_field_by_name("templates", messages(templates));
    manifest.set_field_by_name("schema", Value::Message(schema));
    Ok(manifest.encode_to_vec())
}

fn messages(items: Vec<DynamicMessage>) -> Value {
    Value::List(items.into_iter().map(Value::Message).collect())
}

fn check_release(input: &CompileInput<'_>) -> Result<(), CompileError> {
    let bad = |why: &str| Err(CompileError::Input(why.to_owned()));
    if input.product.is_empty() {
        return bad("product is empty");
    }
    if input.version.is_empty() || input.version.starts_with('v') {
        return bad("version must be non-empty and carry no leading v");
    }
    let hex = input.commit.len() == 40
        && input
            .commit
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if !hex {
        return bad("commit must be a full lowercase hex SHA");
    }
    Ok(())
}

// ---- reading dynamic messages ----

fn string(message: &DynamicMessage, field: &str) -> String {
    message
        .get_field_by_name(field)
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn sub(message: &DynamicMessage, field: &str) -> Option<DynamicMessage> {
    if !message.has_field_by_name(field) {
        return None;
    }
    message.get_field_by_name(field)?.as_message().cloned()
}

fn subs(message: &DynamicMessage, field: &str) -> Vec<DynamicMessage> {
    message
        .get_field_by_name(field)
        .and_then(|v| {
            v.as_list()
                .map(|l| l.iter().filter_map(|x| x.as_message().cloned()).collect())
        })
        .unwrap_or_default()
}

/// The populated member of `oneof` in `message`: its field name and value.
fn case(message: &DynamicMessage, oneof: &str) -> Option<(String, Value)> {
    message.fields().find_map(|(field, value)| {
        let member = field.containing_oneof()?;
        (member.name() == oneof).then(|| (field.name().to_owned(), value.clone()))
    })
}

fn enum_number(message: &DynamicMessage, field: &str) -> i32 {
    message
        .get_field_by_name(field)
        .and_then(|v| v.as_enum_number())
        .unwrap_or_default()
}

fn strings(message: &DynamicMessage, field: &str) -> Vec<String> {
    message
        .get_field_by_name(field)
        .and_then(|v| {
            v.as_list().map(|l| {
                l.iter()
                    .filter_map(|s| s.as_str().map(str::to_owned))
                    .collect()
            })
        })
        .unwrap_or_default()
}

fn has_label(message: &DynamicMessage) -> bool {
    sub(message, "label").is_some_and(|l| !string(&l, "text").is_empty())
}

fn msg_mut<'a>(message: &'a mut DynamicMessage, field: &str) -> Option<&'a mut DynamicMessage> {
    if !message.has_field_by_name(field) {
        return None;
    }
    message.get_field_by_name_mut(field)?.as_message_mut()
}

fn list_mut<'a>(
    message: &'a mut DynamicMessage,
    field: &str,
) -> impl Iterator<Item = &'a mut DynamicMessage> {
    message
        .get_field_by_name_mut(field)
        .and_then(Value::as_list_mut)
        .into_iter()
        .flatten()
        .filter_map(Value::as_message_mut)
}

// ---- the declared surface ----

struct Extensions {
    trigger: ExtensionDescriptor,
    action: ExtensionDescriptor,
    templates: ExtensionDescriptor,
    authz: ExtensionDescriptor,
    classification: ExtensionDescriptor,
}

impl Extensions {
    fn find(pool: &DescriptorPool) -> Option<Self> {
        Some(Self {
            trigger: pool.get_extension_by_name("bones.v1.automation_trigger")?,
            action: pool.get_extension_by_name("bones.v1.automation_action")?,
            templates: pool.get_extension_by_name("bones.v1.automation_templates")?,
            authz: pool.get_extension_by_name("bones.v1.authz")?,
            classification: pool.get_extension_by_name("bones.v1.classification")?,
        })
    }
}

struct Trigger {
    event: String,
    message: MessageDescriptor,
    declaration: DynamicMessage,
}

struct Action {
    method: String,
    descriptor: MethodDescriptor,
    declaration: DynamicMessage,
    authz: DynamicMessage,
}

impl Action {
    fn flag(&self, name: &str) -> bool {
        self.declaration
            .get_field_by_name(name)
            .and_then(|v| v.as_bool())
            .unwrap_or_default()
    }
}

struct Surface {
    triggers: BTreeMap<String, Trigger>,
    actions: BTreeMap<String, Action>,
    templates: Vec<DynamicMessage>,
    declaring_files: BTreeSet<String>,
}

impl Surface {
    fn read(pool: &DescriptorPool, ext: &Extensions) -> Self {
        let mut surface = Self {
            triggers: BTreeMap::new(),
            actions: BTreeMap::new(),
            templates: Vec::new(),
            declaring_files: BTreeSet::new(),
        };
        for message in pool.all_messages() {
            let options = message.options();
            if !options.has_extension(&ext.trigger) {
                continue;
            }
            let value = options.get_extension(&ext.trigger);
            let declaration = value.as_message().expect("a trigger is a message");
            let event = message.full_name().to_owned();
            surface
                .declaring_files
                .insert(message.parent_file().name().to_owned());
            surface.triggers.insert(
                event.clone(),
                Trigger {
                    event,
                    declaration: transcode(declaration, "bones.v1.AutomationTrigger"),
                    message,
                },
            );
        }
        for service in pool.services() {
            surface.read_service(&service, ext);
        }
        surface
    }

    fn read_service(&mut self, service: &prost_reflect::ServiceDescriptor, ext: &Extensions) {
        let options = service.options();
        if options.has_extension(&ext.templates) {
            let found = options.get_extension(&ext.templates);
            for template in found.as_list().unwrap_or_default() {
                let template = template.as_message().expect("a template is a message");
                self.declaring_files
                    .insert(service.parent_file().name().to_owned());
                self.templates
                    .push(transcode(template, "bones.v1.AutomationTemplate"));
            }
        }
        for method in service.methods() {
            let options = method.options();
            if !options.has_extension(&ext.action) {
                continue;
            }
            let declaration = options.get_extension(&ext.action);
            let authz = if options.has_extension(&ext.authz) {
                options.get_extension(&ext.authz).as_message().cloned()
            } else {
                None
            };
            self.declaring_files
                .insert(method.parent_file().name().to_owned());
            self.actions.insert(
                method.full_name().to_owned(),
                Action {
                    method: method.full_name().to_owned(),
                    declaration: transcode(
                        declaration.as_message().expect("an action is a message"),
                        "bones.v1.AutomationAction",
                    ),
                    authz: authz
                        .unwrap_or_else(|| DynamicMessage::new(shipped("bones.v1.AuthzRule"))),
                    descriptor: method,
                },
            );
        }
    }

    fn manifest_triggers(&self) -> Vec<DynamicMessage> {
        self.triggers
            .values()
            .map(|t| {
                let mut m = DynamicMessage::new(shipped("bones.v1.ManifestTrigger"));
                m.set_field_by_name("event", Value::String(t.event.clone()));
                m.set_field_by_name("declaration", Value::Message(t.declaration.clone()));
                m
            })
            .collect()
    }

    fn manifest_actions(&self) -> Vec<DynamicMessage> {
        self.actions
            .values()
            .map(|a| {
                let mut m = DynamicMessage::new(shipped("bones.v1.ManifestAction"));
                m.set_field_by_name("method", Value::String(a.method.clone()));
                m.set_field_by_name("declaration", Value::Message(a.declaration.clone()));
                m.set_field_by_name(
                    "grants",
                    Value::List(requires(&a.authz).into_iter().map(Value::String).collect()),
                );
                m
            })
            .collect()
    }
}

fn requires(authz: &DynamicMessage) -> Vec<String> {
    strings(authz, "requires")
}

// ---- the schema ----

/// The `FileDescriptorSet` of the declaring files and everything they import,
/// dependencies before dependents and otherwise by name, without source info;
/// and the names of its files.
fn schema(
    set: &[u8],
    pool: &DescriptorPool,
    roots: &BTreeSet<String>,
) -> (DynamicMessage, BTreeSet<String>) {
    let mut deps: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut pending: Vec<String> = roots.iter().cloned().collect();
    while let Some(name) = pending.pop() {
        if deps.contains_key(&name) {
            continue;
        }
        let file = pool
            .files()
            .find(|f| f.name() == name)
            .expect("a declaring or imported file is in its own pool");
        let imports: Vec<String> = file.dependencies().map(|d| d.name().to_owned()).collect();
        pending.extend(imports.iter().cloned());
        deps.insert(name, imports);
    }

    let mut ordered: Vec<String> = Vec::new();
    while ordered.len() < deps.len() {
        let next = deps
            .iter()
            .filter(|(name, _)| !ordered.contains(name))
            .find(|(_, imports)| imports.iter().all(|i| ordered.contains(i)))
            .map(|(name, _)| name.clone())
            .expect("an import graph is acyclic");
        ordered.push(next);
    }

    let source = DynamicMessage::decode(shipped("google.protobuf.FileDescriptorSet"), set)
        .expect("a set that built a pool decodes as a set");
    let mut files = subs(&source, "file");
    files.retain(|f| deps.contains_key(&string(f, "name")));
    for file in &mut files {
        file.clear_field_by_name("source_code_info");
    }
    files.sort_by_cached_key(|f| {
        let name = string(f, "name");
        ordered.iter().position(|o| *o == name)
    });
    let mut out = DynamicMessage::new(shipped("google.protobuf.FileDescriptorSet"));
    out.set_field_by_name("file", messages(files));
    (out, deps.into_keys().collect())
}

// ---- the rules ----

#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    Text,
    Integer,
    Boolean,
}

/// What a step may read: its document's trigger, parameters and earlier steps.
struct Scope {
    trigger: Option<MessageDescriptor>,
    params: BTreeMap<String, Option<Shape>>,
    /// Earlier step ids and their action's response, when it is the
    /// product's own.
    earlier: BTreeMap<String, Option<MessageDescriptor>>,
    /// Every step id of the document.
    ids: BTreeSet<String>,
}

struct Checker<'a> {
    product: &'a str,
    surface: &'a Surface,
    ext: &'a Extensions,
    /// Import paths of the product's own files: the classification rule
    /// descends only into messages defined in them.
    own_files: BTreeSet<&'a str>,
    refusals: Vec<Refusal>,
}

impl<'a> Checker<'a> {
    fn new(
        product: &'a str,
        surface: &'a Surface,
        ext: &'a Extensions,
        own_files: BTreeSet<&'a str>,
    ) -> Self {
        Self {
            product,
            surface,
            ext,
            own_files,
            refusals: Vec::new(),
        }
    }

    fn refuse(&mut self, rule: Rule, site: &str, detail: impl Into<String>) {
        self.refusals.push(Refusal {
            rule,
            site: site.to_owned(),
            detail: detail.into(),
        });
    }

    fn own(&self, product: &str) -> bool {
        product.is_empty() || product == self.product
    }

    fn declarations(&mut self) {
        let surface = self.surface;
        for trigger in surface.triggers.values() {
            if !has_label(&trigger.declaration) {
                self.refuse(Rule::T1, &format!("trigger {}", trigger.event), "no label");
            }
            self.classified(&format!("trigger {}", trigger.event), &trigger.message);
        }
        for action in surface.actions.values() {
            let site = format!("action {}", action.method);
            let effect = enum_number(&action.declaration, "effect");
            if !has_label(&action.declaration) || effect == 0 {
                self.refuse(Rule::A1, &site, "no label, or the effect is unspecified");
            }
            let kind = enum_number(&action.authz, "kind");
            if kind != AUTHZ_KIND_AUTHENTICATED || requires(&action.authz).is_empty() {
                self.refuse(
                    Rule::A2,
                    &site,
                    "not AUTHZ_KIND_AUTHENTICATED with a non-empty requires",
                );
            }
            if action.descriptor.is_client_streaming() || action.descriptor.is_server_streaming() {
                self.refuse(Rule::A3, &site, "the RPC is not unary");
            }
            self.idempotency(&site, action, effect);
            self.resolves(&site, action, effect);
            let (request, response) = (action.descriptor.input(), action.descriptor.output());
            self.untyped(&site, &request);
            self.untyped(&site, &response);
            if effect == EFFECT_PURE {
                self.classified(&site, &response);
            }
            if action.flag("person_reserved") && action.flag("planner_pinned") {
                self.refuse(
                    Rule::A7,
                    &site,
                    "person_reserved and planner_pinned are both set",
                );
            }
        }
    }

    /// A4: a lasting action declares how a repeat behaves, and a keyed one
    /// names top-level request fields.
    fn idempotency(&mut self, site: &str, action: &Action, effect: i32) {
        let mode = enum_number(&action.declaration, "idempotency");
        if effect != EFFECT_PURE && mode == 0 {
            self.refuse(
                Rule::A4,
                site,
                "the effect is not PURE and idempotency is unspecified",
            );
        }
        if mode == IDEMPOTENCY_KEYED {
            let request = action.descriptor.input();
            let keys = strings(&action.declaration, "idempotency_key_fields");
            if keys.is_empty() {
                self.refuse(Rule::A4, site, "KEYED idempotency names no key field");
            }
            for key in keys {
                if request.get_field_by_name(&key).is_none() {
                    self.refuse(
                        Rule::A4,
                        site,
                        format!("key field {key:?} is not a top-level field of the request"),
                    );
                }
            }
        }
    }

    /// A5: `resolves` belongs to a read action and names real fields.
    fn resolves(&mut self, site: &str, action: &Action, effect: i32) {
        let Some(resolver) = sub(&action.declaration, "resolves") else {
            return;
        };
        if effect != EFFECT_PURE {
            self.refuse(
                Rule::A5,
                site,
                "resolves is set on an action that is not PURE",
            );
        }
        let query = string(&resolver, "query_field");
        if action
            .descriptor
            .input()
            .get_field_by_name(&query)
            .is_none()
        {
            self.refuse(
                Rule::A5,
                site,
                format!("query_field {query:?} is not a top-level field of the request"),
            );
        }
        let results = string(&resolver, "results_field");
        let candidate = match resolve_path(&action.descriptor.output(), &results) {
            Ok(field) => match field.kind() {
                Kind::Message(message) if field.is_list() && !field.is_map() => Some(message),
                _ => None,
            },
            Err(why) => {
                self.refuse(Rule::A5, site, format!("results_field {results:?}: {why}"));
                return;
            }
        };
        let Some(candidate) = candidate else {
            self.refuse(
                Rule::A5,
                site,
                format!("results_field {results:?} is not a repeated message field"),
            );
            return;
        };
        for name in ["value_field", "label_field"] {
            let path = string(&resolver, name);
            if let Err(why) = resolve_path(&candidate, &path) {
                self.refuse(Rule::A5, site, format!("{name} {path:?}: {why}"));
            }
        }
    }

    /// A6: nothing reachable from `root` is an untyped payload.
    fn untyped(&mut self, site: &str, root: &MessageDescriptor) {
        walk_messages(root, |message| {
            let mut next_messages = Vec::new();
            for field in message.fields() {
                let Kind::Message(next) = field.kind() else {
                    continue;
                };
                if UNTYPED.contains(&next.full_name()) {
                    self.refuse(
                        Rule::A6,
                        site,
                        format!("{} reaches {}", field.full_name(), next.full_name()),
                    );
                } else {
                    next_messages.push(next);
                }
            }
            next_messages
        });
    }

    /// C1: every field of `root`, at every depth through messages of the
    /// product's own files, carries a classification other than UNSPECIFIED.
    /// A message of another file is classified at the field and not entered.
    fn classified(&mut self, site: &str, root: &MessageDescriptor) {
        walk_messages(root, |message| {
            let mut next_messages = Vec::new();
            for field in message.fields() {
                let options = field.options();
                let class = if options.has_extension(&self.ext.classification) {
                    options
                        .get_extension(&self.ext.classification)
                        .as_enum_number()
                        .unwrap_or_default()
                } else {
                    0
                };
                if class == 0 {
                    self.refuse(
                        Rule::C1,
                        site,
                        format!("field {} is unclassified", field.full_name()),
                    );
                }
                let target = match field.kind() {
                    Kind::Message(entry) if field.is_map() => {
                        match entry.map_entry_value_field().kind() {
                            Kind::Message(value) => Some(value),
                            _ => None,
                        }
                    }
                    Kind::Message(next) => Some(next),
                    _ => None,
                };
                if let Some(next) = target {
                    if self.own_files.contains(next.parent_file().name()) {
                        next_messages.push(next);
                    }
                }
            }
            next_messages
        });
    }

    /// Check every template and return the resolved ones.
    fn templates(&mut self) -> Vec<DynamicMessage> {
        let surface = self.surface;
        let mut names = BTreeSet::new();
        let mut out = Vec::new();
        for template in &surface.templates {
            let name = string(template, "name");
            if !names.insert(name.clone()) {
                self.refuse(Rule::M1, &format!("template {name}"), "name declared twice");
            }
            self.template(&name, template);
            let mut template = template.clone();
            self.resolve(&mut template);
            out.push(template);
        }
        out
    }

    fn template(&mut self, name: &str, template: &DynamicMessage) {
        let site = format!("template {name}");
        if name.is_empty() || !has_label(template) {
            self.refuse(Rule::M1, &site, "a template needs a name and a label");
        }
        let doc = sub(template, "document")
            .unwrap_or_else(|| DynamicMessage::new(shipped("bones.v1.AutomationDocument")));
        let trigger = sub(&doc, "trigger");
        let trigger_message = if let Some(reference) = &trigger {
            self.trigger_of(&site, reference)
        } else {
            self.refuse(Rule::D1, &site, "the document names no trigger");
            None
        };
        let steps = subs(&doc, "steps");
        if steps.is_empty() {
            self.refuse(Rule::M1, &site, "the document has no steps");
        }
        if !subs(&doc, "arguments").is_empty() {
            self.refuse(Rule::D6, &site, "a template carries no arguments");
        }
        let mut scope = Scope {
            trigger: trigger_message,
            params: self.parameters(&site, &doc),
            earlier: BTreeMap::new(),
            ids: steps.iter().map(|s| string(s, "id")).collect(),
        };
        for step in &steps {
            let id = string(step, "id");
            let step_site = format!("{site} step {id}");
            if scope.earlier.contains_key(&id) {
                self.refuse(Rule::D3, &step_site, "the step id is used twice");
            }
            let output = self.step(&step_site, step, &scope);
            scope.earlier.insert(id, output);
        }
    }

    fn trigger_of(&mut self, site: &str, reference: &DynamicMessage) -> Option<MessageDescriptor> {
        if !self.own(&string(reference, "product")) {
            return None;
        }
        let event = string(reference, "event");
        let found = self.surface.triggers.get(&event).map(|t| t.message.clone());
        if found.is_none() {
            self.refuse(
                Rule::D1,
                site,
                format!("{event:?} is not a declared trigger"),
            );
        }
        found
    }

    fn action_of(&mut self, site: &str, reference: &DynamicMessage) -> Option<&'a Action> {
        if !self.own(&string(reference, "product")) {
            return None;
        }
        let method = string(reference, "method");
        let surface: &'a Surface = self.surface;
        let found = surface.actions.get(&method);
        if found.is_none() {
            self.refuse(
                Rule::D1,
                site,
                format!("{method:?} is not a declared action"),
            );
        }
        found
    }

    fn parameters(&mut self, site: &str, doc: &DynamicMessage) -> BTreeMap<String, Option<Shape>> {
        let mut params = BTreeMap::new();
        for parameter in subs(doc, "parameters") {
            let name = string(&parameter, "name");
            let psite = format!("{site} parameter {name}");
            if params.contains_key(&name) {
                self.refuse(Rule::D5, &psite, "the parameter is named twice");
            }
            if !has_label(&parameter) {
                self.refuse(Rule::M1, &psite, "a parameter needs a label");
            }
            let shape = self.parameter_shape(&psite, &parameter);
            if let Some(default) = sub(&parameter, "default_value") {
                if shape.is_none() || literal_shape(&default) != shape {
                    self.refuse(
                        Rule::D6,
                        &psite,
                        "the default does not match the parameter's type",
                    );
                }
            }
            params.insert(name, shape);
        }
        params
    }

    fn parameter_shape(&mut self, site: &str, parameter: &DynamicMessage) -> Option<Shape> {
        let Some((kind, Value::Message(body))) = case(parameter, "type") else {
            self.refuse(Rule::U1, site, "the parameter has no type");
            return None;
        };
        match kind.as_str() {
            "integer" => Some(Shape::Integer),
            "boolean" => Some(Shape::Boolean),
            "choice" => {
                let values: Vec<String> = subs(&body, "choices")
                    .iter()
                    .map(|c| string(c, "value"))
                    .collect();
                let unique: BTreeSet<&String> = values.iter().collect();
                if values.is_empty() || unique.len() != values.len() {
                    self.refuse(
                        Rule::D5,
                        site,
                        "a choice needs at least one value, all unique",
                    );
                }
                Some(Shape::Text)
            }
            "entity" => {
                let resolver = sub(&body, "resolver")
                    .unwrap_or_else(|| DynamicMessage::new(shipped("bones.v1.ActionRef")));
                if let Some(action) = self.action_of(site, &resolver) {
                    if !action.declaration.has_field_by_name("resolves") {
                        self.refuse(
                            Rule::D5,
                            site,
                            "the resolver action does not declare resolves",
                        );
                    }
                }
                Some(Shape::Text)
            }
            _ => Some(Shape::Text),
        }
    }

    fn step(
        &mut self,
        site: &str,
        step: &DynamicMessage,
        scope: &Scope,
    ) -> Option<MessageDescriptor> {
        let Some((_, Value::Message(body))) = case(step, "kind") else {
            self.refuse(Rule::U1, site, "the step has no kind");
            return None;
        };
        let reference = sub(&body, "action")
            .unwrap_or_else(|| DynamicMessage::new(shipped("bones.v1.ActionRef")));
        let action = self.action_of(site, &reference)?;
        if action.flag("person_reserved") {
            self.refuse(Rule::D2, site, "the action is person_reserved");
        }
        let request = action.descriptor.input();
        let mut seen = BTreeSet::new();
        for binding in subs(&body, "inputs") {
            let field = string(&binding, "field");
            if !seen.insert(field.clone()) {
                self.refuse(Rule::D4, site, format!("field {field:?} is bound twice"));
            }
            self.binding(site, &request, &binding, scope);
        }
        Some(action.descriptor.output())
    }

    fn binding(
        &mut self,
        site: &str,
        request: &MessageDescriptor,
        binding: &DynamicMessage,
        scope: &Scope,
    ) {
        let field = string(binding, "field");
        let destination = match resolve_path(request, &field) {
            Ok(f) => f,
            Err(why) => return self.refuse(Rule::D4, site, format!("input {field:?}: {why}")),
        };
        let Some((source, value)) = case(binding, "source") else {
            return self.refuse(Rule::U1, site, format!("input {field:?} has no source"));
        };
        match value {
            Value::String(text) if source == "trigger_field" => {
                if let Some(message) = &scope.trigger {
                    self.same_type(site, &destination, message, &text);
                }
            }
            Value::String(name) => match scope.params.get(&name) {
                None => self.refuse(
                    Rule::D5,
                    site,
                    format!("parameter {name:?} is not declared"),
                ),
                Some(shape) => {
                    if shape.is_none() || *shape != field_shape(&destination) {
                        self.refuse(
                            Rule::D4,
                            site,
                            format!("input {field:?} and parameter {name:?} differ in type"),
                        );
                    }
                }
            },
            Value::Message(body) if source == "step_output" => {
                let step = string(&body, "step");
                match scope.earlier.get(&step) {
                    None => {
                        let why = if scope.ids.contains(&step) {
                            "the step itself or a later one"
                        } else {
                            "no step of the document"
                        };
                        self.refuse(
                            Rule::D3,
                            site,
                            format!("input {field:?} reads {step:?}, {why}"),
                        );
                    }
                    Some(None) => {}
                    Some(Some(response)) => {
                        self.same_type(site, &destination, response, &string(&body, "field"));
                    }
                }
            }
            Value::Message(literal) => {
                let shape = literal_shape(&literal);
                if shape.is_none() || shape != field_shape(&destination) {
                    self.refuse(
                        Rule::D4,
                        site,
                        format!("input {field:?} and its literal differ in type"),
                    );
                }
            }
            _ => {}
        }
    }

    fn same_type(
        &mut self,
        site: &str,
        destination: &FieldDescriptor,
        message: &MessageDescriptor,
        path: &str,
    ) {
        match resolve_path(message, path) {
            Err(why) => self.refuse(Rule::D4, site, format!("source {path:?}: {why}")),
            Ok(source) => {
                if source.kind() != destination.kind() || source.is_list() != destination.is_list()
                {
                    self.refuse(
                        Rule::D4,
                        site,
                        format!(
                            "input {:?} and source {path:?} differ in type",
                            destination.name()
                        ),
                    );
                }
            }
        }
    }

    /// Fill every empty product with the declaring product and order each
    /// step's inputs by field.
    fn resolve(&self, template: &mut DynamicMessage) {
        let product = self.product;
        let fill = |reference: &mut DynamicMessage| {
            if string(reference, "product").is_empty() {
                reference.set_field_by_name("product", Value::String(product.to_owned()));
            }
        };
        let Some(doc) = msg_mut(template, "document") else {
            return;
        };
        if let Some(trigger) = msg_mut(doc, "trigger") {
            fill(trigger);
        }
        for parameter in list_mut(doc, "parameters") {
            if let Some(resolver) =
                msg_mut(parameter, "entity").and_then(|e| msg_mut(e, "resolver"))
            {
                fill(resolver);
            }
        }
        for step in list_mut(doc, "steps") {
            let Some(action) = msg_mut(step, "action") else {
                continue;
            };
            if let Some(reference) = msg_mut(action, "action") {
                fill(reference);
            }
            if let Some(inputs) = action
                .get_field_by_name_mut("inputs")
                .and_then(Value::as_list_mut)
            {
                inputs.sort_by_cached_key(|v| v.as_message().map(|m| string(m, "field")));
            }
        }
    }
}

fn literal_shape(value: &DynamicMessage) -> Option<Shape> {
    match case(value, "kind")?.0.as_str() {
        "text" => Some(Shape::Text),
        "integer" => Some(Shape::Integer),
        _ => Some(Shape::Boolean),
    }
}

fn field_shape(field: &FieldDescriptor) -> Option<Shape> {
    if field.is_list() || field.is_map() {
        return None;
    }
    match field.kind() {
        Kind::String => Some(Shape::Text),
        Kind::Bool => Some(Shape::Boolean),
        Kind::Int32
        | Kind::Int64
        | Kind::Uint32
        | Kind::Uint64
        | Kind::Sint32
        | Kind::Sint64
        | Kind::Fixed32
        | Kind::Fixed64
        | Kind::Sfixed32
        | Kind::Sfixed64 => Some(Shape::Integer),
        _ => None,
    }
}

/// The field a dot-separated path names, descending only through singular
/// message-typed fields.
fn resolve_path(message: &MessageDescriptor, path: &str) -> Result<FieldDescriptor, String> {
    let mut current = message.clone();
    let mut segments = path.split('.').peekable();
    while let Some(segment) = segments.next() {
        let field = current
            .get_field_by_name(segment)
            .ok_or_else(|| format!("{} has no field {segment:?}", current.full_name()))?;
        if segments.peek().is_none() {
            return Ok(field);
        }
        match field.kind() {
            Kind::Message(next) if !field.is_list() && !field.is_map() => current = next,
            _ => {
                return Err(format!(
                    "{segment:?} is not a singular message to descend into"
                ));
            }
        }
    }
    unreachable!("a split path has at least one segment")
}

/// Visit `root` and every message `step` returns, each once. `step` sees a
/// message and names the messages to visit next.
fn walk_messages(
    root: &MessageDescriptor,
    mut step: impl FnMut(&MessageDescriptor) -> Vec<MessageDescriptor>,
) {
    let mut seen = BTreeSet::new();
    let mut pending = vec![root.clone()];
    while let Some(message) = pending.pop() {
        if seen.insert(message.full_name().to_owned()) {
            pending.extend(step(&message));
        }
    }
}
