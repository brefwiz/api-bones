// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! Runs the shared error-localization Gherkin contract that both languages
//! answer.
//!
//! `TwoPass` owns scenario selection, concurrency and the census. The catalog
//! registry is process-wide, so scenarios run one at a time.
//!
//! This lane crate is `publish = false`, so its dependencies reach no released
//! artifact.
mod steps;
mod world;

use brefwiz_cucumber_steps::TwoPass;
use world::LocalizationWorld;

#[tokio::main]
async fn main() {
    let features = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../features/error_localization.feature"
    )
    .to_owned();
    let two_pass = TwoPass {
        skip_tags: vec!["wip".into()],
        isolated_tag: "isolated".into(),
        parallel: 1,
    };
    two_pass.run::<LocalizationWorld>(features).await;
}
