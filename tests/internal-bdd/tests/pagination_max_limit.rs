// SPDX-License-Identifier: MIT
//! Runs `features/pagination_max_limit.feature`.
//!
//! `api_bones::pagination::MAX_LIMIT` is the single named ceiling every
//! list endpoint validates a page size against -- no declared SDK surface
//! exposes it directly (SPEC.md `sdk_surfaces`), so its contract is proven
//! here via SPEC.md `internal_behavior_owners` instead.

use api_bones::connect::build_page;
use api_bones::pagination::PaginationParams;
use cucumber::{World, then, when};

#[derive(Debug, Default, World)]
struct PageWorld {
    pagination_result: Option<Result<PaginationParams, api_bones::ValidationError>>,
    connect_limit: Option<u64>,
}

#[when(expr = "a caller requests offset pagination with limit {int}")]
fn when_offset_pagination(world: &mut PageWorld, limit: u64) {
    world.pagination_result = Some(PaginationParams::new(limit, 0));
}

#[then(expr = "the request is accepted with limit {int}")]
fn then_accepted(world: &mut PageWorld, expected: u64) {
    let params = world
        .pagination_result
        .take()
        .expect("no pagination request made yet")
        .expect("expected the request to be accepted");
    assert_eq!(params.limit(), expected);
}

#[then(expr = "the request is rejected as out of range")]
fn then_rejected(world: &mut PageWorld) {
    let err = world
        .pagination_result
        .take()
        .expect("no pagination request made yet")
        .expect_err("expected the request to be rejected");
    assert_eq!(err.rule.as_deref(), Some("range"));
}

#[when(expr = "a caller requests a Connect offset page with limit {int}")]
fn when_connect_page(world: &mut PageWorld, limit: u64) {
    let page = build_page(limit, 0, 0, 0);
    world.connect_limit = Some(page.limit());
}

#[then(expr = "the built page reports limit {int}")]
fn then_page_limit(world: &mut PageWorld, expected: u64) {
    let limit = world
        .connect_limit
        .take()
        .expect("no Connect page built yet");
    assert_eq!(limit, expected);
}

#[tokio::main]
async fn main() {
    let features = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/features/pagination_max_limit.feature"
    );
    PageWorld::run(features).await;
}
