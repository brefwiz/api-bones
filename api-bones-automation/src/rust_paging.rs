// SPDX-License-Identifier: MIT
//! Rust page glue emitted from the canonical descriptor paging policy.

use std::collections::BTreeSet;
use std::fmt::{self, Write};

use heck::{ToSnakeCase, ToUpperCamelCase};
use prost_reflect::{DescriptorPool, Kind, MessageDescriptor, MethodDescriptor};

use crate::paging_policy;

/// A descriptor cannot be represented by Rust platform page glue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustPagingError(String);

impl fmt::Display for RustPagingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RustPagingError {}

fn rust_ident(name: &str) -> String {
    escape_ident(&name.to_snake_case())
}

fn escape_ident(name: &str) -> String {
    match name {
        "self" | "Self" | "super" | "crate" => format!("{name}_"),
        "as" | "async" | "await" | "break" | "const" | "continue" | "dyn" | "else" | "enum"
        | "extern" | "false" | "fn" | "for" | "if" | "impl" | "in" | "let" | "loop" | "match"
        | "mod" | "move" | "mut" | "pub" | "ref" | "return" | "static" | "struct" | "trait"
        | "true" | "type" | "unsafe" | "use" | "where" | "while" | "yield" | "try" | "gen"
        | "abstract" | "become" | "box" | "do" | "final" | "macro" | "override" | "priv"
        | "typeof" | "unsized" | "virtual" => format!("r#{name}"),
        _ => name.to_owned(),
    }
}

fn message_path(message: &MessageDescriptor, module: &dyn Fn(&str) -> String) -> String {
    let package = message.package_name();
    let local = message
        .full_name()
        .strip_prefix(package)
        .unwrap_or(message.full_name());
    let mut parts: Vec<_> = local.trim_start_matches('.').split('.').collect();
    let name = parts.pop().expect("message has a name");
    let mut path = module(package);
    for part in parts {
        write!(path, "::{}", nested_module(part)).expect("write to String");
    }
    write!(path, "::{name}").expect("write to String");
    path
}

fn nested_module(name: &str) -> String {
    let chars: Vec<_> = name.chars().collect();
    let mut out = String::new();
    for (index, ch) in chars.iter().enumerate() {
        if ch.is_uppercase() && index > 0 {
            let prev = chars[index - 1];
            if prev.is_lowercase()
                || (prev.is_uppercase()
                    && chars.get(index + 1).is_some_and(|next| next.is_lowercase()))
            {
                out.push('_');
            }
        }
        out.extend(ch.to_lowercase());
    }
    escape_ident(&out)
}

#[derive(Default)]
struct Output {
    requests: BTreeSet<String>,
    responses: BTreeSet<String>,
    text: String,
}

impl Output {
    fn method(
        &mut self,
        method: &MethodDescriptor,
        policy: &crate::PagingPolicy,
        module: &dyn Fn(&str) -> String,
    ) -> Result<String, RustPagingError> {
        let request = method.input();
        let response = method.output();
        let req = message_path(&request, module);
        let resp = message_path(&response, module);
        let request_page = request
            .fields()
            .find(|field| field.json_name() == policy.request_page_field)
            .expect("paging request field");
        let items = response
            .fields()
            .find(|field| field.json_name() == policy.item_field)
            .expect("paging items field");
        let page = response
            .fields()
            .find(|field| field.json_name() == policy.page_field)
            .expect("paging response field");
        let item = item_type(items.kind(), module)?;
        if self.requests.insert(request.full_name().to_owned()) {
            writeln!(
                self.text,
                "api_bones_batch::paged_request!({req}, {});",
                escape_ident(request_page.name())
            )
            .expect("write to String");
        }
        if self.responses.insert(response.full_name().to_owned()) {
            writeln!(
                self.text,
                "api_bones_batch::paged_response!({resp}, item: {item}, items: {}, page: {});",
                escape_ident(items.name()),
                escape_ident(page.name())
            )
            .expect("write to String");
        }
        let idempotency = match method
            .options()
            .get_field_by_name("idempotency_level")
            .and_then(|value| value.as_enum_number())
        {
            Some(1) => "NoSideEffects",
            Some(2) => "Idempotent",
            _ => "Unspecified",
        };
        let name = rust_ident(method.name());
        let with_options = format!("{}_with_options", method.name().to_snake_case());
        Ok(format!(
            "{{ method: \"{}/{}\", name: {name}, with_options: {with_options}, idempotency: api_bones_batch::__private::Idempotency::{idempotency}, request: {req}, response: {resp} }},\n",
            method.parent_service().full_name(),
            method.name()
        ))
    }
}

