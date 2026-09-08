// SPDX-License-Identifier: MIT
//! Runs the shared Gherkin contract that both languages answer.
//!
//! The runner is `brefwiz_cucumber_steps::TwoPass` rather than a hand-built
//! `filter_run_and_exit`: constructing a Cucumber runner directly is what the
//! BDD delegation rule exists to stop, and the platform primitive already owns
//! scenario selection, concurrency and the census.
//!
//! This lane crate is `publish = false`, so depending on the internal runner
//! costs the published artifacts nothing — the same shape `api-bones-connect`
//! already has with its own internal dependency.

mod steps;

use brefwiz_cucumber_steps::TwoPass;
use steps::RetryWorld;

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
