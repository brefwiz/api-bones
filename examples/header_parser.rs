// SPDX-License-Identifier: MIT
//! Non-axum header parsing — for webhook verifiers and out-of-band tooling.
//!
//! Handlers served by axum routers must consume [`OrganizationContext`] (see
//! `examples/org_context.rs`). This example demonstrates the sanctioned path
//! for callers that have no `AuthLayer` and only need to parse a well-formed
//! `X-Org-Id` header.
//!
//! Run: `cargo run --example header_parser --features http`

use api_bones::{OrgId, OrgIdHeaderError};
use http::HeaderMap;

fn verify_webhook(headers: &HeaderMap) -> Result<OrgId, OrgIdHeaderError> {
    OrgId::try_from_headers(headers)
}

fn main() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-org-id",
        "550e8400-e29b-41d4-a716-446655440000".parse().unwrap(),
    );

    match verify_webhook(&headers) {
        Ok(org_id) => println!("parsed org_id: {org_id}"),
        Err(e) => eprintln!("rejected: {e}"),
    }

    // Rejection path: missing header
    let empty = HeaderMap::new();
    match verify_webhook(&empty) {
        Ok(_) => unreachable!(),
        Err(e) => println!("missing-header rejection: {e}"),
    }
}
