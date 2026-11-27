// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! Runs the Gherkin contract the Rust SDK answers; the TypeScript package has no fleet transport.
//!
//! `TwoPass` owns scenario selection, concurrency and the census.
mod steps;
mod world;

use brefwiz_cucumber_steps::TwoPass;
use world::FailureReachWorld;

#[tokio::main]
async fn main() {
    let features = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../features/connect_failure_reach.feature"
    )
    .to_owned();
    // Every scenario opens its own peer, so none shares state.
    let two_pass = TwoPass {
        skip_tags: vec!["wip".into()],
        isolated_tag: "isolated".into(),
        parallel: 1,
    };
    two_pass.run::<FailureReachWorld>(features).await;
}
