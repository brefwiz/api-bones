//! Audit metadata for API resources.
//!
//! Demonstrates `AuditInfo` for tracking creation and update timestamps
//! with canonical [`Principal`] actor identities.
//!
//! Run: `cargo run --example audit`

use api_bones::{AuditInfo, Principal};

fn main() {
    // -- Create with a user principal --
    let mut audit = AuditInfo::now(Principal::new("alice"));
    println!("Created:  {} by {:?}", audit.created_at, audit.created_by);
    println!("Updated:  {} by {:?}", audit.updated_at, audit.updated_by);

    // -- Touch (update) with a different user principal --
    audit.touch(Principal::new("bob"));
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

    // -- JSON representation --
    println!("\nJSON:");
    let json = serde_json::to_string_pretty(&audit).expect("serialization");
    println!("{json}");
}
