//! Demonstrates the `connect` feature: proto adapter primitives for Connect RPC services.
//!
//! Run with:
//!   cargo run --example `connect_adapter` --features connect

#[cfg(feature = "connect")]
fn main() {
    use api_bones::connect::{
        ConnectOptionExt as _, build_page, chrono_opt_to_timestamp, chrono_to_timestamp, parse_uuid,
    };

    // ── Timestamp conversion ───────────────────────────────────────────────
    let now = chrono::Utc::now();
    let ts = chrono_to_timestamp(now);
    println!("Timestamp: seconds={}, nanos={}", ts.seconds, ts.nanos);

    let opt_ts = chrono_opt_to_timestamp(Some(now));
    assert!(opt_ts.is_some());
    let none_ts = chrono_opt_to_timestamp(None);
    assert!(none_ts.is_none());

    // ── UUID parsing ───────────────────────────────────────────────────────
    let id = parse_uuid("550e8400-e29b-41d4-a716-446655440000", "org_id")
        .expect("valid UUID should parse");
    println!("Parsed UUID: {id}");

    let err = parse_uuid("not-a-uuid", "org_id").unwrap_err();
    println!("Parse error (expected): {err:?}");

    // ── Offset pagination ──────────────────────────────────────────────────
    // First page of 20, 100 total items.
    let page = build_page(0, 0, 20, 100);
    println!(
        "Page: limit={}, offset={}, total={}, has_more={}",
        page.limit(),
        page.offset(),
        page.total_count(),
        page.has_more(),
    );
    assert_eq!(page.limit(), 20);
    assert!(page.has_more());

    // Last page.
    let last = build_page(20, 80, 20, 100);
    assert!(!last.has_more());

    // Destructure into proto response fields.
    let (total_count, has_more, limit, offset) = page.into_parts();
    println!("parts: total={total_count}, has_more={has_more}, limit={limit}, offset={offset}");

    // ── ConnectOptionExt ───────────────────────────────────────────────────
    let found: Result<Option<&str>, connectrpc::ConnectError> = Ok(Some("hello"));
    assert_eq!(found.or_not_found("missing").unwrap(), "hello");

    let missing: Result<Option<&str>, connectrpc::ConnectError> = Ok(None);
    assert!(missing.or_not_found("record not found").is_err());

    println!("All connect adapter primitives OK");
}

#[cfg(not(feature = "connect"))]
fn main() {
    eprintln!("Run with --features connect");
}
