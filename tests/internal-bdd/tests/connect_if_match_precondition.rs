// SPDX-License-Identifier: MIT
//! Runs `features/connect_if_match_precondition.feature`.
//!
//! `check_if_match`/`etag_from_updated_at` are Connect adapter code no
//! declared SDK surface exposes -- callers reach them directly as library
//! code, not through a generated contract (SPEC.md `internal_behavior_owners`).

use api_bones::connect::{check_if_match, etag_from_updated_at};
use chrono::{DateTime, TimeZone as _, Utc};
use connectrpc::{ConnectError, RequestContext};
use cucumber::{World, given, then, when};
use http::HeaderMap;

fn timestamp(millis: i64) -> DateTime<Utc> {
    Utc.timestamp_millis_opt(millis).unwrap()
}

#[derive(Default)]
struct PendingHeader {
    value: Option<String>,
    set: bool,
}

#[derive(Default, World)]
struct EtagWorld {
    current: Option<api_bones::etag::ETag>,
    header: PendingHeader,
    result: Option<Result<(), ConnectError>>,
}

impl std::fmt::Debug for EtagWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EtagWorld").finish()
    }
}

#[given(expr = "a current ETag derived from updated_at {int}")]
fn given_current_etag(world: &mut EtagWorld, millis: i64) {
    world.current = Some(etag_from_updated_at(timestamp(millis)));
}

#[given(expr = "no If-Match header on the request")]
fn given_no_header(world: &mut EtagWorld) {
    world.header = PendingHeader::default();
}

#[given(expr = "an If-Match header of {string}")]
fn given_header_value(world: &mut EtagWorld, value: String) {
    world.header = PendingHeader {
        value: Some(value),
        set: true,
    };
}

#[given(expr = "an If-Match header equal to the current ETag")]
fn given_header_equal_to_current(world: &mut EtagWorld) {
    let current = world
        .current
        .as_ref()
        .expect("current ETag not set")
        .to_string();
    world.header = PendingHeader {
        value: Some(current),
        set: true,
    };
}

#[given(expr = "an If-Match header equal to the current ETag's value, strongly quoted")]
fn given_header_strong_quoted(world: &mut EtagWorld) {
    let current = world.current.as_ref().expect("current ETag not set");
    world.header = PendingHeader {
        value: Some(format!("\"{}\"", current.value)),
        set: true,
    };
}

#[given(expr = "an If-Match header equal to the ETag derived from updated_at {int}")]
fn given_header_equal_to_other(world: &mut EtagWorld, millis: i64) {
    let other = etag_from_updated_at(timestamp(millis));
    world.header = PendingHeader {
        value: Some(other.to_string()),
        set: true,
    };
}

#[when(expr = "the If-Match precondition is checked")]
fn when_checked(world: &mut EtagWorld) {
    let mut headers = HeaderMap::new();
    if world.header.set {
        let value = world.header.value.clone().unwrap_or_default();
        headers.insert("if-match", value.parse().unwrap());
    }
    let ctx = RequestContext::new(headers);
    let current = world.current.as_ref().expect("current ETag not set");
    world.result = Some(check_if_match(&ctx, current));
}

#[then(expr = "the check succeeds")]
fn then_succeeds(world: &mut EtagWorld) {
    world
        .result
        .take()
        .expect("check not run")
        .expect("expected the precondition check to succeed");
}

#[then(expr = "the check fails with failed_precondition")]
fn then_fails_failed_precondition(world: &mut EtagWorld) {
    let err = world.result.take().expect("check not run").unwrap_err();
    assert_eq!(err.code, connectrpc::ErrorCode::FailedPrecondition);
}

#[then(expr = "the check fails with aborted")]
fn then_fails_aborted(world: &mut EtagWorld) {
    let err = world.result.take().expect("check not run").unwrap_err();
    assert_eq!(err.code, connectrpc::ErrorCode::Aborted);
}

#[then(expr = "the check fails with invalid_argument")]
fn then_fails_invalid_argument(world: &mut EtagWorld) {
    let err = world.result.take().expect("check not run").unwrap_err();
    assert_eq!(err.code, connectrpc::ErrorCode::InvalidArgument);
}

#[tokio::main]
async fn main() {
    let features = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/features/connect_if_match_precondition.feature"
    );
    EtagWorld::run(features).await;
}
