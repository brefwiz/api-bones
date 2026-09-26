// SPDX-License-Identifier: MIT
//! The Rust half of `../../features/connect_precondition.feature`.
//!
//! The TypeScript half answers the same file; neither keeps its own copy of
//! the rows.

use api_bones_connect::Idempotency;
use cucumber::{given, then, when};

use crate::world::PreconditionWorld;

fn idempotency_of(name: &str) -> Idempotency {
    match name {
        "NO_SIDE_EFFECTS" => Idempotency::NoSideEffects,
        "IDEMPOTENT" => Idempotency::Idempotent,
        "UNSPECIFIED" => Idempotency::Unspecified,
        other => panic!("the contract names an idempotency this step cannot build: {other}"),
    }
}

#[given(expr = "a unary method declared {string}")]
fn given_declared(world: &mut PreconditionWorld, idempotency: String) {
    world.declare_method(idempotency_of(&idempotency));
}

#[given(expr = "a method with no policy entry at all")]
fn given_no_policy(world: &mut PreconditionWorld) {
    world.declare_no_policy();
}

#[given(expr = "a call to that method carrying {string} as its own if-match header")]
fn given_caller_header(world: &mut PreconditionWorld, value: String) {
    world.set_caller_header(&value);
}

#[when(expr = "the call is sent")]
async fn when_sent(world: &mut PreconditionWorld) {
    world.send().await;
}

#[then(expr = "the call carries {string} as its if-match header")]
fn then_sent_header(world: &mut PreconditionWorld, expected: String) {
    let actual = world.sent_if_match().unwrap_or_else(|| "none".to_owned());
    assert_eq!(actual, expected);
}
