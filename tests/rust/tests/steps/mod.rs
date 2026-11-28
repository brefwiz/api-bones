// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! The Rust half of `../../features/connect_retry_eligibility.feature`.
//!
//! The TypeScript half answers the same file; neither keeps its own copy of
//! the rows.

mod batch;
mod csrf_replay_rule;
mod delegation_templates;
mod error_catalog_declarations;
mod error_info;
mod error_localization;
mod failure_reach;
mod field_labels;
mod fleet_connect_bound;
#[path = "../../../shared/keyed_write_contract.rs"]
mod keyed_write_contract;
mod keyed_writes;
mod overridable_defaults;
mod paged_without_batch;
mod pagination;
mod pinned_peer;
mod precondition;
pub mod rate_limit_retry;
mod retry_after;
mod root_store;
#[path = "../../../shared/shipped_resolver.rs"]
mod shipped_resolver;
mod stream_control;
mod stream_keepalive;
mod stream_rotation;
mod support;
mod tracing_fields;

use crate::world::RetryWorld;
use connectrpc::{ConnectError, ErrorCode};
use cucumber::{given, then};

fn code_of(name: &str) -> ErrorCode {
    match name {
        "internal" => ErrorCode::Internal,
        "invalid_argument" => ErrorCode::InvalidArgument,
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
        ErrorCode::InvalidArgument => "invalid_argument",
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
    let actual = api_bones_connect::is_connection_write_failure(world.failure());
    assert_eq!(actual.to_string(), expected);
}

#[then(expr = "it is retryable without server instruction: {word}")]
fn then_unprompted(world: &mut RetryWorld, expected: String) {
    let actual = api_bones_connect::is_unprompted_retryable(world.failure().code);
    assert_eq!(actual.to_string(), expected);
}

#[then(expr = "its shape permits a replay: {word}")]
fn then_replayable(world: &mut RetryWorld, expected: String) {
    let actual = api_bones_connect::is_replayable_transport_failure(world.failure());
    assert_eq!(actual.to_string(), expected);
}

#[then(expr = "it is reported to the caller as {string}")]
fn then_reported(world: &mut RetryWorld, expected: String) {
    let failure = world.failure.take().expect("no failure given");
    let recoded = api_bones_connect::connection_failure_as_unavailable(failure);
    assert_eq!(name_of(recoded.code), expected);
}
