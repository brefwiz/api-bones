// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! Runs the Gherkin contract the Rust SDK answers; the TypeScript package
//! leaves root trust to its runtime.
//!
//! Each scenario sets `SSL_CERT_FILE` and reads the count from the real
//! `root_store()`, so scenarios run one at a time.
//!
//! `TwoPass` owns scenario selection, concurrency and the census.
mod steps;
mod world;

use brefwiz_cucumber_steps::TwoPass;
use world::RootStoreWorld;

#[tokio::main]
async fn main() {
    let features = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../features/connect_root_store.feature"
    )
    .to_owned();
    // Every scenario writes its own file and owns the environment while it runs.
    let two_pass = TwoPass {
        skip_tags: vec!["wip".into()],
        isolated_tag: "isolated".into(),
        parallel: 1,
    };
    two_pass.run::<RootStoreWorld>(features).await;
}