fn item_type(kind: Kind, module: &dyn Fn(&str) -> String) -> Result<String, RustPagingError> {
    Ok(match kind {
        Kind::Double => "f64".into(),
        Kind::Float => "f32".into(),
        Kind::Int32 | Kind::Sint32 | Kind::Sfixed32 => "i32".into(),
        Kind::Int64 | Kind::Sint64 | Kind::Sfixed64 => "i64".into(),
        Kind::Uint32 | Kind::Fixed32 => "u32".into(),
        Kind::Uint64 | Kind::Fixed64 => "u64".into(),
        Kind::Bool => "bool".into(),
        Kind::String => "String".into(),
        Kind::Bytes => "Vec<u8>".into(),
        Kind::Message(message) => message_path(&message, module),
        Kind::Enum(value) => {
            return Err(RustPagingError(format!(
                "enum page items are unsupported: {}",
                value.full_name()
            )));
        }
    })
}

/// Emit traits and list methods for unary paging RPCs on selected services.
///
/// `package_module` maps a proto package to its generated Rust module path.
/// Server streams remain available only on the generated raw client.
///
/// # Errors
/// Refuses unknown or duplicate services, duplicate service names, enum items,
/// and generated method names that collide with the platform client surface.
///
/// # Panics
/// Panics if the paging policy names fields absent from its own descriptor.
pub fn emit_rust_paging(
    pool: &DescriptorPool,
    services: &[&str],
    package_module: &dyn Fn(&str) -> String,
) -> Result<String, RustPagingError> {
    let mut selected = services.to_vec();
    selected.sort_unstable();
    let mut service_names = BTreeSet::new();
    let mut output = Output::default();
    for service_name in selected {
        let service = pool
            .get_service_by_name(service_name)
            .ok_or_else(|| RustPagingError(format!("unknown service: {service_name}")))?;
        let name = service.name().to_upper_camel_case();
        if !service_names.insert(name.clone()) {
            return Err(RustPagingError(format!("duplicate service name: {name}")));
        }
        let mut methods: Vec<_> = service.methods().collect();
        methods.sort_by(|a, b| a.name().cmp(b.name()));
        let mut entries = String::new();
        let mut method_names = BTreeSet::new();
        for method in methods {
            if method.is_client_streaming() || method.is_server_streaming() {
                continue;
            }
            let Some(policy) = paging_policy(pool, &method) else {
                continue;
            };
            if policy.kind != "unary" {
                continue;
            }
            let rust_name = rust_ident(method.name());
            let with_options = format!("{}_with_options", method.name().to_snake_case());
            if method.name().starts_with("__")
                || rust_name.starts_with("__")
                || matches!(
                    rust_name.as_str(),
                    "new" | "anonymous" | "observed" | "raw" | "clone"
                )
                || !method_names.insert(rust_name.clone())
                || !method_names.insert(with_options.clone())
            {
                return Err(RustPagingError(format!(
                    "reserved or duplicate list method: {}",
                    method.full_name()
                )));
            }
            entries.push_str(&output.method(&method, &policy, package_module)?);
        }
        if !entries.is_empty() {
            writeln!(output.text, "api_bones_batch::paged_method! {{ ext: {name}Lists, client: {}::{name}Client<api_bones_batch::__private::connectrpc::client::HttpClient>, lists: [\n{entries}] }}", package_module(service.package_name())).expect("write to String");
        }
    }
    Ok(output.text)
}

#[cfg(test)]
#[path = "../tests/support/resolver.rs"]
mod resolver;

#[cfg(test)]
mod tests {
    use super::*;

    fn pool(item: &str, services: &str) -> DescriptorPool {
        let source = format!(
            "syntax = \"proto3\"; package fixture.v1; import \"bones/v1/annotations.proto\"; import \"bones/v1/pagination.proto\"; message Req {{ bones.v1.PageRequest page = 1; }} message Res {{ repeated {item} items = 1; bones.v1.PageResponse page = 2; }} enum Choice {{ UNKNOWN = 0; }} {services}"
        );
        let mut compiler = protox::Compiler::with_file_resolver(resolver::Resolver(source));
        compiler.include_imports(true);
        compiler
            .open_file(resolver::PATH)
            .expect("fixture compiles");
        compiler.descriptor_pool()
    }

    fn module(package: &str) -> String {
        format!("crate::{}", package.replace('.', "::"))
    }

