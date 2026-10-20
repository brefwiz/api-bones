// SPDX-License-Identifier: MIT
//! The Rust half of `../../features/connect_csrf_replay_rule.feature`.
//!
//! The TypeScript half answers the same file; neither keeps its own copy of
//! the rows.

use crate::world::CsrfReplayWorld;
use cucumber::{given, then};

fn token(name: &str) -> Option<&str> {
    (name != "none").then_some(name)
}

#[given(
    expr = "a refusal with status {int} carrying the token {string} for a call that sent {string}"
)]
fn given_refusal(world: &mut CsrfReplayWorld, status: u16, refusal: String, sent: String) {
    world.status = status;
    world.refusal = token(&refusal).map(str::to_owned);
    world.sent = token(&sent).map(str::to_owned);
}

#[then(expr = "the call is replayed with {string}")]
fn then_replayed(world: &mut CsrfReplayWorld, expected: String) {
    let replay = api_bones_connect::csrf_replay_token(
        world.status,
        world.refusal.as_deref(),
        world.sent.as_deref(),
    );
    assert_eq!(replay.as_deref().unwrap_or("none"), expected);
}
