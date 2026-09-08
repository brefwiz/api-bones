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
use world::RetryWorld;

#[tokio::main]
async fn main() {
    let features = concat!(env!("CARGO_MANIFEST_DIR"), "/../features").to_owned();
    // Nothing is skipped and nothing is isolated: every row of this contract is
    // a pure classification over one constructed error, so the scenarios share
    // no state and none of them needs holding back.
    let two_pass = TwoPass {
        skip_tags: vec!["wip".into()],
        isolated_tag: "isolated".into(),
        parallel: 1,
    };
    two_pass.run::<RetryWorld>(features).await;
}
