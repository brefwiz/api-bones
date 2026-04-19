//! Audit metadata for API resources.
//!
//! Demonstrates `AuditInfo` for tracking creation and update timestamps
//! with canonical [`Principal`] actor identities.
//!
//! Run: `cargo run --example audit`

use api_bones::{AuditInfo, Principal, PrincipalId, PrincipalKind};
use uuid::Uuid;

fn main() {
    // -- Create with a human principal (UUID-backed, no PII) --
    let alice_id = Uuid::parse_str("a1a1a1a1-a1a1-4a1a-a1a1-a1a1a1a1a1a1").unwrap();
    let mut audit = AuditInfo::now(Principal::human(alice_id));
    println!("Created:  {} by {:?}", audit.created_at, audit.created_by);
    println!("Updated:  {} by {:?}", audit.updated_at, audit.updated_by);

    // -- Touch (update) with a different human principal --
    let bob_id = Uuid::parse_str("b2b2b2b2-b2b2-4b2b-b2b2-b2b2b2b2b2b2").unwrap();
    audit.touch(Principal::human(bob_id));
    println!(
        "\nAfter touch: {} by {:?}",
        audit.updated_at, audit.updated_by
    );

    // -- Touch with a system principal (const, zero-alloc) --
    audit.touch(Principal::system("sealwiz.rotation-engine"));
    println!(
        "After system touch: {} by {:?}",
        audit.updated_at, audit.updated_by
    );

    // -- Inspect PrincipalId and PrincipalKind directly --
    let p = Principal::system("sealwiz.rotation-engine");
    let id: &PrincipalId = &p.id;
    let kind: &PrincipalKind = &p.kind;
    println!("\nPrincipalId: {id}  kind: {kind:?}");
    // Build a principal from an owned string (e.g. a DB round-trip)
    let from_db = Principal {
        id: PrincipalId::from_owned("some-legacy-id".to_string()),
        kind: PrincipalKind::Service,
        org_path: Vec::new(),
    };
    println!("From DB: {:?}", from_db.id);

    // -- JSON representation --
    println!("\nJSON:");
    let json = serde_json::to_string_pretty(&audit).expect("serialization");
    println!("{json}");
}
