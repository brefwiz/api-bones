//! `Range` request and `Content-Range` response headers (RFC 7233).
//!
//! Demonstrates parsing `Range` request headers, validating ranges against
//! a resource length, and building `Content-Range` response headers.
//!
//! Run: `cargo run --example range_headers`

use api_bones::{ByteRange, ContentRange, RangeHeader};

fn main() {
    let resource_len: u64 = 10_000;

    // -- Parse Range request headers --
    println!("--- Parsing Range headers ---");
    let range: RangeHeader = "bytes=0-499".parse().expect("valid range");
    println!("Parsed: {range}");
    assert_eq!(range, RangeHeader::Bytes(vec![ByteRange::FromTo(0, 499)]));

    let range: RangeHeader = "bytes=500-".parse().expect("valid range");
    println!("Parsed open-ended: {range}");
    assert_eq!(range, RangeHeader::Bytes(vec![ByteRange::From(500)]));

    let range: RangeHeader = "bytes=-200".parse().expect("valid range");
    println!("Parsed suffix: {range}");
    assert_eq!(range, RangeHeader::Bytes(vec![ByteRange::Suffix(200)]));

    let range: RangeHeader = "bytes=0-999,2000-2999".parse().expect("valid range");
    println!("Parsed multi-range: {range}");

    // -- Validate ranges against resource length --
    println!("\n--- Range validation (resource = {resource_len} bytes) ---");
    let valid = ByteRange::FromTo(0, 499);
    println!("bytes=0-499   valid: {}", valid.is_valid(resource_len));

    let invalid_start = ByteRange::FromTo(10_000, 10_999);
    println!(
        "bytes=10000-10999 valid: {}",
        invalid_start.is_valid(resource_len)
    );

    let inverted = ByteRange::FromTo(500, 100);
    println!("bytes=500-100 valid: {}", inverted.is_valid(resource_len));

    let suffix = ByteRange::Suffix(200);
    println!("bytes=-200    valid: {}", suffix.is_valid(resource_len));

    // -- Resolve ranges to concrete byte positions --
    println!("\n--- Range resolution ---");
    let r = ByteRange::FromTo(0, 499);
    println!("bytes=0-499  resolved: {:?}", r.resolve(resource_len));

    let r = ByteRange::From(9_500);
    println!("bytes=9500-  resolved: {:?}", r.resolve(resource_len));

    let r = ByteRange::Suffix(500);
    println!("bytes=-500   resolved: {:?}", r.resolve(resource_len));

    // -- Build Content-Range response headers --
    println!("\n--- Content-Range responses ---");
    let cr = ContentRange::bytes(0, 499, Some(resource_len));
    println!("Partial response:  {cr}");
    assert_eq!(cr.to_string(), "bytes 0-499/10000");

    let cr = ContentRange::bytes(9_500, 9_999, Some(resource_len));
    println!("Tail response:     {cr}");

    // Unknown total length (streaming)
    let cr = ContentRange::bytes(0, 499, None);
    println!("Streaming (unknown total): {cr}");
    assert_eq!(cr.to_string(), "bytes 0-499/*");

    // Unsatisfiable range (416 response)
    let cr = ContentRange::unsatisfiable(resource_len);
    println!("Unsatisfiable:     {cr}");
    assert_eq!(cr.to_string(), "bytes */10000");

    println!("\nAll range_headers examples passed.");
}
