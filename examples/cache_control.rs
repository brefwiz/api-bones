//! `Cache-Control` header builder and parser (RFC 7234).
//!
//! Demonstrates building `CacheControl` directives and parsing them from a
//! header string.
//!
//! Run: `cargo run --example cache_control`

use api_bones::CacheControl;

fn main() {
    // -- Build a typical immutable public asset response --
    let cc = CacheControl::new()
        .public()
        .max_age(31_536_000) // 1 year
        .immutable();
    println!("Static asset: {cc}");
    assert_eq!(cc.to_string(), "public, immutable, max-age=31536000");

    // -- Private API response: no caching --
    let cc = CacheControl::private_no_cache();
    println!("Private API:  {cc}");
    assert!(cc.private && cc.no_cache && cc.no_store);

    // -- Short-lived public response with stale-while-revalidate --
    let cc = CacheControl::new()
        .public()
        .max_age(60)
        .stale_while_revalidate(30)
        .stale_if_error(86_400);
    println!("SWR response: {cc}");

    // -- Request directive: only serve from cache --
    let cc = CacheControl::new().only_if_cached().max_stale(300);
    println!("Offline req:  {cc}");

    // -- Parse from a header string --
    let cc: CacheControl = "public, max-age=3600, must-revalidate"
        .parse()
        .expect("valid header");
    println!(
        "\nParsed: public={} max_age={:?} must_revalidate={}",
        cc.public, cc.max_age, cc.must_revalidate
    );
    assert!(cc.public);
    assert_eq!(cc.max_age, Some(3600));
    assert!(cc.must_revalidate);

    // -- Parse no-store --
    let cc: CacheControl = "no-store".parse().expect("valid header");
    assert!(cc.no_store);
    println!("no-store parsed: no_store={}", cc.no_store);

    println!("\nAll cache_control examples passed.");
}
