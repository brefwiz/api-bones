// SPDX-License-Identifier: MIT
//! The Rust half of `../../features/connect_error_info.feature`.
//!
//! The TypeScript half answers the same file; neither keeps its own copy of
//! the rows.

use crate::world::{ErrorInfoWorld, ReadError};
use api_bones_connect::{CarriesErrorInfo as _, ErrorInfo, with_error_info};
use connectrpc::{ConnectError, ErrorDetail};
use cucumber::{given, then, when};

const BASE_MESSAGE: &str = "refused";

#[given(expr = "a refusal {string} whose ErrorInfo detail has code {string} and emitter {string}")]
fn given_refusal(world: &mut ErrorInfoWorld, connect_code: String, code: String, emitter: String) {
    let info = ErrorInfo { code, emitter };
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
    });
}

fn read(world: &ErrorInfoWorld) -> &ReadError {
    world.read.as_ref().expect("the refusal was not read")
}

fn expected(value: &str) -> Option<&str> {
    (value != "none").then_some(value)
}

#[then(expr = "its error code is {string}")]
fn then_code(world: &mut ErrorInfoWorld, value: String) {
    assert_eq!(read(world).code.as_deref(), expected(&value));
}

#[then(expr = "its emitter is {string}")]
fn then_emitter(world: &mut ErrorInfoWorld, value: String) {
    assert_eq!(read(world).emitter.as_deref(), expected(&value));
}

#[then(expr = "its text ends with {string}")]
fn then_text(world: &mut ErrorInfoWorld, value: String) {
    let text = &read(world).text;
    match expected(&value) {
        Some(token) => assert!(text.ends_with(token), "text: {text}"),
        None => assert!(!text.contains("[bones-error"), "text: {text}"),
    }
}
