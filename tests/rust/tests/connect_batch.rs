// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! Runs the Gherkin contract both languages answer: the batch runtime over a
//! multi-organization widgets server.
//!
//! `TwoPass` owns scenario selection,
//! concurrency and the census.
mod steps;
mod world;

use brefwiz_cucumber_steps::TwoPass;
use world::BatchWorld;

#[tokio::main]
async fn main() {
    let features = format!(
        "{}/../features/connect_batch.feature",
        env!("CARGO_MANIFEST_DIR")
    );
    // Every scenario builds its own server and client, so none shares state.
    let two_pass = TwoPass {
        skip_tags: vec!["wip".into()],
        isolated_tag: "isolated".into(),
        parallel: 1,
    };
    two_pass.run::<BatchWorld>(features).await;
}
