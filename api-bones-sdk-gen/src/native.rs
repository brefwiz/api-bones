// SPDX-License-Identifier: MIT
//! Native-operation generator shared by the descriptor CLI and buf plugin.

use std::collections::BTreeSet;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;

use anyhow::{Context, Result, bail};
use prost::Message;
use prost_reflect::{DescriptorPool, DynamicMessage, Value};
use prost_types::compiler::{CodeGeneratorRequest, CodeGeneratorResponse, code_generator_response};

const NATIVE_EXTENSION: &str = "bones.v1.native_services";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Kind {
    Publish,
    Deliver,
    Acknowledge,
    NegativeAcknowledge,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Operation {
    name: String,
    kind: Kind,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Service {
    package: String,
    name: String,
    operations: Vec<Operation>,
}

pub fn generate_descriptor_set(
    descriptor_set: &Path,
    rust_out: &Path,
    typescript_out: &Path,
    proof_out: Option<&Path>,
    check: bool,
) -> Result<()> {
    let bytes = fs::read(descriptor_set)
        .with_context(|| format!("read descriptor set {}", descriptor_set.display()))?;
    generate_descriptor_set_bytes(&bytes, rust_out, typescript_out, check)?;
    if let Some(proof_out) = proof_out {
        let services = services_from_descriptor_set(&bytes, None, false)?;
        let rpc_count = rpc_method_count(&bytes)?;
        write_output(
            proof_out,
            &format!(
                "{{\"rpcCount\":{rpc_count},\"nativeServiceCount\":{}}}\n",
                services.len()
            ),
        )?;
    }
    Ok(())
}

/// Generate or check both outputs from encoded descriptor bytes.
pub fn generate_descriptor_set_bytes(
    bytes: &[u8],
    rust_out: &Path,
    typescript_out: &Path,
    check: bool,
) -> Result<()> {
    let services = services_from_descriptor_set(bytes, None, false)?;
    let rust = render_rust(&services);
    let typescript = render_typescript(&services);
    if check {
        check_output(rust_out, &rust)?;
        check_output(typescript_out, &typescript)?;
    } else {
        write_output(rust_out, &rust)?;
        write_output(typescript_out, &typescript)?;
    }
    Ok(())
}

/// Generate both target sources from one encoded `FileDescriptorSet`.
pub fn generate_sources(bytes: &[u8]) -> Result<(String, String)> {
    let services = services_from_descriptor_set(bytes, None, false)?;
    Ok((render_rust(&services), render_typescript(&services)))
}

/// Count RPC methods in a descriptor set independently from native metadata.
pub fn rpc_method_count(bytes: &[u8]) -> Result<usize> {
    let pool = DescriptorPool::decode(bytes).context("decode proto descriptor set")?;
    Ok(pool
        .services()
        .map(|service| service.methods().count())
        .sum())
}

fn write_output(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, contents).with_context(|| format!("write {}", path.display()))
}

fn check_output(path: &Path, expected: &str) -> Result<()> {
    let actual = fs::read_to_string(path)
        .with_context(|| format!("stale generated output: {} is missing", path.display()))?;
    if actual != expected {
        bail!("stale generated output: {}", path.display());
    }
    Ok(())
}

fn services_from_descriptor_set(
    bytes: &[u8],
    files_to_generate: Option<&BTreeSet<String>>,
    allow_empty: bool,
) -> Result<Vec<Service>> {
    let pool = DescriptorPool::decode(bytes).context("decode proto descriptor set")?;
    let Some(extension) = pool.get_extension_by_name(NATIVE_EXTENSION) else {
        if allow_empty {
            return Ok(Vec::new());
        }
        bail!("descriptor set does not include bones.v1 native annotations");
    };
    let mut services = Vec::new();
    for file in pool.files() {
        if files_to_generate.is_some_and(|files| !files.contains(file.name())) {
            continue;
        }
        let options = file.options();
        if !options.has_extension(&extension) {
            continue;
        }
        let extension_value = options.get_extension(&extension).into_owned();
        let native_services = match extension_value {
            Value::List(values) => values,
            _ => bail!("native_services annotation is not repeated"),
        };
        for native_value in &native_services {
            let native = message(native_value, "native_services")?;
            let service_name = string_field(native, "name")?;
            if !valid_pascal_case(&service_name) {
                bail!("native service name {service_name:?} must be PascalCase");
            }
            let operations = list_field(native, "operations")?;
            if operations.is_empty() {
                bail!("native service {service_name} declares no operations");
            }
            let mut names = BTreeSet::new();
            let mut generated_names = BTreeSet::new();
            let mut parsed = Vec::new();
            for value in &operations {
                let operation = message(value, "operations")?;
                let name = string_field(operation, "name")?;
                if !valid_lower_snake(&name) {
                    bail!("native operation name {name:?} must be lower_snake_case");
                }
                if !names.insert(name.clone()) {
                    bail!("duplicate native operation {name:?} in {service_name}");
                }
                let generated_name = lower_camel(&name);
                if reserved_operation_name(&name, &generated_name) {
                    bail!("native operation name {name:?} conflicts with generated client API");
                }
                if !generated_names.insert(generated_name) {
                    bail!("native operation names in {service_name} collide after case conversion");
                }
                let kind = enum_field(operation, "kind")?;
                let kind = match kind {
                    1 => Kind::Publish,
                    2 => Kind::Deliver,
                    3 => Kind::Acknowledge,
                    4 => Kind::NegativeAcknowledge,
                    _ => bail!("native operation {name:?} has unspecified kind"),
                };
                parsed.push(Operation { name, kind });
            }
            parsed.sort();
            services.push(Service {
                package: file.package_name().to_owned(),
                name: service_name,
                operations: parsed,
            });
        }
    }
    if services.is_empty() && !allow_empty {
        bail!("descriptor set declares no native services");
    }
    services.sort();
    let mut emitted_symbols = BTreeSet::new();
    for service in &services {
        let symbol = client_name(service);
        if !emitted_symbols.insert(symbol.clone()) {
            bail!("native services collide on generated client name {symbol:?}");
        }
    }
    for pair in services.windows(2) {
        if pair[0].package == pair[1].package && pair[0].name == pair[1].name {
            bail!(
                "duplicate native service {}.{}",
                pair[0].package,
                pair[0].name
            );
        }
    }
    Ok(services)
}

fn valid_pascal_case(value: &str) -> bool {
    value.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
        && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

fn reserved_operation_name(rust_name: &str, typescript_name: &str) -> bool {
    const RUST: &[&str] = &[
        "abstract",
        "as",
        "async",
        "await",
        "become",
        "box",
        "break",
        "const",
        "continue",
        "crate",
        "do",
        "dyn",
        "else",
        "enum",
        "extern",
        "false",
        "final",
        "fn",
        "for",
        "gen",
        "if",
        "impl",
        "in",
        "let",
        "loop",
        "macro",
        "match",
        "mod",
        "move",
        "mut",
        "override",
        "priv",
        "pub",
        "ref",
        "return",
        "self",
        "static",
        "struct",
        "super",
        "trait",
        "true",
        "try",
        "type",
        "typeof",
        "unsafe",
        "unsized",
        "use",
        "virtual",
        "where",
        "while",
        "yield",
        "new",
        "transport",
    ];
    const TYPESCRIPT: &[&str] = &[
        "break",
        "case",
        "catch",
        "class",
        "const",
        "constructor",
        "continue",
        "debugger",
        "default",
        "delete",
        "do",
        "else",
        "enum",
        "export",
        "extends",
        "false",
        "finally",
        "for",
        "function",
        "if",
        "import",
        "in",
        "instanceof",
        "new",
        "null",
        "return",
        "super",
        "switch",
        "this",
        "throw",
        "true",
        "try",
        "typeof",
        "var",
        "void",
        "while",
        "with",
        "yield",
        "transport",
    ];
    RUST.contains(&rust_name) || TYPESCRIPT.contains(&typescript_name)
}

fn message<'a>(value: &'a Value, field: &str) -> Result<&'a DynamicMessage> {
    match value {
        Value::Message(value) => Ok(value),
        _ => bail!("native annotation field {field} is not a message"),
    }
}

