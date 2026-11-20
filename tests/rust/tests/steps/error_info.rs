// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! The Rust half of `../../features/connect_error_info.feature`.
//!
//! The TypeScript half answers the same file; neither keeps its own copy of
//! the rows.

use crate::world::{ErrorInfoWorld, ReadError};
use api_bones::connect::{
    CarriesErrorInfo as _, ErrorArg, ErrorArgError, ErrorInfo, ValidationFailureDetail, error_info,
    try_with_error_info, violations_of, with_error_info,
};
use connectrpc::{ConnectError, ErrorDetail};
use cucumber::{given, then, when};

const BASE_MESSAGE: &str = "refused";

#[given(expr = "a refusal {string} whose ErrorInfo detail has code {string} and emitter {string}")]
fn given_refusal(world: &mut ErrorInfoWorld, connect_code: String, code: String, emitter: String) {
    let info = ErrorInfo {
        code,
        emitter,
        ..Default::default()
    };
    let failure = ConnectError::new(super::code_of(&connect_code), BASE_MESSAGE);
    world.failure = Some(with_error_info(failure, &info));
}

#[given(expr = "a refusal {string} whose ErrorInfo detail has the wire value {string}")]
fn given_wire(world: &mut ErrorInfoWorld, connect_code: String, wire: String) {
    let detail = ErrorDetail {
        type_url: "bones.v1.ErrorInfo".into(),
        value: Some(wire),
        debug: None,
    };
    world.failure =
        Some(ConnectError::new(super::code_of(&connect_code), BASE_MESSAGE).with_detail(detail));
}

#[given(expr = "a refusal {string} with no ErrorInfo detail")]
fn given_bare(world: &mut ErrorInfoWorld, connect_code: String) {
    world.failure = Some(ConnectError::new(
        super::code_of(&connect_code),
        BASE_MESSAGE,
    ));
}

#[when(expr = "the refusal is read as an SDK error")]
fn when_read(world: &mut ErrorInfoWorld) {
    let failure = world.failure.as_ref().expect("no refusal given");
    world.read = Some(ReadError {
        code: failure.code(),
        emitter: failure.emitter(),
        text: failure.display_with_token(BASE_MESSAGE),
        args: error_info(failure)
            .map(|info| info.args)
            .unwrap_or_default(),
        violations: Vec::new(),
    });
}

fn expected(value: &str) -> Option<&str> {
    (value != "none").then_some(value)
}

#[then(expr = "its error code is {string}")]
fn then_code(world: &mut ErrorInfoWorld, value: String) {
    assert_eq!(
        world
            .read
            .as_ref()
            .expect("the refusal was not read")
            .code
            .as_deref(),
        expected(&value)
    );
}

#[then(expr = "its emitter is {string}")]
fn then_emitter(world: &mut ErrorInfoWorld, value: String) {
    assert_eq!(
        world
            .read
            .as_ref()
            .expect("the refusal was not read")
            .emitter
            .as_deref(),
        expected(&value)
    );
}

#[then(expr = "its text ends with {string}")]
fn then_text(world: &mut ErrorInfoWorld, value: String) {
    let text = &world.read.as_ref().expect("the refusal was not read").text;
    match expected(&value) {
        Some(token) => assert!(text.ends_with(token), "text: {text}"),
        None => assert!(!text.contains("[bones-error"), "text: {text}"),
    }
}

#[given(
    expr = "a refusal {string} whose ErrorInfo detail has code {string} and emitter {string} and arguments {string}"
)]
fn given_refusal_with_args(
    world: &mut ErrorInfoWorld,
    connect_code: String,
    code: String,
    emitter: String,
    spec: String,
) {
    let mut info = ErrorInfo {
        code,
        emitter,
        ..Default::default()
    };
    for (name, arg) in parse_args(&spec) {
        info = info.with_arg(name, arg).expect("arguments within bounds");
    }
    let failure = ConnectError::new(super::code_of(&connect_code), BASE_MESSAGE);
    world.failure = Some(with_error_info(failure, &info));
}

/// `name=type:value` items separated by `;`. `repeat:c*N` is a string of `N`
/// copies of `c`.
fn parse_args(spec: &str) -> Vec<(String, ErrorArg)> {
    spec.split(';')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(|item| {
            let (name, typed) = item.split_once('=').expect("name=type:value");
            let (kind, value) = typed.split_once(':').expect("type:value");
            let arg = match kind {
                "string" => ErrorArg::String(value.to_owned()),
                "repeat" => {
                    let (unit, count) = value.rsplit_once('*').expect("char*count");
                    ErrorArg::String(unit.repeat(count.parse().expect("repeat count")))
                }
                "int64" => ErrorArg::Int64(value.parse().expect("int64")),
                "double" => ErrorArg::Double(value.parse().expect("double")),
                "timestamp" => {
                    let (seconds, nanos) = value.split_once('/').expect("seconds/nanos");
                    ErrorArg::Timestamp {
                        seconds: seconds.parse().expect("seconds"),
                        nanos: nanos.parse().expect("nanos"),
                    }
                }
                other => panic!("unknown argument type {other}"),
            };
            (name.to_owned(), arg)
        })
        .collect()
}

