// SPDX-License-Identifier: MIT
//! Runs `features/connect_client_headers.feature`.
//!
//! `ConnectConfigExt::with_bearer` is transport code the crate's own callers
//! use directly -- no declared SDK surface exposes it (SPEC.md
//! `sdk_surfaces`) -- so its contract is proven here via SPEC.md
//! `internal_behavior_owners` instead.

use api_bones::connect::ConnectConfigExt;
use connectrpc::client::ClientConfig;
use cucumber::{World, given, then, when};

fn base_uri() -> http::Uri {
    "http://localhost:8080".parse().expect("valid URI")
}

#[derive(Debug, Default, World)]
struct HeaderWorld {
    config: Option<ClientConfig>,
}

impl HeaderWorld {
    fn config(&self) -> &ClientConfig {
        self.config.as_ref().expect("no client config built yet")
    }
}

#[given(expr = "a fresh client config")]
fn given_fresh(world: &mut HeaderWorld) {
    world.config = Some(ClientConfig::new(base_uri()));
}

#[given(expr = "a client config already carrying an authorization header of {string}")]
fn given_existing_authorization(world: &mut HeaderWorld, value: String) {
    world.config = Some(ClientConfig::new(base_uri()).with_default_header("authorization", value));
}

#[given(expr = "the config carries an {string} header of {string}")]
fn given_extra_header(world: &mut HeaderWorld, name: String, value: String) {
    let config = world.config.take().expect("no client config built yet");
    world.config = Some(config.with_default_header(name, value));
}

#[when(expr = "the bearer token is set to {string}")]
fn when_with_bearer(world: &mut HeaderWorld, token: String) {
    let config = world.config.take().expect("no client config built yet");
    world.config = Some(config.with_bearer(&token));
}

#[then(expr = "the config carries exactly one authorization header, {string}")]
fn then_single_authorization_header(world: &mut HeaderWorld, expected: String) {
    let headers = world.config().default_headers();
    assert_eq!(
        headers.get_all("authorization").iter().count(),
        1,
        "expected exactly one authorization header"
    );
    let actual = headers.get("authorization").and_then(|v| v.to_str().ok());
    assert_eq!(actual, Some(expected.as_str()));
}

#[then(expr = "the config still carries an {string} header of {string}")]
fn then_still_carries_header(world: &mut HeaderWorld, name: String, expected: String) {
    let headers = world.config().default_headers();
    let actual = headers.get(&name).and_then(|v| v.to_str().ok());
    assert_eq!(actual, Some(expected.as_str()));
}

#[tokio::main]
async fn main() {
    let features = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/features/connect_client_headers.feature"
    );
    HeaderWorld::run(features).await;
}
