//! Demonstrates `QueryBuilder` — type-safe query string construction.
//!
//! Run with:
//! ```bash
//! cargo run --example query_builder
//! ```

use api_bones::url::QueryBuilder;
use serde::Serialize;

#[derive(Serialize)]
struct SearchParams {
    q: String,
    page: u32,
    per_page: u32,
    /// `None` fields are skipped automatically.
    filter: Option<String>,
}

fn main() {
    // Basic key=value pairs via .set()
    let qs = QueryBuilder::new()
        .set("limit", 20u32)
        .set("sort", "desc")
        .build();
    println!("Basic:           {qs}");
    assert_eq!(qs, "limit=20&sort=desc");

    // Skip None with .set_opt()
    let qs = QueryBuilder::new()
        .set("page", 1u32)
        .set_opt("cursor", None::<&str>)
        .set_opt("after", Some("abc123"))
        .build();
    println!("Optional params: {qs}");
    assert_eq!(qs, "page=1&after=abc123");

    // Flatten a struct's fields via .extend_from_struct()
    let params = SearchParams {
        q: "hello world".into(),
        page: 2,
        per_page: 50,
        filter: None,
    };
    let qs = QueryBuilder::new()
        .extend_from_struct(&params)
        .expect("serializable struct")
        .build();
    println!("From struct:     {qs}");
    assert!(qs.contains("q=hello+world"));
    assert!(qs.contains("page=2"));
    assert!(qs.contains("per_page=50"));
    assert!(!qs.contains("filter"));

    // Merge into an existing URL
    let base = "https://api.example.com/v1/items";
    let url = QueryBuilder::new()
        .set("page", 3u32)
        .set("sort", "name")
        .merge_into_url(base);
    println!("Merged (no ?):   {url}");
    assert_eq!(url, "https://api.example.com/v1/items?page=3&sort=name");

    let base_with_qs = "https://api.example.com/v1/items?limit=10";
    let url = QueryBuilder::new()
        .set("page", 2u32)
        .merge_into_url(base_with_qs);
    println!("Merged (has ?):  {url}");
    assert_eq!(url, "https://api.example.com/v1/items?limit=10&page=2");

    // URL encoding: spaces, ampersands
    let qs = QueryBuilder::new().set("q", "hello world&more").build();
    println!("URL encoded:     {qs}");
    assert_eq!(qs, "q=hello+world%26more");

    println!("\nAll assertions passed.");
}
