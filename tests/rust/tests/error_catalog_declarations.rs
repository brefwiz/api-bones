// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! Runs the error catalog declarations Gherkin contract over the shipped vocabulary.
//!
//! `TwoPass` owns scenario selection, concurrency and the census.
mod steps;
mod world;

use brefwiz_cucumber_steps::TwoPass;
use world::ErrorCatalogWorld;

#[tokio::main]
async fn main() {
    let features = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../features/error_catalog_declarations.feature"
    )
    .to_owned();
    // Every scenario compiles its own contract, so none shares state.
    let two_pass = TwoPass {
        skip_tags: vec!["wip".into()],
        isolated_tag: "isolated".into(),
        parallel: 1,
    };
    two_pass.run::<ErrorCatalogWorld>(features).await;
}
