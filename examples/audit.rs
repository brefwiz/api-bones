//! Audit metadata for API resources.
//!
//! Demonstrates `AuditInfo` for tracking creation and update timestamps
//! with optional actor references.
//!
//! Run: `cargo run --example audit`

use api_bones::AuditInfo;

fn main() {
    // -- Create with an actor --
    let mut audit = AuditInfo::now(Some("alice".into()));
    println!("Created:  {} by {:?}", audit.created_at, audit.created_by);
    println!("Updated:  {} by {:?}", audit.updated_at, audit.updated_by);

    // -- Touch (update) with a different actor --
    audit.touch(Some("bob".into()));
    println!(
        "\nAfter touch: {} by {:?}",
        audit.updated_at, audit.updated_by
    );

    // -- Touch without actor clears updated_by --
    audit.touch(None);
    println!(
        "After anonymous touch: {} by {:?}",
        audit.updated_at, audit.updated_by
    );

    // -- JSON representation --
    println!("\nJSON:");
    let json = serde_json::to_string_pretty(&audit).expect("serialization");
    println!("{json}");
}
