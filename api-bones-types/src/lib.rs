// SPDX-License-Identifier: MIT
//! One combined JSON Schema (draft-07) for the public `api-bones` serde types.
//!
//! Every type lands in the `definitions` map of a single root schema, so a type
//! shared by several others is described once. Generic types are described with
//! `serde_json::Value` as their type parameter, which TypeScript renders as
//! `unknown`. Definition names carry no monomorphization suffix, and a `$ref`
//! stands alone, since draft-07 ignores keys beside it.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::Path,
};

use api_bones::{
    ApiError, ApiResponse, ApiVersion, AuditInfo, BackoffStrategy, BulkItemResult, BulkRequest,
    BulkResponse, CacheControl, CorrelationId, CursorPaginatedResponse, CursorPagination,
    CursorPaginationParams, Deprecated, ETag, ErrorArg, ErrorCode, ErrorInfoMember, FilterEntry,
    FilterOp, FilterParams, HealthCheck, HealthStatus, IdempotencyKey, IfMatch, IfNoneMatch,
    KeysetPaginatedResponse, KeysetPaginationParams, Link, Links, LivenessResponse, OrgId,
    PaginatedResponse, PaginationParams, Principal, PrincipalId, PrincipalKind, RateLimitInfo,
    ReadinessResponse, RequestId, ResponseMeta, RetryPolicy, SamplingFlags, SearchParams,
    SemverTriple, Slug, SortDirection, SortParams, SpanId, StatusCode, TraceContext, TraceId,
    ValidationError,
};
use schemars::generate::{SchemaGenerator, SchemaSettings};
use serde_json::{Map, Value, json};

const DEFINITIONS_PREFIX: &str = "#/definitions/";
const GENERIC_MARKER: &str = "_for_";

macro_rules! registered_types {
    ($($ty:ty => $name:literal),* $(,)?) => {
        /// Public name of every type registered in the schema, in registration order.
        pub const REGISTERED: &[&str] = &[$($name),*];

        /// Registers every type in `generator`'s definitions; the returned `$ref`
        /// of each is not needed.
        fn register_types(generator: &mut SchemaGenerator) {
            $( let _ = generator.subschema_for::<$ty>(); )*
        }
    };
}

registered_types!(
    ApiError => "ApiError",
    ValidationError => "ValidationError",
    ErrorCode => "ErrorCode",
    ErrorArg => "ErrorArg",
    ErrorInfoMember => "ErrorInfoMember",
    PaginationParams => "PaginationParams",
    PaginatedResponse<Value> => "PaginatedResponse",
    CursorPagination => "CursorPagination",
    CursorPaginationParams => "CursorPaginationParams",
    CursorPaginatedResponse<Value> => "CursorPaginatedResponse",
    KeysetPaginationParams<Value> => "KeysetPaginationParams",
    KeysetPaginatedResponse<Value> => "KeysetPaginatedResponse",
    HealthStatus => "HealthStatus",
    HealthCheck => "HealthCheck",
    LivenessResponse => "LivenessResponse",
    ReadinessResponse => "ReadinessResponse",
    SortDirection => "SortDirection",
    SortParams => "SortParams",
    FilterOp => "FilterOp",
    FilterEntry => "FilterEntry",
    FilterParams => "FilterParams",
    SearchParams => "SearchParams",
    ApiResponse<Value> => "ApiResponse",
    ResponseMeta => "ResponseMeta",
    Link => "Link",
    Links => "Links",
    RateLimitInfo => "RateLimitInfo",
    ETag => "ETag",
    IfMatch => "IfMatch",
    IfNoneMatch => "IfNoneMatch",
    BulkRequest<Value> => "BulkRequest",
    BulkResponse<Value> => "BulkResponse",
    BulkItemResult<Value> => "BulkItemResult",
    AuditInfo => "AuditInfo",
    Slug => "Slug",
    BackoffStrategy => "BackoffStrategy",
    RetryPolicy => "RetryPolicy",
    RequestId => "RequestId",
    CorrelationId => "CorrelationId",
    IdempotencyKey => "IdempotencyKey",
    TraceContext => "TraceContext",
    TraceId => "TraceId",
    SpanId => "SpanId",
    SamplingFlags => "SamplingFlags",
    Deprecated => "Deprecated",
    CacheControl => "CacheControl",
    ApiVersion => "ApiVersion",
    SemverTriple => "SemverTriple",
    StatusCode => "StatusCode",
    PrincipalId => "PrincipalId",
    Principal => "Principal",
    PrincipalKind => "PrincipalKind",
    OrgId => "OrgId",
);

/// Builds the combined schema: a root object whose properties reference every
/// definition, so a TypeScript generator that emits only reachable definitions
/// emits all of them.
///
/// # Panics
///
/// Panics when two definitions share a name once their generic suffix is
/// removed, because one would silently replace the other.
#[must_use]
pub fn build_schema() -> Value {
    let mut generator = SchemaSettings::draft07()
        .with(|settings| settings.inline_subschemas = false)
        .into_generator();

    register_types(&mut generator);

    let definitions: BTreeMap<String, Value> =
        generator.take_definitions(false).into_iter().collect();
    let renames = generic_renames(definitions.keys());

    let mut public_definitions = Map::new();
    for (name, mut schema) in definitions {
        normalize(&mut schema, &renames);
        let public_name = renames.get(&name).cloned().unwrap_or(name);
        public_definitions.insert(public_name, schema);
    }

    let properties: Map<String, Value> = public_definitions
        .keys()
        .map(|name| {
            (
                name.clone(),
                json!({ "$ref": format!("{DEFINITIONS_PREFIX}{name}") }),
            )
        })
        .collect();

    json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "title": "ApiBones",
        "type": "object",
        "properties": properties,
        "definitions": public_definitions,
    })
}