fn list_field(message: &DynamicMessage, field: &str) -> Result<Vec<Value>> {
    match message.get_field_by_name(field).as_deref() {
        Some(Value::List(values)) => Ok(values.clone()),
        _ => bail!("native annotation field {field} is not repeated"),
    }
}

fn string_field(message: &DynamicMessage, field: &str) -> Result<String> {
    match message.get_field_by_name(field).as_deref() {
        Some(Value::String(value)) if !value.is_empty() => Ok(value.clone()),
        _ => bail!("native annotation field {field} must be a non-empty string"),
    }
}

fn enum_field(message: &DynamicMessage, field: &str) -> Result<i32> {
    match message.get_field_by_name(field).as_deref() {
        Some(Value::EnumNumber(value)) => Ok(*value),
        _ => bail!("native annotation field {field} must be an enum"),
    }
}

fn valid_lower_snake(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('_')
        && !value.ends_with('_')
        && value.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        && !value.contains("__")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn rust_method(operation: &Operation) -> String {
    match operation.kind {
        Kind::Publish => format!(
            "    pub async fn {}(&self, request: Publication, options: CallOptions) -> Result<PublishResult, NativeError> {{ options.check()?; self.transport.publish(\"{}\", request, options).await }}\n",
            operation.name, operation.name
        ),
        Kind::Deliver => format!(
            "    pub async fn {}(&self, options: CallOptions) -> Result<BoxDeliveryStream, NativeError> {{ options.check()?; self.transport.deliver(\"{}\", options).await }}\n",
            operation.name, operation.name
        ),
        Kind::Acknowledge => format!(
            "    pub async fn {}(&self, handle: &AckHandle, options: CallOptions) -> Result<(), NativeError> {{ options.check()?; let attempt = AckAttempt::begin(handle)?; match self.transport.acknowledge(\"{}\", handle.clone(), options).await {{ Ok(()) => {{ attempt.succeed(); Ok(()) }}, Err(error) => Err(error) }} }}\n",
            operation.name, operation.name
        ),
        Kind::NegativeAcknowledge => format!(
            "    pub async fn {}(&self, handle: &AckHandle, options: CallOptions) -> Result<(), NativeError> {{ options.check()?; let attempt = AckAttempt::begin(handle)?; match self.transport.negative_acknowledge(\"{}\", handle.clone(), options).await {{ Ok(()) => {{ attempt.succeed(); Ok(()) }}, Err(error) => Err(error) }} }}\n",
            operation.name, operation.name
        ),
    }
}

fn render_rust(services: &[Service]) -> String {
    let mut output = String::from(RUST_PREAMBLE);
    for service in services {
        let client = client_name(service);
        output.push_str(&format!("/// Native client for `{}.{}`.\npub struct {client}<T> {{ transport: T }}\nimpl<T: NativeTransport> {client}<T> {{\n    pub fn new(transport: T) -> Self {{ Self {{ transport }} }}\n", service.package, service.name));
        for operation in &service.operations {
            output.push_str(&rust_method(operation));
        }
        output.push_str("}\n");
    }
    output
}

fn ts_method(operation: &Operation) -> String {
    let camel = lower_camel(&operation.name);
    match operation.kind {
        Kind::Publish => format!(
            "  async {camel}(request: Publication, options: CallOptions = {{}}): Promise<PublishResult> {{ checkCall(options); return await this.transport.publish(\"{}\", request, options); }}\n",
            operation.name
        ),
        Kind::Deliver => format!(
            "  {camel}(options: CallOptions = {{}}): AsyncIterable<Delivery> {{ checkCall(options); return this.transport.deliver(\"{}\", options); }}\n",
            operation.name
        ),
        Kind::Acknowledge => format!(
            "  async {camel}(handle: AckHandle, options: CallOptions = {{}}): Promise<void> {{ checkCall(options); beginAck(handle); try {{ await this.transport.acknowledge(\"{}\", handle, options); finishAck(handle); }} catch (error) {{ retryAck(handle); throw error; }} }}\n",
            operation.name
        ),
        Kind::NegativeAcknowledge => format!(
            "  async {camel}(handle: AckHandle, options: CallOptions = {{}}): Promise<void> {{ checkCall(options); beginAck(handle); try {{ await this.transport.negativeAcknowledge(\"{}\", handle, options); finishAck(handle); }} catch (error) {{ retryAck(handle); throw error; }} }}\n",
            operation.name
        ),
    }
}

fn render_typescript(services: &[Service]) -> String {
    let mut output = String::from(TS_PREAMBLE);
    for service in services {
        let client = client_name(service);
        output.push_str(&format!("/** Native client for {}.{}. */\nexport class {client} {{\n  constructor(private readonly transport: NativeTransport) {{}}\n", service.package, service.name));
        for operation in &service.operations {
            output.push_str(&ts_method(operation));
        }
        output.push_str("}\n");
    }
    output
}

fn client_name(service: &Service) -> String {
    let package = service
        .package
        .split('.')
        .map(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect::<String>())
                .unwrap_or_default()
        })
        .collect::<String>();
    format!("{package}{}Client", service.name)
}

