// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! Runs the shared Gherkin contract that both languages answer.
//!
//! `TwoPass` owns scenario selection, concurrency and the census.
mod steps;
mod world;

use brefwiz_cucumber_steps::TwoPass;
use world::OverridableDefaultsWorld;

#[tokio::main]
async fn main() {
    let features = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../features/overridable_defaults.feature"
    )
    .to_owned();
    // Every scenario compiles its own contract, so none shares state.
    let two_pass = TwoPass {
        skip_tags: vec!["wip".into()],
        isolated_tag: "isolated".into(),
        parallel: 1,
    };
    two_pass.run::<OverridableDefaultsWorld>(features).await;
}
