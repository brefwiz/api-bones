//! `ETag` creation and conditional request checks (RFC 7232).
//!
//! Demonstrates strong/weak `ETag` creation, `If-Match` and `If-None-Match`
//! conditional header evaluation.
//!
//! Run: `cargo run --example etag_conditional`

use api_bones::{ETag, IfMatch, IfNoneMatch};

fn main() {
    // -- ETag creation --
    let strong = ETag::strong("v1-abc123");
    let weak = ETag::weak("v1-abc123");
    println!("Strong ETag: {strong}");
    println!("Weak   ETag: {weak}");

    // -- Strong comparison: both must be strong with same value --
    println!("\n--- Strong comparison ---");
    println!(
        "strong == strong same value: {}",
        strong.matches(&ETag::strong("v1-abc123"))
    );
    println!(
        "strong == strong diff value: {}",
        strong.matches(&ETag::strong("v2-xyz"))
    );
    println!("strong == weak same value:   {}", strong.matches(&weak));

    // -- Weak comparison: values match regardless of strength --
    println!("\n--- Weak comparison ---");
    println!("strong ~= weak same value: {}", strong.matches_weak(&weak));
    println!(
        "strong ~= weak diff value: {}",
        strong.matches_weak(&ETag::weak("other"))
    );

    // -- If-Match: used by PUT/PATCH to prevent lost updates --
    println!("\n--- If-Match (optimistic concurrency) ---");
    let if_match_any = IfMatch::Any;
    let if_match = IfMatch::Tags(vec![ETag::strong("v1-abc123"), ETag::strong("v1-def")]);
    println!(
        "IfMatch::Any matches:            {}",
        if_match_any.matches(&strong)
    );
    println!(
        "IfMatch::Tags matches current:   {}",
        if_match.matches(&strong)
    );
    println!(
        "IfMatch::Tags matches stale:     {}",
        if_match.matches(&ETag::strong("v0-old"))
    );

    // Simulate PUT precondition check
    let server_etag = ETag::strong("v1-abc123");
    let client_condition = IfMatch::Tags(vec![ETag::strong("v1-abc123")]);
    if client_condition.matches(&server_etag) {
        println!("Precondition met — safe to update resource.");
    } else {
        println!("Precondition Failed (412) — resource has changed.");
    }

    // -- If-None-Match: used by GET to check for fresh cached copy --
    println!("\n--- If-None-Match (conditional GET) ---");
    let if_none_match = IfNoneMatch::Tags(vec![ETag::strong("v1-abc123")]);
    // matches() returns true when condition is SATISFIED (i.e., resource changed → 200)
    // matches() returns false when condition is NOT satisfied (resource unchanged → 304)
    println!(
        "IfNoneMatch satisfied (same tag → 304): {}",
        if_none_match.matches(&strong)
    );
    println!(
        "IfNoneMatch satisfied (new  tag → 200): {}",
        if_none_match.matches(&ETag::strong("v2-new"))
    );
    println!(
        "IfNoneMatch::Any satisfied:             {}",
        IfNoneMatch::Any.matches(&strong)
    );

    // Simulate GET conditional check: client sends its cached ETag
    let cached_etag = ETag::strong("v1-abc123");
    let condition = IfNoneMatch::Tags(vec![cached_etag]);
    if condition.matches(&server_etag) {
        println!("OK (200) — return fresh response.");
    } else {
        println!("Not Modified (304) — serve from cache.");
    }

    println!("\nAll etag_conditional examples passed.");
}
