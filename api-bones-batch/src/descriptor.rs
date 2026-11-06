// SPDX-License-Identifier: MIT
//! Declared batch method contracts from the generated file descriptor set.

use std::collections::HashMap;

use connectrpc::ConnectError;
use prost_reflect::{DescriptorPool, FieldDescriptor, Kind, MethodDescriptor};

pub(crate) type Methods = HashMap<String, String>;

pub(crate) fn batch_methods(bytes: &[u8], service: &str) -> Result<Methods, ConnectError> {
    let pool = DescriptorPool::decode(bytes).map_err(|error| {
        ConnectError::invalid_argument(format!("invalid batch client descriptor: {error}"))
    })?;
    let service = pool.get_service_by_name(service).ok_or_else(|| {
        ConnectError::invalid_argument("batch client service is missing from its descriptor")
    })?;
    let extension = pool
        .get_extension_by_name("bones.v1.batch_of")
        .ok_or_else(|| {
            ConnectError::invalid_argument("batch client descriptor has no batch policy discovery")
        })?;
    let mut methods = Methods::new();
    for method in service.methods() {
        let options = method.options();
        if !options.has_extension(&extension) {
            continue;
        }
        let relation = options.get_extension(&extension);
        let named = relation
            .as_str()
            .filter(|named| !named.is_empty())
            .ok_or_else(|| {
                ConnectError::invalid_argument(
                    "batch client descriptor has an empty batch relation",
                )
            })?;
        let single = match named.split_once('/') {
            Some((owner, single)) if owner == service.full_name() => single,
            Some(_) => {
                return Err(ConnectError::invalid_argument(
                    "batch relation names another service",
                ));
            }
            None => named,
        };
        if single == method.name()
            || !service
                .methods()
                .any(|candidate| candidate.name() == single)
        {
            return Err(ConnectError::invalid_argument(
                "batch relation must name a sibling single method",
            ));
        }
        let idempotency = options
            .get_field_by_name("idempotency_level")
            .and_then(|value| value.as_enum_number())
            .unwrap_or_default();
        if idempotency != 1 {
            return Err(ConnectError::invalid_argument(format!(
                "batch method {} must declare NO_SIDE_EFFECTS",
                method.name()
            )));
        }
        if method.is_client_streaming() || method.is_server_streaming() {
            return Err(ConnectError::invalid_argument("batch method must be unary"));
        }
        methods.insert(
            format!("{}/{}", service.full_name(), method.name()),
            format!("{}/{single}", service.full_name()),
        );
    }
    if methods.is_empty() && !service.methods().any(|method| is_unary_paging(&method)) {
        return Err(ConnectError::invalid_argument(
            "empty batch policy discovery",
        ));
    }
    Ok(methods)
}

fn carries(mut fields: impl Iterator<Item = FieldDescriptor>, message: &str) -> bool {
    fields.any(|field| matches!(field.kind(), Kind::Message(found) if found.full_name() == message))
}

