//! Rate limit metadata types.
//!
//! Demonstrates `RateLimitInfo` for tracking API quota state,
//! including the `retry_after` hint for 429 responses.
//!
//! Run: `cargo run --example rate_limit`

use api_bones::RateLimitInfo;

fn main() {
    // -- Within quota --
    let ok = RateLimitInfo::new(100, 42, 1_700_000_000);
    println!("Within quota:");
    println!(
        "  limit={}, remaining={}, reset={}, exceeded={}",
        ok.limit,
        ok.remaining,
        ok.reset,
        ok.is_exceeded()
    );
    print_json("  ", &ok);

    // -- Exceeded with retry_after --
    let exceeded = RateLimitInfo::new(100, 0, 1_700_000_000).retry_after(60);
    println!("\nExceeded:");
    println!(
        "  limit={}, remaining={}, exceeded={}, retry_after={:?}",
        exceeded.limit,
        exceeded.remaining,
        exceeded.is_exceeded(),
        exceeded.retry_after
    );
    print_json("  ", &exceeded);
}

fn print_json(prefix: &str, value: &impl serde::Serialize) {
    let json = serde_json::to_string_pretty(value).expect("serialization");
    for line in json.lines() {
        println!("{prefix}{line}");
    }
}
