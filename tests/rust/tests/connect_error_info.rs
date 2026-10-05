// SPDX-License-Identifier: MIT
//! Runs the shared Gherkin contract that both languages answer.
//!
//! `TwoPass` owns scenario selection, concurrency and the census.
//!
//! This lane crate is `publish = false`, so its dependencies reach no released
//! artifact.
mod steps;
mod world;

use brefwiz_cucumber_steps::TwoPass;
use world::ErrorInfoWorld;

#[tokio::main]
async fn main() {
    let features = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../features/connect_error_info.feature"
    )
    .to_owned();
    // Every scenario builds its own failure, so none of them shares state and
    // none needs holding back.
    let two_pass = TwoPass {
        skip_tags: vec!["wip".into()],
        isolated_tag: "isolated".into(),
        parallel: 1,
    };
    two_pass.run::<ErrorInfoWorld>(features).await;
}
