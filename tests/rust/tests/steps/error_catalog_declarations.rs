// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! The steps of `../../../features/error_catalog_declarations.feature`.
//!
//! The error enum is compiled against the shipped `bones/v1` files, its
//! declarations are read back through `api-bones-protos`, and the
//! `error-catalog` check of `api-bones-i18n` runs over the catalog strings.

use std::collections::BTreeMap;

use api_bones_i18n::{ArgDeclaration, ArgKind, check_catalog};
use api_bones_protos::error_args::{ErrorArgKind, declared_error_args};
use cucumber::{given, then, when};

use super::shipped_resolver::{CONTRACT, compiler};
use crate::world::ErrorCatalogWorld;

const ENUM: &str = "contract.v1.ErrorCode";

fn source(values: &[(String, Vec<String>)]) -> String {
    let members: String = values
        .iter()
        .enumerate()
        .map(|(index, (value, arguments))| {
            let declarations: Vec<String> = arguments
                .iter()
                .map(|argument| {
                    let (name, kind) = argument.split_once(':').expect("name:kind");
                    format!(
                        "(bones.v1.error_args) = {{ name: \"{name}\" kind: ERROR_ARG_KIND_{} }}",
                        kind.to_uppercase()
                    )
                })
                .collect();
            let options = if declarations.is_empty() {
                String::new()
            } else {
                format!(" [{}]", declarations.join(", "))
            };
            format!("  {value} = {}{options};\n", index + 1)
        })
        .collect();
    format!(
        "syntax = \"proto3\";\npackage contract.v1;\n\
         import \"bones/v1/annotations.proto\";\n\
         enum ErrorCode {{\n  ERROR_CODE_UNSPECIFIED = 0;\n{members}}}\n"
    )
}

fn declared_by_the_descriptor(world: &ErrorCatalogWorld) -> BTreeMap<String, Vec<ArgDeclaration>> {
    let mut compiler = compiler(source(&world.values), api_bones_protos::files());
    compiler
        .open_file(CONTRACT)
        .unwrap_or_else(|e| panic!("contract failed to compile: {e}"));
    let pool = compiler.descriptor_pool();
    let error_enum = pool.get_enum_by_name(ENUM).expect("the error enum");
    world
        .values
        .iter()
        .map(|(name, _)| {
            let value = error_enum
                .get_value_by_name(name)
                .expect("a declared value");
            let declared = declared_error_args(&value)
                .unwrap_or_else(|e| panic!("the declaration is refused: {e}"))
                .into_iter()
                .map(|decl| ArgDeclaration {
                    name: decl.name,
                    kind: match decl.kind {
                        ErrorArgKind::String => ArgKind::String,
                        ErrorArgKind::Int64 => ArgKind::Int64,
                        ErrorArgKind::Double => ArgKind::Double,
                        ErrorArgKind::Timestamp => ArgKind::Timestamp,
                    },
                })
                .collect();
            (name.clone(), declared)
        })
        .collect()
}

#[given(
    expr = "the emitter {string} has an error enum value {string} declaring no error arguments"
)]
fn given_value_without_arguments(world: &mut ErrorCatalogWorld, emitter: String, value: String) {
    world.emitter = emitter;
    world.values.push((value, Vec::new()));
}

#[given(
    expr = "the emitter {string} has an error enum value {string} declaring the error arguments {string}"
)]
fn given_value_with_arguments(
    world: &mut ErrorCatalogWorld,
    emitter: String,
    value: String,
    arguments: String,
) {
    world.emitter = emitter;
    world
        .values
        .push((value, arguments.split(", ").map(str::to_owned).collect()));
}

#[given(expr = "the catalog string {string} is {string}")]
fn given_string(world: &mut ErrorCatalogWorld, key: String, text: String) {
    world.strings.insert(key, text);
}

#[when("the error-catalog check runs over the catalog")]
fn when_checked(world: &mut ErrorCatalogWorld) {
    let codes = declared_by_the_descriptor(world);
    let catalog = serde_json::to_string(&world.strings).expect("a catalog is JSON");
    world.outcome = Some(check_catalog(&world.emitter, &catalog, &codes));
}

#[then("the error-catalog check passes")]
fn then_passes(world: &mut ErrorCatalogWorld) {
    let outcome = world.outcome.as_ref().expect("the check did not run");
    assert_eq!(outcome, &Ok(()));
}

#[then(expr = "the error-catalog check fails at {string} with {string}")]
fn then_fails(world: &mut ErrorCatalogWorld, key: String, problem: String) {
    let outcome = world.outcome.as_ref().expect("the check did not run");
    let issues = outcome.as_ref().expect_err("the check passed");
    assert!(
        issues
            .iter()
            .any(|issue| issue.key == key && issue.problem.to_string().contains(&problem)),
        "no issue at `{key}` says `{problem}`: {issues:?}"
    );
}
