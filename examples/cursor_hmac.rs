//! Cursor encode/sign/decode/verify with the `hmac` feature.
//!
//! Demonstrates:
//! - Unsigned encode/decode roundtrip
//! - HMAC-SHA256 signed cursor encode/decode
//! - Tamper detection
//! - Using a cursor string with `KeysetPaginatedResponse`
//!
//! Run: `cargo run --example cursor_hmac --features hmac`

use api_bones::cursor::Cursor;
use api_bones::pagination::KeysetPaginatedResponse;
#[cfg(feature = "hmac")]
use base64::Engine as _;

fn main() {
    // -------------------------------------------------------------------------
    // 1. Unsigned encode/decode roundtrip
    // -------------------------------------------------------------------------
    let payload = b"user:42:created_at:2026-01-01";
    let encoded = Cursor::encode(payload);

    println!("Unsigned cursor:");
    println!("  encoded  = {encoded}");
    println!(
        "  url-safe = {}",
        encoded
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    );

    let decoded = Cursor::decode(&encoded).unwrap();
    assert_eq!(decoded, payload);
    println!("  decoded  = {:?}", std::str::from_utf8(&decoded).unwrap());

    // -------------------------------------------------------------------------
    // 2. Invalid base64 returns an error (no panic)
    // -------------------------------------------------------------------------
    let err = Cursor::decode("!!!not-base64!!!").unwrap_err();
    println!("\nInvalid base64 error: {err}");

    // -------------------------------------------------------------------------
    // 3. HMAC-signed cursor
    // -------------------------------------------------------------------------
    #[cfg(feature = "hmac")]
    {
        let key = b"super-secret-signing-key";
        let payload = b"user:42";

        let signed = Cursor::encode_signed(payload, key);
        println!("\nSigned cursor:");
        println!("  encoded  = {signed}");

        // Verify and decode
        let verified = Cursor::decode_signed(&signed, key).unwrap();
        assert_eq!(verified, payload);
        println!("  decoded  = {:?}", std::str::from_utf8(&verified).unwrap());

        // Wrong key → signature mismatch
        let err = Cursor::decode_signed(&signed, b"wrong-key").unwrap_err();
        println!("  wrong key error: {err}");

        // Tampered payload → signature mismatch
        let mut raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(&signed)
            .unwrap();
        raw[0] ^= 0xFF;
        let tampered = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&raw);
        let err = Cursor::decode_signed(&tampered, key).unwrap_err();
        println!("  tampered error:  {err}");
    }

    #[cfg(not(feature = "hmac"))]
    println!("\n[hmac feature not enabled — run with --features hmac to see signed cursor demo]");

    // -------------------------------------------------------------------------
    // 4. Use a cursor string with KeysetPaginatedResponse
    // -------------------------------------------------------------------------
    let items = vec!["order-1".to_string(), "order-2".to_string()];
    let next_cursor = Cursor::encode(b"order-2:created_at:2026-01-02");

    // first_page: no previous cursor, has_next = true
    let page = KeysetPaginatedResponse::first_page(items, true, Some(next_cursor.clone()));

    println!("\nKeysetPaginatedResponse:");
    println!("  items       = {:?}", page.items);
    println!("  has_next    = {}", page.has_next);
    println!("  has_prev    = {}", page.has_prev);
    println!("  next_cursor = {:?}", page.next_cursor);
    assert_eq!(page.next_cursor.as_deref(), Some(next_cursor.as_str()));
}
