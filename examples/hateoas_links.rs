//! HATEOAS `Link` and `Links` types.
//!
//! Demonstrates building link collections with factory methods, custom rels,
//! method hints, and lookup via `find()`.
//!
//! Run: `cargo run --example hateoas_links`

use api_bones::links::{Link, Links};

fn main() {
    let links = Links::new()
        .push(Link::self_link("/bookings/42"))
        .push(Link::next("/bookings?page=3"))
        .push(Link::prev("/bookings?page=1"))
        .push(Link::first("/bookings?page=1"))
        .push(Link::last("/bookings?page=10"))
        .push(Link::related("/guests/7"))
        .push(Link::new("cancel", "/bookings/42/cancel").method("POST"));

    println!("Links collection ({} links):", links.len());
    for link in links.iter() {
        println!(
            "  rel={:10} href={} {}",
            link.rel,
            link.href,
            link.method
                .as_deref()
                .map_or(String::new(), |m| format!("[{m}]"))
        );
    }

    println!("\nFind 'self':   {:?}", links.find("self").map(|l| &l.href));
    println!("Find 'cancel': {:?}", links.find("cancel").map(|l| &l.href));
    println!("Find 'edit':   {:?}", links.find("edit"));

    println!("\nJSON:");
    let json = serde_json::to_string_pretty(&links).expect("serialization");
    println!("{json}");
}
