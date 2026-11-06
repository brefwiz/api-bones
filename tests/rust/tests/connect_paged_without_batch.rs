// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! Runs the Gherkin contract both languages answer: the paged iterator on a
//! service that declares List RPCs and no batch method.
mod steps;
mod world;

use brefwiz_cucumber_steps::TwoPass;
use world::UnbatchedPagingWorld;

#[tokio::main]
async fn main() {
    let features = format!(
        "{}/../features/connect_paged_without_batch.feature",
        env!("CARGO_MANIFEST_DIR")
    );
    let two_pass = TwoPass {
        skip_tags: vec!["wip".into()],
        isolated_tag: "isolated".into(),
        parallel: 1,
    };
    two_pass.run::<UnbatchedPagingWorld>(features).await;
}