/// A unary List the paged glue binds: the canonical paging shape of
/// `api_bones_automation::paging_policy` restricted to unary methods, i.e. a
/// `bones.v1.PageRequest` in, a `bones.v1.PageResponse` and exactly one
/// repeated field out.
fn is_unary_paging(method: &MethodDescriptor) -> bool {
    !method.is_client_streaming()
        && !method.is_server_streaming()
        && carries(method.input().fields(), "bones.v1.PageRequest")
        && carries(method.output().fields(), "bones.v1.PageResponse")
        && method
            .output()
            .fields()
            .filter(FieldDescriptor::is_list)
            .count()
            == 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use protox::file::{File, FileResolver, GoogleFileResolver};

    const PATH: &str = "fixture/v1/fixture.proto";

    struct Resolver(String);

    impl FileResolver for Resolver {
        fn open_file(&self, name: &str) -> Result<File, protox::Error> {
            if name == PATH {
                return File::from_source(name, &self.0);
            }
            if let Some((_, bytes)) = api_bones_protos::files().find(|(path, _)| *path == name) {
                return File::from_source(name, std::str::from_utf8(bytes).expect("utf-8 proto"));
            }
            GoogleFileResolver::new().open_file(name)
        }
    }

    fn descriptor(imports: &str, services: &str) -> Vec<u8> {
        let source = format!(
            "syntax = \"proto3\"; package fixture.v1; {imports} \
             message Req {{ bones.v1.PageRequest page = 1; }} \
             message Res {{ repeated uint32 items = 1; bones.v1.PageResponse page = 2; }} \
             message Plain {{ string id = 1; }} \
             message Two {{ repeated uint32 a = 1; repeated uint32 b = 2; bones.v1.PageResponse page = 3; }} \
             {services}"
        );
        let mut compiler = protox::Compiler::with_file_resolver(Resolver(source));
        compiler.include_imports(true);
        compiler.open_file(PATH).expect("fixture compiles");
        compiler.encode_file_descriptor_set()
    }

    const IMPORTS: &str =
        "import \"bones/v1/annotations.proto\"; import \"bones/v1/pagination.proto\";";

    fn discover(services: &str) -> Result<Methods, ConnectError> {
        batch_methods(&descriptor(IMPORTS, services), "fixture.v1.S")
    }

    fn refusal(result: Result<Methods, ConnectError>) -> String {
        result
            .expect_err("discovery is refused")
            .message
            .unwrap_or_default()
    }

    #[test]
    fn a_list_only_service_binds_with_no_batch_methods() {
        let methods = discover("service S { rpc List(Req) returns (Res); }")
            .expect("paging methods need no batch sibling");
        assert!(methods.is_empty());
    }

    #[test]
    fn a_service_with_paging_and_batch_methods_keeps_its_relations() {
        let methods = discover(
            "service S { rpc List(Req) returns (Res) { option idempotency_level = NO_SIDE_EFFECTS; } \
             rpc BatchList(Plain) returns (Plain) { option idempotency_level = NO_SIDE_EFFECTS; option (bones.v1.batch_of) = \"List\"; } }",
        )
        .expect("batch relation is read");
        assert_eq!(methods.len(), 1);
        assert_eq!(
            methods.get("fixture.v1.S/BatchList").map(String::as_str),
            Some("fixture.v1.S/List")
        );
    }

    #[test]
    fn a_service_with_neither_batch_nor_paging_methods_is_refused() {
        let message = refusal(discover(
            "service S { rpc Get(Plain) returns (Plain); rpc Stream(Req) returns (stream Res); \
             rpc Upload(stream Req) returns (Res); rpc Mismatch(Plain) returns (Res); \
             rpc Ambiguous(Req) returns (Two); }",
        ));
        assert!(
            message.contains("empty batch policy discovery"),
            "{message}"
        );
    }

    #[test]
    fn a_service_without_methods_is_refused() {
        let message = refusal(discover("service S {}"));
        assert!(
            message.contains("empty batch policy discovery"),
            "{message}"
        );
    }

    #[test]
    fn a_descriptor_without_the_batch_extension_stays_refused() {
        let bytes = descriptor(
            "import \"bones/v1/pagination.proto\";",
            "service S { rpc List(Req) returns (Res); }",
        );
        let message = refusal(batch_methods(&bytes, "fixture.v1.S"));
        assert!(message.contains("no batch policy discovery"), "{message}");
    }

    #[test]
    fn paging_does_not_excuse_a_malformed_batch_relation() {
        let message = refusal(discover(
            "service S { rpc List(Req) returns (Res); \
             rpc BatchList(Plain) returns (Plain) { option idempotency_level = IDEMPOTENT; option (bones.v1.batch_of) = \"List\"; } }",
        ));
        assert!(
            message.contains("must declare NO_SIDE_EFFECTS"),
            "{message}"
        );
        let message = refusal(discover(
            "service S { rpc List(Req) returns (Res); \
             rpc BatchList(Plain) returns (Plain) { option idempotency_level = NO_SIDE_EFFECTS; option (bones.v1.batch_of) = \"Missing\"; } }",
        ));
        assert!(message.contains("sibling single method"), "{message}");
    }

    #[test]
    fn an_unknown_service_is_refused() {
        let bytes = descriptor(IMPORTS, "service S { rpc List(Req) returns (Res); }");
        let message = refusal(batch_methods(&bytes, "fixture.v1.Other"));
        assert!(message.contains("missing from its descriptor"), "{message}");
    }
}
