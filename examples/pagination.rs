//! Pagination examples.
//!
//! Demonstrates constructing both offset-based (`PaginatedResponse`) and
//! cursor-based (`CursorPaginatedResponse`) paginated responses.
//!
//! Run: `cargo run --example pagination`

use api_bones::{CursorPaginatedResponse, CursorPagination, PaginatedResponse, PaginationParams};

fn main() {
    // -- Offset-based pagination --
    let params = PaginationParams {
        limit: Some(10),
        offset: Some(0),
    };
    let items: Vec<&str> = vec!["alpha", "bravo", "charlie"];
    let response = PaginatedResponse::new(items, 25, &params);

    println!("=== Offset-based pagination ===");
    println!("Items:       {:?}", response.items);
    println!("Total count: {}", response.total_count);
    println!("Has more:    {}", response.has_more);
    println!("Limit:       {}", response.limit);
    println!("Offset:      {}", response.offset);

    #[cfg(feature = "serde")]
    println!(
        "JSON:\n{}",
        serde_json::to_string_pretty(&response).expect("serialize")
    );

    // -- Cursor-based pagination --
    let cursor_response = CursorPaginatedResponse::new(
        vec!["delta", "echo", "foxtrot"],
        CursorPagination::more("eyJpZCI6NDJ9"),
    );

    println!("\n=== Cursor-based pagination ===");
    println!("Data:        {:?}", cursor_response.data);
    println!("Has more:    {}", cursor_response.pagination.has_more);
    println!("Next cursor: {:?}", cursor_response.pagination.next_cursor);

    #[cfg(feature = "serde")]
    println!(
        "JSON:\n{}",
        serde_json::to_string_pretty(&cursor_response).expect("serialize")
    );

    // -- Last page (no cursor) --
    let last_page = CursorPaginatedResponse::new(vec!["golf"], CursorPagination::last_page());

    println!("\n=== Last page ===");
    println!("Has more:    {}", last_page.pagination.has_more);
    println!("Next cursor: {:?}", last_page.pagination.next_cursor);
}
