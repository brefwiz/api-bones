// SPDX-License-Identifier: MIT
//! The Rust half of `../../features/connect_retry_eligibility.feature`.
//!
//! The TypeScript half answers the same file; neither keeps its own copy of
//! the rows.

use api_bones_connect::{
    connection_failure_as_unavailable, is_connection_write_failure,
    is_replayable_transport_failure, is_unprompted_retryable,
};
use connectrpc::{ConnectError, ErrorCode};
use cucumber::{given, then};

fn code_of(name: &str) -> ErrorCode {
    match name {
        "internal" => ErrorCode::Internal,
        "unavailable" => ErrorCode::Unavailable,
        "unauthenticated" => ErrorCode::Unauthenticated,
        "aborted" => ErrorCode::Aborted,
        "resource_exhausted" => ErrorCode::ResourceExhausted,
        "permission_denied" => ErrorCode::PermissionDenied,
        other => panic!("the contract names a code this step cannot build: {other}"),
    }
}

fn name_of(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::Internal => "internal",
        ErrorCode::Unavailable => "unavailable",
        ErrorCode::Unauthenticated => "unauthenticated",
        ErrorCode::Aborted => "aborted",
        ErrorCode::ResourceExhausted => "resource_exhausted",
        ErrorCode::PermissionDenied => "permission_denied",
        other => panic!("the implementation produced a code the contract does not name: {other:?}"),
    }
}

#[given(expr = "a Connect failure with code {string} and message {string}")]
fn given_failure(world: &mut RetryWorld, code: String, message: String) {
    world.failure = Some(ConnectError::new(code_of(&code), message));
}

#[then(expr = "it is a connection write failure: {word}")]
fn then_write_failure(world: &mut RetryWorld, expected: String) {
    assert_eq!(
        is_connection_write_failure(world.failure()).to_string(),
        expected
    );
}

#[then(expr = "it is retryable without server instruction: {word}")]
fn then_unprompted(world: &mut RetryWorld, expected: String) {
    assert_eq!(
        is_unprompted_retryable(world.failure().code).to_string(),
        expected
    );
}

#[then(expr = "its shape permits a replay: {word}")]
fn then_replayable(world: &mut RetryWorld, expected: String) {
    assert_eq!(
        is_replayable_transport_failure(world.failure()).to_string(),
        expected
    );
}

#[then(expr = "it is reported to the caller as {string}")]
fn then_reported(world: &mut RetryWorld, expected: String) {
    let failure = world.failure.take().expect("no failure given");
    assert_eq!(
        name_of(connection_failure_as_unavailable(failure).code),
        expected
    );
}
