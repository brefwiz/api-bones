//! Query parameter types for list endpoints.
//!
//! Demonstrates `SortParams`, `SortDirection`, `FilterParams`,
//! `FilterEntry`, and `SearchParams`.
//!
//! Run: `cargo run --example query_params`

use api_bones::{FilterEntry, FilterParams, SearchParams, SortDirection, SortParams};

fn main() {
    // -- Sorting --
    println!("=== Sorting ===");
    let sort_asc = SortParams::asc("created_at");
    let sort_desc = SortParams::desc("price");
    println!(
        "SortParams::asc(\"created_at\"): sort_by={}, direction={:?}",
        sort_asc.sort_by, sort_asc.direction
    );
    println!(
        "SortParams::desc(\"price\"):     sort_by={}, direction={:?}",
        sort_desc.sort_by, sort_desc.direction
    );
    println!("Default SortDirection: {:?}", SortDirection::default());

    // -- Filtering --
    println!("\n=== Filtering ===");
    let filters = FilterParams::new(vec![
        FilterEntry {
            field: "status".into(),
            operator: "eq".into(),
            value: "active".into(),
        },
        FilterEntry {
            field: "price".into(),
            operator: "gt".into(),
            value: "100".into(),
        },
    ]);
    println!("FilterParams ({} filters):", filters.filters.len());
    println!("  is_empty: {}", filters.is_empty());
    print_json("  ", &filters);

    let empty_filters = FilterParams::default();
    println!(
        "\nEmpty FilterParams: is_empty={}",
        empty_filters.is_empty()
    );

    // -- Full-text search --
    println!("\n=== Search ===");
    let basic_search = SearchParams::new("deluxe suite");
    println!("Basic search:");
    print_json("  ", &basic_search);

    let scoped_search = SearchParams::with_fields("deluxe suite", vec!["name", "description"]);
    println!("\nScoped search (specific fields):");
    print_json("  ", &scoped_search);
}

fn print_json(prefix: &str, value: &impl serde::Serialize) {
    let json = serde_json::to_string_pretty(value).expect("serialization");
    for line in json.lines() {
        println!("{prefix}{line}");
    }
}
