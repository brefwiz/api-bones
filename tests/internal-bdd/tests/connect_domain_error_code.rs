// SPDX-License-Identifier: MIT
//! Runs `features/connect_domain_error_code.feature`.
//!
//! `domain_to_connect` is Connect adapter code no declared SDK surface
//! exposes -- callers reach it directly as library code (SPEC.md
//! `internal_behavior_owners`).

use api_bones::connect::{
    CarriesErrorInfo as _, DomainErrorKind, IntoDomainErrorKind, domain_to_connect,
};
use connectrpc::{ConnectError, ErrorCode};
use cucumber::{World, given, then, when};

struct Domain {
    kind: String,
    code: Option<&'static str>,
}

impl IntoDomainErrorKind for Domain {
    fn kind(&self) -> DomainErrorKind {
        match self.kind.as_str() {
            "not_found" => DomainErrorKind::NotFound,
            "conflict" => DomainErrorKind::Conflict("conflict".into()),
            "internal" => DomainErrorKind::Internal("secret detail".into()),
            other => panic!("the contract names a kind this step cannot build: {other}"),
        }
    }

    fn code(&self) -> Option<&'static str> {
        self.code
    }
}

#[derive(Default, World)]
struct DomainWorld {
    domain: Option<Domain>,
    mapped: Option<ConnectError>,
}

impl std::fmt::Debug for DomainWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DomainWorld").finish()
    }
}

impl DomainWorld {
    fn mapped(&self) -> &ConnectError {
        self.mapped.as_ref().expect("no mapped error yet")
    }
}

fn static_code(code: &str) -> &'static str {
    match code {
        "GRANT_MISSING" => "GRANT_MISSING",
        other => panic!("the contract names a code this step cannot build: {other}"),
    }
}

#[given(expr = "a domain error of kind {string} with error code {string}")]
fn given_coded(world: &mut DomainWorld, kind: String, code: String) {
    world.domain = Some(Domain {
        kind,
        code: Some(static_code(&code)),
    });
}

#[given(expr = "a domain error of kind {string} with no error code")]
fn given_uncoded(world: &mut DomainWorld, kind: String) {
    world.domain = Some(Domain { kind, code: None });
}

#[when(expr = "the domain error is mapped to a Connect error")]
fn when_mapped(world: &mut DomainWorld) {
    let domain = world.domain.as_ref().expect("no domain error given");
    world.mapped = Some(domain_to_connect(domain));
}

#[then(expr = "the Connect error has code {string}")]
fn then_code(world: &mut DomainWorld, expected: String) {
    let actual = match world.mapped().code {
        ErrorCode::NotFound => "not_found",
        ErrorCode::AlreadyExists => "already_exists",
        ErrorCode::Internal => "internal",
        other => panic!("the implementation produced a code the contract does not name: {other:?}"),
    };
    assert_eq!(actual, expected);
}

#[then(expr = "the Connect error carries error code {string}")]
fn then_error_code(world: &mut DomainWorld, expected: String) {
    assert_eq!(world.mapped().code().as_deref(), Some(expected.as_str()));
}

#[then(expr = "the Connect error carries no error code")]
fn then_no_error_code(world: &mut DomainWorld) {
    assert_eq!(world.mapped().code(), None);
}

#[then(expr = "the Connect error carries no emitter")]
fn then_no_emitter(world: &mut DomainWorld) {
    assert_eq!(world.mapped().emitter(), None);
}

#[then(expr = "the Connect error message is {string}")]
fn then_message(world: &mut DomainWorld, expected: String) {
    assert_eq!(world.mapped().message.as_deref(), Some(expected.as_str()));
}

#[tokio::main]
async fn main() {
    let features = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/features/connect_domain_error_code.feature"
    );
    DomainWorld::run(features).await;
}
