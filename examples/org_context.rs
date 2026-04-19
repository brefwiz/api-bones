// SPDX-License-Identifier: LicenseRef-Proprietary
//! Cross-cutting platform context bundle.
//!
//! Demonstrates constructing an [`OrganizationContext`] that combines
//! tenant, principal, request-id, roles, and an optional opaque attestation.
//!
//! Run: `cargo run --example org_context`

use api_bones::{
    Attestation, AttestationKind, OrgId, OrganizationContext, Principal, RequestId, Role,
};
use uuid::Uuid;

fn main() {
    let org_id = OrgId::new(Uuid::parse_str("a0a0a0a0-a0a0-4a0a-a0a0-a0a0a0a0a0a0").unwrap());
    let principal = Principal::human(Uuid::parse_str("b1b1b1b1-b1b1-4b1b-b1b1-b1b1b1b1b1b1").unwrap());
    let request_id = RequestId::new();

    // Basic construction — roles empty, no attestation
    let ctx = OrganizationContext::new(org_id, principal.clone(), request_id);
    println!("org_id:     {}", ctx.org_id);
    println!("principal:  {}", ctx.principal);
    println!("request_id: {}", ctx.request_id);
    println!("roles:      {:?}", ctx.roles);
    println!("attest:     {:?}", ctx.attestation);

    // Builder — add roles and a JWT attestation
    let ctx = OrganizationContext::new(org_id, principal, request_id)
        .with_roles(vec![Role::from("admin"), Role::from("billing-viewer")])
        .with_attestation(Attestation {
            kind: AttestationKind::Jwt,
            raw: b"<opaque-jwt-bytes>".to_vec(),
        });

    println!("\nWith roles and attestation:");
    println!("roles:  {:?}", ctx.roles.iter().map(|r| r.as_str()).collect::<Vec<_>>());
    println!("kind:   {:?}", ctx.attestation.as_ref().unwrap().kind);
}
