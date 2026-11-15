// SPDX-License-Identifier: MIT
//! Contract of the generated schema: the names the npm package promises,
//! well-formed references, and a stable, reproducible file.

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use api_bones_schema_gen::{REGISTERED, build_schema};
use serde_json::{Value, json};

const DEFINITIONS_PREFIX: &str = "#/definitions/";

/// Definitions that exist only because a registered type references them. The
/// schema holds exactly the registered names plus these. `Duration` is the std
/// type referenced by `BackoffStrategy`.
const TRANSITIVE_EXTRAS: &[&str] = &["Duration"];

/// Registered types that are generic; each is described with `Value` as its
/// type parameter under its plain name.
const GENERIC_TYPES: &[&str] = &[
    "PaginatedResponse",
    "CursorPaginatedResponse",
    "KeysetPaginationParams",
    "KeysetPaginatedResponse",
    "ApiResponse",
    "BulkRequest",
    "BulkResponse",
    "BulkItemResult",
];

fn definitions(schema: &Value) -> &serde_json::Map<String, Value> {
    schema["definitions"]
        .as_object()
        .expect("schema has a definitions object")
}

fn collect_refs(value: &Value, refs: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(target)) = map.get("$ref") {
                assert_eq!(map.len(), 1, "`$ref` to {target} has sibling keys");
                refs.push(target.clone());
            }
            for child in map.values() {
                collect_refs(child, refs);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_refs(item, refs);
            }
        }
        _ => {}
    }
}

fn target_name(reference: &str) -> &str {
    reference
        .strip_prefix(DEFINITIONS_PREFIX)
        .unwrap_or_else(|| panic!("`{reference}` is not a definitions reference"))
}

#[test]
fn definitions_are_exactly_the_registered_types_and_their_extras() {
    let schema = build_schema();
    let defined: BTreeSet<&str> = definitions(&schema).keys().map(String::as_str).collect();
    let expected: BTreeSet<&str> = REGISTERED
        .iter()
        .chain(TRANSITIVE_EXTRAS)
        .copied()
        .collect();
    assert_eq!(
        REGISTERED.len(),
        REGISTERED.iter().collect::<BTreeSet<_>>().len(),
        "a type is registered twice"
    );
    let missing: Vec<&&str> = expected.difference(&defined).collect();
    let unexpected: Vec<&&str> = defined.difference(&expected).collect();
    assert!(missing.is_empty(), "definitions missing: {missing:?}");
    assert!(
        unexpected.is_empty(),
        "unregistered definitions: {unexpected:?}"
    );
}

#[test]
fn duration_is_an_object_of_integer_secs_and_nanos() {
    let schema = build_schema();
    let duration = &definitions(&schema)["Duration"];
    assert_eq!(duration["type"], json!("object"));
    for field in ["secs", "nanos"] {
        assert_eq!(duration["properties"][field]["type"], json!("integer"));
    }
    let mut required: Vec<&str> = duration["required"]
        .as_array()
        .expect("Duration lists required fields")
        .iter()
        .map(|name| name.as_str().expect("required name is a string"))
        .collect();
    required.sort_unstable();
    assert_eq!(required, ["nanos", "secs"]);
}

#[test]
fn principal_id_is_a_string_definition_that_principal_references() {
    let schema = build_schema();
    let defined = definitions(&schema);
    assert_eq!(defined["PrincipalId"]["type"], json!("string"));
    assert_eq!(
        defined["Principal"]["properties"]["id"],
        json!({ "$ref": "#/definitions/PrincipalId" })
    );
}

#[test]
fn no_definition_keeps_a_generic_suffix() {
    let schema = build_schema();
    let suffixed: Vec<&String> = definitions(&schema)
        .keys()
        .filter(|name| name.contains("_for_"))
        .collect();
    assert!(suffixed.is_empty(), "suffixed definitions: {suffixed:?}");
}

#[test]
fn every_generic_type_is_registered_and_defined_under_its_plain_name() {
    let schema = build_schema();
    let defined = definitions(&schema);
    for name in GENERIC_TYPES {
        assert!(REGISTERED.contains(name), "{name} is not registered");
        assert!(defined.contains_key(*name), "{name} is not defined");
        assert!(
            !defined
                .keys()
                .any(|key| key.starts_with(&format!("{name}_for_"))),
            "{name} kept a generic suffix"
        );
    }
}

#[test]
fn every_ref_resolves_and_stands_alone() {
    let schema = build_schema();
    let defined = definitions(&schema);
    let mut refs = Vec::new();
    collect_refs(&schema, &mut refs);
    assert!(!refs.is_empty(), "schema contains no references");
    for reference in &refs {
        let name = target_name(reference);
        assert!(defined.contains_key(name), "dangling reference {reference}");
    }
}

#[test]
fn root_reaches_every_definition() {
    let schema = build_schema();
    let defined = definitions(&schema);
    let properties = schema["properties"]
        .as_object()
        .expect("schema has a root properties object");
    let reached: BTreeSet<&str> = properties
        .values()
        .map(|property| {
            target_name(
                property["$ref"]
                    .as_str()
                    .expect("root property is a `$ref`"),
            )
        })
        .collect();
    let all: BTreeSet<&str> = defined.keys().map(String::as_str).collect();
    assert_eq!(reached, all);
}

#[test]
fn output_is_deterministic() {
    assert_eq!(build_schema().to_string(), build_schema().to_string());
}

fn run_binary(out: &Path) -> std::process::ExitStatus {
    Command::new(env!("CARGO_BIN_EXE_api-bones-schema-gen"))
        .arg(out)
        .status()
        .expect("run api-bones-schema-gen")
}

fn scratch_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create scratch directory");
    dir
}

#[test]
fn binary_writes_the_schema_to_the_given_path() {
    let dir = scratch_dir("binary-writes");
    let out = dir.join("nested").join("schema.json");

    assert!(run_binary(&out).success());

    let written = fs::read_to_string(&out).expect("schema file written");
    assert!(written.ends_with('\n'));
    let parsed: Value = serde_json::from_str(&written).expect("written schema is JSON");
    assert_eq!(parsed, build_schema());
}

#[test]
fn two_binary_runs_write_identical_bytes() {
    let dir = scratch_dir("binary-twice");
    let first = dir.join("first.json");
    let second = dir.join("second.json");

    assert!(run_binary(&first).success());
    assert!(run_binary(&second).success());

    assert_eq!(
        fs::read(&first).expect("first schema"),
        fs::read(&second).expect("second schema")
    );
}

#[test]
fn binary_fails_when_the_path_cannot_be_written() {
    let dir = scratch_dir("binary-unwritable");
    let blocker = dir.join("blocker");
    fs::write(&blocker, "a file, not a directory").expect("write blocker");

    let status = run_binary(&blocker.join("schema.json"));

    assert_eq!(status.code(), Some(1));
}

#[test]
fn binary_refuses_a_missing_path() {
    let status = Command::new(env!("CARGO_BIN_EXE_api-bones-schema-gen"))
        .status()
        .expect("run api-bones-schema-gen");
    assert_eq!(status.code(), Some(2));
}
