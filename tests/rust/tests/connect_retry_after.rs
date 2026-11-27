// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! Runs the Gherkin contract both languages answer: a refusal names when to
//! come back, and a client reads the wait.
mod steps;
mod world;

use brefwiz_cucumber_steps::TwoPass;
use world::RetryAfterWorld;

#[tokio::main]
async fn main() {
    let features = format!(
        "{}/../features/connect_retry_after.feature",
        env!("CARGO_MANIFEST_DIR")
    );
    let two_pass = TwoPass {
        skip_tags: vec!["wip".into()],
        isolated_tag: "isolated".into(),
        parallel: 1,
    };
    two_pass.run::<RetryAfterWorld>(features).await;
}
