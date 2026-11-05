// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! Runs the batch identity-guard Gherkin contract.
mod steps;
mod world;

use brefwiz_cucumber_steps::TwoPass;
use world::BatchWorld;

#[tokio::main]
async fn main() {
    let features = format!(
        "{}/../features/connect_batch_identity_guard.feature",
        env!("CARGO_MANIFEST_DIR")
    );
    let two_pass = TwoPass {
        skip_tags: vec!["wip".into()],
        isolated_tag: "isolated".into(),
        parallel: 1,
    };
    two_pass.run::<BatchWorld>(features).await;
}
