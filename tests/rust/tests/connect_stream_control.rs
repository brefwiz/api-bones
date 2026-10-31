// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! Runs the Gherkin contract the Rust SDK answers; the TypeScript codec has no surface yet.
//!
//! `TwoPass` owns scenario selection, concurrency and the census.
mod steps;
mod world;

use brefwiz_cucumber_steps::TwoPass;
use world::StreamControlWorld;

#[tokio::main]
async fn main() {
    let features = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../features/connect_stream_control.feature"
    )
    .to_owned();
    // Every scenario declares its own control and frame, so none shares state.
    let two_pass = TwoPass {
        skip_tags: vec!["wip".into()],
        isolated_tag: "isolated".into(),
        parallel: 1,
    };
    two_pass.run::<StreamControlWorld>(features).await;
}
