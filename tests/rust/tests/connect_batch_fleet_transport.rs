// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! Runs the Gherkin contract the Rust SDK answers: a batch read through a
//! generated client on the fleet transport. The TypeScript package has no
//! fleet client.
//!
//! `TwoPass` owns scenario selection, concurrency and the census.
mod steps;
mod world;

use brefwiz_cucumber_steps::TwoPass;
use world::BatchWorld;

#[tokio::main]
async fn main() {
    let features = format!(
        "{}/../features/connect_batch_fleet_transport.feature",
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