fn lower_camel(value: &str) -> String {
    let mut parts = value.split('_');
    let mut result = parts.next().unwrap_or_default().to_owned();
    for part in parts {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            result.extend(first.to_uppercase());
            result.extend(chars);
        }
    }
    result
}

const RUST_PREAMBLE: &str = r#"// @generated by api-bones-sdk-gen. Do not edit.
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, atomic::{AtomicBool, AtomicU8}};
use std::time::Instant;

#[derive(Clone, Debug, Default)]
pub struct CallOptions { pub deadline: Option<Instant>, pub cancelled: Option<Arc<AtomicBool>> }
impl CallOptions { fn check(&self) -> Result<(), NativeError> { if self.cancelled.as_ref().is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Relaxed)) { return Err(NativeError::Cancelled); } if self.deadline.is_some_and(|deadline| Instant::now() >= deadline) { return Err(NativeError::DeadlineExceeded); } Ok(()) } }
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Publication { pub message_id: String, pub entity_id: String, pub payload: Vec<u8> }
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublishResult { pub duplicate: bool }
#[derive(Clone, Debug)]
pub struct AckHandle { pub value: Vec<u8>, state: Arc<AtomicU8> }
impl AckHandle { pub fn new(value: Vec<u8>) -> Self { Self { value, state: Arc::new(AtomicU8::new(0)) } } fn begin(&self) -> Result<(), NativeError> { self.state.compare_exchange(0, 1, std::sync::atomic::Ordering::Relaxed, std::sync::atomic::Ordering::Relaxed).map(|_| ()).map_err(|_| NativeError::Transport { code: "ack_handle_used".to_owned() }) } fn succeed(&self) { self.state.store(2, std::sync::atomic::Ordering::Relaxed); } fn retry(&self) { self.state.store(0, std::sync::atomic::Ordering::Relaxed); } }
struct AckAttempt { handle: AckHandle, complete: bool }
impl AckAttempt { fn begin(handle: &AckHandle) -> Result<Self, NativeError> { handle.begin()?; Ok(Self { handle: handle.clone(), complete: false }) } fn succeed(mut self) { self.handle.succeed(); self.complete = true; } }
impl Drop for AckAttempt { fn drop(&mut self) { if !self.complete { self.handle.retry(); } } }
#[derive(Clone, Debug)]
pub struct Delivery { pub message_id: String, pub entity_id: String, pub payload: Vec<u8>, pub ack_handle: AckHandle }
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NativeError { PermissionDenied { code: String }, Cancelled, DeadlineExceeded, Transport { code: String } }
pub type BoxNativeFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, NativeError>> + Send + 'a>>;
pub trait DeliveryStream: Send { fn next(&mut self) -> BoxNativeFuture<'_, Option<Delivery>>; }
pub type BoxDeliveryStream = Box<dyn DeliveryStream>;
pub trait NativeTransport: Send + Sync {
    // Implementations must observe cancellation and deadline while in flight;
    // generated clients additionally refuse calls already cancelled/expired.
    fn publish(&self, operation: &'static str, request: Publication, options: CallOptions) -> BoxNativeFuture<'_, PublishResult>;
    fn deliver(&self, operation: &'static str, options: CallOptions) -> BoxNativeFuture<'_, BoxDeliveryStream>;
    fn acknowledge(&self, operation: &'static str, handle: AckHandle, options: CallOptions) -> BoxNativeFuture<'_, ()>;
    fn negative_acknowledge(&self, operation: &'static str, handle: AckHandle, options: CallOptions) -> BoxNativeFuture<'_, ()>;
}
"#;

const TS_PREAMBLE: &str = r#"// @generated by api-bones-sdk-gen. Do not edit.
export interface CallOptions { signal?: AbortSignal; deadline?: Date; }
export interface Publication { messageId: string; entityId: string; payload: Uint8Array; }
export interface PublishResult { duplicate: boolean; }
export class AckHandle { constructor(readonly value: Uint8Array) { ackStates.set(this, "fresh"); } }
const ackStates = new WeakMap<AckHandle, "fresh" | "pending" | "done">();
function beginAck(handle: AckHandle): void { if (ackStates.get(handle) !== "fresh") throw new NativeError("transport", "acknowledgement handle already used"); ackStates.set(handle, "pending"); }
function finishAck(handle: AckHandle): void { ackStates.set(handle, "done"); }
function retryAck(handle: AckHandle): void { ackStates.set(handle, "fresh"); }
export interface Delivery { messageId: string; entityId: string; payload: Uint8Array; ackHandle: AckHandle; }
export type NativeErrorCode = "permission_denied" | "cancelled" | "deadline_exceeded" | "transport";
export class NativeError extends Error { constructor(readonly code: NativeErrorCode, message: string) { super(message); } }
function checkCall(options: CallOptions): void { if (options.signal?.aborted) throw new NativeError("cancelled", "operation cancelled"); if (options.deadline !== undefined && options.deadline.getTime() <= Date.now()) throw new NativeError("deadline_exceeded", "operation deadline exceeded"); }
export interface NativeTransport {
  /** Observe signal and deadline for the full in-flight operation. */
  publish(operation: string, request: Publication, options: CallOptions): Promise<PublishResult>;
  deliver(operation: string, options: CallOptions): AsyncIterable<Delivery>;
  acknowledge(operation: string, handle: AckHandle, options: CallOptions): Promise<void>;
  negativeAcknowledge(operation: string, handle: AckHandle, options: CallOptions): Promise<void>;
}
"#;

pub fn run_plugin() -> Result<()> {
    let mut input = Vec::new();
    std::io::stdin().read_to_end(&mut input)?;
    // Decode request metadata normally, but preserve each embedded
    // FileDescriptorProto's raw bytes for custom options. Decoding through
    // prost-types alone discards extensions unknown to that generated type.
    let request =
        CodeGeneratorRequest::decode(input.as_slice()).context("decode code generator request")?;
    let descriptor = descriptor_set_from_plugin_request(&input)?;
    let target = request
        .parameter
        .as_deref()
        .unwrap_or("rust")
        .strip_prefix("target=")
        .unwrap_or(request.parameter.as_deref().unwrap_or("rust"));
    let output = generate_plugin_output(&descriptor, &request.file_to_generate, target)?;
    let response = CodeGeneratorResponse {
        file: output
            .map(|(name, content)| code_generator_response::File {
                name: Some(name),
                content: Some(content),
                ..Default::default()
            })
            .into_iter()
            .collect(),
        supported_features: Some(1),
        ..Default::default()
    };
    std::io::stdout().write_all(&response.encode_to_vec())?;
    Ok(())
}

/// Generate one protoc-plugin response file from raw descriptor bytes.
///
/// Shared by the stdin/stdout plugin adapter and generated-client contract
/// fixtures so both exercise identical requested-file selection and rendering.
pub fn generate_plugin_output(
    descriptor: &[u8],
    files_to_generate: &[String],
    target: &str,
) -> Result<Option<(String, String)>> {
    let files_to_generate = files_to_generate.iter().cloned().collect();
    let services = services_from_descriptor_set(descriptor, Some(&files_to_generate), true)?;
    if services.is_empty() {
        return Ok(None);
    }
    match target {
        "rust" => Ok(Some(("bones_native.rs".to_owned(), render_rust(&services)))),
        "typescript" => Ok(Some((
            "bones_native.ts".to_owned(),
            render_typescript(&services),
        ))),
        other => bail!("unsupported native generator target {other:?}"),
    }
}

fn descriptor_set_from_plugin_request(input: &[u8]) -> Result<Vec<u8>> {
    let mut cursor = 0;
    let mut output = Vec::new();
    while cursor < input.len() {
        let key = read_varint(input, &mut cursor)?;
        let field = key >> 3;
        let wire = key & 7;
        if wire == 2 {
            let len = usize::try_from(read_varint(input, &mut cursor)?)
                .context("plugin field length does not fit usize")?;
            let end = cursor
                .checked_add(len)
                .context("plugin field length overflow")?;
            let bytes = input.get(cursor..end).context("truncated plugin field")?;
            cursor = end;
            if field == 15 {
                output.push(0x0a);
                write_varint(u64::try_from(len)?, &mut output);
                output.extend_from_slice(bytes);
            }
        } else {
            skip_wire(input, &mut cursor, wire)?;
        }
    }
    if output.is_empty() {
        bail!("code generator request contains no proto descriptors");
    }
    Ok(output)
}

fn read_varint(input: &[u8], cursor: &mut usize) -> Result<u64> {
    let mut value = 0_u64;
    for shift in (0..70).step_by(7) {
        let byte = *input.get(*cursor).context("truncated protobuf varint")?;
        *cursor += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    bail!("protobuf varint exceeds 10 bytes")
}

fn write_varint(mut value: u64, output: &mut Vec<u8>) {
    while value >= 0x80 {
        output.push((value as u8 & 0x7f) | 0x80);
        value >>= 7;
    }
    output.push(value as u8);
}

fn skip_wire(input: &[u8], cursor: &mut usize, wire: u64) -> Result<()> {
    let len = match wire {
        0 => {
            read_varint(input, cursor)?;
            return Ok(());
        }
        1 => 8,
        2 => usize::try_from(read_varint(input, cursor)?)?,
        5 => 4,
        _ => bail!("unsupported protobuf wire type {wire}"),
    };
    *cursor = (*cursor)
        .checked_add(len)
        .context("protobuf field length overflow")?;
    if *cursor > input.len() {
        bail!("truncated protobuf field");
    }
    Ok(())
}

pub fn write_plugin_error(error: &str) {
    let response = CodeGeneratorResponse {
        error: Some(error.to_owned()),
        ..Default::default()
    };
    let _ = std::io::stdout().write_all(&response.encode_to_vec());
}