/// Writes [`build_schema`] to `path` as pretty JSON, creating parent directories.
///
/// # Errors
///
/// Returns the I/O error from creating the directory or writing the file.
pub fn write_schema(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let pretty = serde_json::to_string_pretty(&build_schema())?;
    fs::write(path, format!("{pretty}\n"))
}

/// Maps each monomorphized definition name (`Page_for_Value`) to its plain
/// name (`Page`), asserting the plain names are unique across all definitions.
fn generic_renames<'a>(names: impl Iterator<Item = &'a String>) -> BTreeMap<String, String> {
    let mut renames = BTreeMap::new();
    let mut public_names = BTreeSet::new();
    for name in names {
        let public_name = match name.split_once(GENERIC_MARKER) {
            Some((base, args)) if !base.is_empty() && !args.is_empty() => {
                renames.insert(name.clone(), base.to_owned());
                base
            }
            _ => name.as_str(),
        };
        assert!(
            public_names.insert(public_name.to_owned()),
            "definition name `{public_name}` is produced by more than one type"
        );
    }
    renames
}

/// Rewrites `$ref` targets through `renames` and drops every key that sits
/// beside a `$ref`.
fn normalize(value: &mut Value, renames: &BTreeMap<String, String>) {
    match value {
        Value::String(text) => {
            let target = text
                .strip_prefix(DEFINITIONS_PREFIX)
                .and_then(|name| renames.get(name))
                .map(|name| format!("{DEFINITIONS_PREFIX}{name}"));
            if let Some(target) = target {
                *text = target;
            }
        }
        Value::Array(items) => {
            for item in items {
                normalize(item, renames);
            }
        }
        Value::Object(map) => {
            if map.contains_key("$ref") {
                map.retain(|key, _| key == "$ref");
            }
            for child in map.values_mut() {
                normalize(child, renames);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs, path::PathBuf};

    use serde_json::{Value, json};

    use super::{build_schema, generic_renames, normalize, write_schema};

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("api-bones-schema-gen-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create scratch directory");
        dir
    }

    fn page_rename() -> BTreeMap<String, String> {
        BTreeMap::from([("Page_for_Value".to_owned(), "Page".to_owned())])
    }

    #[test]
    fn normalize_rewrites_a_ref_and_drops_its_siblings() {
        let mut schema = json!({ "$ref": "#/definitions/Page_for_Value", "description": "x" });
        normalize(&mut schema, &page_rename());
        assert_eq!(schema, json!({ "$ref": "#/definitions/Page" }));
    }

    #[test]
    fn normalize_rewrites_refs_in_nested_positions() {
        let reference = || json!({ "$ref": "#/definitions/Page_for_Value", "description": "x" });
        let mut schema = json!({
            "properties": { "page": reference() },
            "items": [reference()],
            "anyOf": [reference(), { "type": "null" }],
        });
        normalize(&mut schema, &page_rename());
        let target = json!({ "$ref": "#/definitions/Page" });
        assert_eq!(schema["properties"]["page"], target);
        assert_eq!(schema["items"][0], target);
        assert_eq!(schema["anyOf"][0], target);
        assert_eq!(schema["anyOf"][1], json!({ "type": "null" }));
    }

    #[test]
    fn normalize_leaves_unrenamed_refs_and_plain_values_alone() {
        let mut schema =
            json!({ "a": { "$ref": "#/definitions/Other" }, "b": "text", "c": [1, true, null] });
        let expected = schema.clone();
        normalize(&mut schema, &page_rename());
        assert_eq!(schema, expected);
    }

    #[test]
    fn write_schema_creates_parents_and_writes_the_schema() {
        let out = scratch("writes").join("nested").join("schema.json");
        write_schema(&out).expect("schema written");
        let parsed: Value =
            serde_json::from_str(&fs::read_to_string(&out).expect("read")).expect("json");
        assert_eq!(parsed, build_schema());
    }

    #[test]
    fn write_schema_fails_when_the_parent_is_a_file() {
        let blocker = scratch("blocked").join("blocker");
        fs::write(&blocker, "a file").expect("write blocker");
        assert!(write_schema(&blocker.join("schema.json")).is_err());
    }

    #[test]
    fn write_schema_fails_when_the_target_is_a_directory() {
        let dir = scratch("target-dir");
        assert!(write_schema(&dir).is_err());
    }

    fn renames(names: &[&str]) -> std::collections::BTreeMap<String, String> {
        let owned: Vec<String> = names.iter().map(|name| (*name).to_owned()).collect();
        generic_renames(owned.iter())
    }

    #[test]
    fn a_generic_suffix_is_stripped_and_plain_names_are_untouched() {
        let map = renames(&["Page_for_Value", "Plain"]);
        assert_eq!(map.len(), 1);
        assert_eq!(map["Page_for_Value"], "Page");
    }

    #[test]
    fn a_marker_without_a_base_or_arguments_is_not_a_generic() {
        assert!(renames(&["_for_Value", "Page_for_"]).is_empty());
    }

    #[test]
    #[should_panic(expected = "definition name `Page` is produced by more than one type")]
    fn a_plain_name_colliding_with_a_stripped_name_panics() {
        renames(&["Page", "Page_for_Value"]);
    }

    #[test]
    #[should_panic(expected = "definition name `Page` is produced by more than one type")]
    fn two_instantiations_of_one_generic_panic() {
        renames(&["Page_for_Value", "Page_for_String"]);
    }
}
