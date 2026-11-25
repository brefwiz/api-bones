// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! Runs the Gherkin contract both languages answer: a refused call waits out
//! its rate limit, bounded by its deadline and its abort.
mod steps;
mod world;

use brefwiz_cucumber_steps::TwoPass;
use world::RateLimitWorld;

#[tokio::main]
async fn main() {
    let features = format!(
        "{}/../features/connect_rate_limit_retry.feature",
        env!("CARGO_MANIFEST_DIR")
    );
    let two_pass = TwoPass {
        skip_tags: vec!["wip".into()],
        isolated_tag: "isolated".into(),
        parallel: 1,
    };
    two_pass.run::<RateLimitWorld>(features).await;
}