    #[test]
    fn emits_sorted_methods_shared_traits_and_all_idempotency_levels() {
        let pool = pool(
            "uint32",
            "service Zed { rpc Z(Req) returns (Res); } service Alpha { rpc Write(Req) returns (Res); rpc Read(Req) returns (Res) { option idempotency_level = NO_SIDE_EFFECTS; } rpc Retry(Req) returns (Res) { option idempotency_level = IDEMPOTENT; } }",
        );
        let a = emit_rust_paging(&pool, &["fixture.v1.Zed", "fixture.v1.Alpha"], &module)
            .expect("emit");
        let b = emit_rust_paging(&pool, &["fixture.v1.Alpha", "fixture.v1.Zed"], &module)
            .expect("emit");
        assert_eq!(a, b);
        assert_eq!(a.matches("paged_request!").count(), 1);
        assert_eq!(a.matches("paged_response!").count(), 1);
        assert!(a.contains("item: u32"));
        assert!(a.contains("Idempotency::NoSideEffects"));
        assert!(a.contains("Idempotency::Idempotent"));
        assert!(a.contains("Idempotency::Unspecified"));
        assert!(a.find("name: read,").expect("read") < a.find("name: retry,").expect("retry"));
        assert!(a.find("ext: AlphaLists").expect("alpha") < a.find("ext: ZedLists").expect("zed"));
    }

    #[test]
    fn streams_stay_raw_and_message_items_use_package_mapping() {
        let pool = pool(
            "Req",
            "message Plain {} message Ambiguous { repeated string first = 1; repeated string second = 2; bones.v1.PageResponse page = 3; } service Lists { rpc List(Req) returns (Res); rpc Watch(Req) returns (stream Res) { option (bones.v1.list_stream_of) = \"List\"; } rpc Upload(stream Req) returns (Res); rpc PlainInput(Plain) returns (Res); rpc TwoItems(Req) returns (Ambiguous); }",
        );
        let out = emit_rust_paging(&pool, &["fixture.v1.Lists"], &module).expect("emit");
        assert!(out.contains("item: crate::fixture::v1::Req"));
        assert!(out.contains("name: list,"));
        assert!(!out.contains("watch"));
        assert!(!out.contains("upload"));
        assert!(!out.contains("plain_input"));
        assert!(!out.contains("two_items"));
    }

    #[test]
    fn refuses_enum_items_unknown_and_duplicate_services() {
        let pool = pool("Choice", "service Lists { rpc List(Req) returns (Res); }");
        assert!(
            emit_rust_paging(&pool, &["fixture.v1.Lists"], &module)
                .expect_err("enum refused")
                .to_string()
                .contains("enum page items")
        );
        assert!(
            emit_rust_paging(&pool, &["missing.Service"], &module)
                .expect_err("unknown refused")
                .to_string()
                .contains("unknown service")
        );
        let scalar_pool = self::pool("string", "service Lists { rpc List(Req) returns (Res); }");
        assert!(
            emit_rust_paging(
                &scalar_pool,
                &["fixture.v1.Lists", "fixture.v1.Lists"],
                &module
            )
            .expect_err("duplicate refused")
            .to_string()
            .contains("duplicate service name")
        );
    }

    #[test]
    fn refuses_platform_client_method_names() {
        for name in ["New", "Anonymous", "Observed", "Raw", "Clone", "__private"] {
            let pool = pool(
                "string",
                &format!("service Lists {{ rpc {name}(Req) returns (Res); }}"),
            );
            assert!(
                emit_rust_paging(&pool, &["fixture.v1.Lists"], &module)
                    .expect_err("reserved method refused")
                    .to_string()
                    .contains("reserved or duplicate list method")
            );
        }
    }

    #[test]
    fn refuses_same_service_name_in_different_packages() {
        let mut pool = pool("string", "service Lists { rpc List(Req) returns (Res); }");
        let mut file = pool
            .get_file_by_name(resolver::PATH)
            .expect("fixture file")
            .file_descriptor_proto()
            .clone();
        file.name = Some("other.proto".into());
        file.package = Some("other.v1".into());
        file.dependency.push(resolver::PATH.into());
        file.message_type.clear();
        file.enum_type.clear();
        pool.add_file_descriptor_proto(file)
            .expect("second package");
        assert!(
            emit_rust_paging(&pool, &["fixture.v1.Lists", "other.v1.Lists"], &module)
                .expect_err("same service name refused")
                .to_string()
                .contains("duplicate service name")
        );
    }

    #[test]
    fn matches_default_buffa_field_keywords_and_bytes() {
        for name in ["self", "Self", "super", "crate"] {
            assert_eq!(escape_ident(name), format!("{name}_"));
        }
        for name in ["type", "abstract", "gen"] {
            assert_eq!(escape_ident(name), format!("r#{name}"));
        }
        assert_eq!(escape_ident("_items"), "_items");
        assert_eq!(nested_module("Msg2Part"), "msg2part");
        let pool = pool("bytes", "service Lists { rpc List(Req) returns (Res); }");
        assert!(
            emit_rust_paging(&pool, &["fixture.v1.Lists"], &module)
                .expect("bytes emitted")
                .contains("item: Vec<u8>")
        );
    }
}