fn describe(arg: &ErrorArg) -> String {
    match arg {
        ErrorArg::String(value) => format!("string:{value}"),
        ErrorArg::Int64(value) => format!("int64:{value}"),
        ErrorArg::Double(value) => format!("double:{value}"),
        ErrorArg::Timestamp { seconds, nanos } => format!("timestamp:{seconds}/{nanos}"),
    }
}

fn reason(error: &ErrorArgError) -> &'static str {
    match error {
        ErrorArgError::TooMany { .. } => "too-many",
        ErrorArgError::InvalidKey { .. } => "invalid-key",
        ErrorArgError::StringTooLong { .. } => "string-too-long",
        ErrorArgError::NonFiniteDouble { .. } => "non-finite-double",
        ErrorArgError::InvalidTimestamp { .. } => "invalid-timestamp",
        other => panic!("unexpected bound: {other}"),
    }
}

fn refusal() -> ConnectError {
    ConnectError::new(super::code_of("internal"), BASE_MESSAGE)
}

fn outcome(attached: Result<ConnectError, ErrorArgError>) -> String {
    attached.map_or_else(|error| reason(&error).to_owned(), |_| "accepted".to_owned())
}

#[when(expr = "an ErrorInfo with code {string} is built with arguments {string}")]
fn when_built(world: &mut ErrorInfoWorld, code: String, spec: String) {
    let info = ErrorInfo {
        code,
        args: parse_args(&spec).into_iter().collect(),
        ..Default::default()
    };
    world.built = Some(outcome(try_with_error_info(refusal(), &info)));
}

#[when(expr = "an ErrorInfo with code {string} is built with {int} numbered arguments")]
fn when_built_numbered(world: &mut ErrorInfoWorld, code: String, count: usize) {
    let info = ErrorInfo {
        code,
        args: (0..count)
            .map(|i| (format!("k{i}"), ErrorArg::Int64(1)))
            .collect(),
        ..Default::default()
    };
    world.built = Some(outcome(try_with_error_info(refusal(), &info)));
}

#[then(expr = "its argument {string} is {string}")]
fn then_argument(world: &mut ErrorInfoWorld, name: String, value: String) {
    let read = world.read.as_ref().expect("the refusal was not read");
    assert_eq!(
        read.args.get(&name).map(describe).as_deref(),
        expected(&value)
    );
}

#[then(expr = "it carries {int} arguments")]
fn then_argument_count(world: &mut ErrorInfoWorld, count: usize) {
    let read = world.read.as_ref().expect("the refusal was not read");
    assert_eq!(read.args.len(), count);
}

#[then(expr = "its text is {string}")]
fn then_text_is(world: &mut ErrorInfoWorld, value: String) {
    let read = world.read.as_ref().expect("the refusal was not read");
    assert_eq!(read.text, value);
}

#[then(expr = "building it ends as {string}")]
fn then_built(world: &mut ErrorInfoWorld, outcome: String) {
    assert_eq!(world.built.as_deref(), Some(outcome.as_str()));
}

#[given(expr = "the refusal has a violation at {string} for rule {string} and arguments {string}")]
fn given_violation(world: &mut ErrorInfoWorld, field: String, rule: String, spec: String) {
    let mut violation = ValidationFailureDetail {
        field,
        rule,
        ..Default::default()
    };
    for (name, arg) in parse_args(&spec) {
        violation = violation
            .with_arg(name, arg)
            .expect("arguments within bounds");
    }
    let failure = world.failure.clone().expect("no refusal given");
    world.failure = Some(failure.with_detail(violation.to_detail().expect("within bounds")));
}

#[given(expr = "the refusal has a ValidationFailure detail with the wire value {string}")]
fn given_violation_wire(world: &mut ErrorInfoWorld, wire: String) {
    let detail = ErrorDetail {
        type_url: "bones.v1.ValidationFailure".into(),
        value: Some(wire),
        debug: None,
    };
    let failure = world.failure.clone().expect("no refusal given");
    world.failure = Some(failure.with_detail(detail));
}

/// The arguments read are those of the first violation.
#[when(expr = "the violations of the refusal are read")]
fn when_violations_read(world: &mut ErrorInfoWorld) {
    let failure = world.failure.as_ref().expect("no refusal given");
    let violations = violations_of(failure);
    world.read = Some(ReadError {
        code: failure.code(),
        emitter: failure.emitter(),
        text: failure.display_with_token(BASE_MESSAGE),
        args: violations
            .first()
            .map(|violation| violation.args.clone())
            .unwrap_or_default(),
        violations,
    });
}

#[then(expr = "it carries {int} violations")]
fn then_violation_count(world: &mut ErrorInfoWorld, count: usize) {
    let read = world.read.as_ref().expect("the violations were not read");
    assert_eq!(read.violations.len(), count);
}

#[then(expr = "violation {int} is at {string} for rule {string}")]
fn then_violation_at(world: &mut ErrorInfoWorld, index: usize, field: String, rule: String) {
    let read = world.read.as_ref().expect("the violations were not read");
    let actual = read
        .violations
        .get(index - 1)
        .map(|violation| (violation.field.clone(), violation.rule.clone()));
    assert_eq!(actual, Some((field, rule)));
}
