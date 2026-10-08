// SPDX-License-Identifier: MIT
//! Runs `features/connect_code.feature`.
//!
//! `ConnectCode` is library code no declared SDK surface exposes -- callers
//! reach it directly (SPEC.md `internal_behavior_owners`).

use api_bones::connect::ConnectCode;
use cucumber::{World, given, then, when};

#[derive(Default, World)]
struct CodeWorld {
    body: String,
    code: Option<ConnectCode>,
}

impl std::fmt::Debug for CodeWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodeWorld").finish()
    }
}

#[given(regex = r"^a Connect error body (.*)$")]
fn given_body(world: &mut CodeWorld, body: String) {
    world.body = body;
}

#[given(expr = "the wire code {string}")]
fn given_wire(world: &mut CodeWorld, wire: String) {
    world.code = Some(ConnectCode::from_wire(&wire).expect("the contract names a protocol code"));
}

#[when("the code is read from the body")]
fn when_read(world: &mut CodeWorld) {
    world.code = ConnectCode::from_error_body(world.body.as_bytes());
}

#[when("the code is converted to the transport code and back")]
fn when_converted(world: &mut CodeWorld) {
    let code = world.code.expect("no code given");
    let transport: connectrpc::ErrorCode = code.into();
    world.code = Some(ConnectCode::from(transport));
}

#[then(expr = "the code is {string}")]
fn then_code(world: &mut CodeWorld, expected: String) {
    let actual = world.code.map_or("none", |c| c.as_wire());
    assert_eq!(actual, expected);
}

#[tokio::main]
async fn main() {
    let features = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/features/connect_code.feature"
    );
    CodeWorld::run(features).await;
}
