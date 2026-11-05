// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! Runs typed-field scenarios through shipped validator APIs.
mod fields_world;
#[path = "steps/typed_fields.rs"]
mod typed_fields;
use brefwiz_cucumber_steps::TwoPass;
use fields_world::FieldsWorld;

#[tokio::main]
async fn main() {
    let features = format!(
        "{}/../features/typed_fields.feature",
        env!("CARGO_MANIFEST_DIR")
    );
    TwoPass {
        skip_tags: vec!["wip".into()],
        isolated_tag: "isolated".into(),
        parallel: 1,
    }
    .run::<FieldsWorld>(features)
    .await;
}
