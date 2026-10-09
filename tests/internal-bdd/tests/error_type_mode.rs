// SPDX-License-Identifier: MIT
//! Runs `features/error_type_mode.feature`.
//!
//! The error type URI mode is library code no declared SDK surface exposes --
//! callers reach it directly (SPEC.md `internal_behavior_owners`). The mode is
//! process-wide, so scenarios run one at a time in file order, the default
//! scenario first.

use api_bones::error::{ErrorTypeMode, error_type_mode, set_error_type_mode};
use cucumber::{World, given, then, when};

#[derive(Default, World)]
struct ModeWorld {
    uri: String,
}

impl std::fmt::Debug for ModeWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModeWorld").finish()
    }
}

#[given(expr = "the application sets the URN namespace {string}")]
fn given_namespace(_world: &mut ModeWorld, namespace: String) {
    set_error_type_mode(ErrorTypeMode::Urn { namespace });
}

#[given(expr = "the application sets the base URL {string}")]
fn given_base_url(_world: &mut ModeWorld, base_url: String) {
    set_error_type_mode(ErrorTypeMode::Url { base_url });
}

#[when(expr = "the error type URI for {string} is rendered")]
fn when_rendered(world: &mut ModeWorld, slug: String) {
    world.uri = error_type_mode().render(&slug);
}

#[then(expr = "the error type URI is {string}")]
fn then_uri(world: &mut ModeWorld, expected: String) {
    assert_eq!(world.uri, expected);
}

#[tokio::main]
async fn main() {
    let features = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/features/error_type_mode.feature"
    );
    ModeWorld::cucumber()
        .max_concurrent_scenarios(1)
        .run_and_exit(features)
        .await;
}
