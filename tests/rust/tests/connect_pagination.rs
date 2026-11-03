// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! Runs the Gherkin contract both languages answer: the page iterator over a
//! PostgreSQL-backed Widgets service.
//!
//! `TwoPass` owns scenario selection, concurrency and the census.
mod steps;
mod world;

use brefwiz_cucumber_steps::TwoPass;
use world::PagingWorld;

#[tokio::main]
async fn main() {
    let features = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../features/connect_pagination.feature"
    )
    .to_owned();
    // Every scenario owns its database and transport.
    let two_pass = TwoPass {
        skip_tags: vec!["wip".into()],
        isolated_tag: "isolated".into(),
        parallel: 1,
    };
    two_pass.run::<PagingWorld>(features).await;
}
