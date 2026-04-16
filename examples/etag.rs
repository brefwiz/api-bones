//! `ETag` and conditional request types (RFC 7232).
//!
//! Demonstrates `ETag` (strong/weak), `IfMatch`, and `IfNoneMatch`
//! with strong and weak comparison semantics.
//!
//! Run: `cargo run --example etag`

use api_bones::{ETag, IfMatch, IfNoneMatch};

fn main() {
    // -- Strong vs weak ETags --
    let strong = ETag::strong("abc123");
    let weak = ETag::weak("abc123");
    println!("Strong ETag: {strong}");
    println!("Weak   ETag: {weak}");

    println!("\n--- Strong comparison (both must be strong + same value) ---");
    println!(
        "strong.matches(strong 'abc123'): {}",
        strong.matches(&ETag::strong("abc123"))
    );
    println!(
        "strong.matches(strong 'xyz'):    {}",
        strong.matches(&ETag::strong("xyz"))
    );
    println!("strong.matches(weak 'abc123'):   {}", strong.matches(&weak));

    println!("\n--- Weak comparison (values match regardless of strength) ---");
    println!(
        "strong.matches_weak(weak 'abc123'): {}",
        strong.matches_weak(&weak)
    );
    println!(
        "strong.matches_weak(weak 'xyz'):    {}",
        strong.matches_weak(&ETag::weak("xyz"))
    );

    // -- If-Match header --
    println!("\n--- If-Match ---");
    let if_match_any = IfMatch::Any;
    let if_match_tags = IfMatch::Tags(vec![ETag::strong("abc123"), ETag::strong("def456")]);
    println!(
        "IfMatch::Any matches strong:   {}",
        if_match_any.matches(&strong)
    );
    println!(
        "IfMatch::Tags matches strong:  {}",
        if_match_tags.matches(&strong)
    );
    println!(
        "IfMatch::Tags matches unknown: {}",
        if_match_tags.matches(&ETag::strong("unknown"))
    );

    // -- If-None-Match header --
    println!("\n--- If-None-Match ---");
    let if_none_match = IfNoneMatch::Tags(vec![ETag::strong("abc123")]);
    println!(
        "IfNoneMatch::Tags matches (unknown): {}",
        if_none_match.matches(&ETag::strong("unknown"))
    );
    println!(
        "IfNoneMatch::Tags matches (known):   {}",
        if_none_match.matches(&strong)
    );
    println!(
        "IfNoneMatch::Any matches:            {}",
        IfNoneMatch::Any.matches(&strong)
    );
}
