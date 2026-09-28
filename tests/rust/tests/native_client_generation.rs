// SPDX-License-Identifier: MIT
mod native_generated {
    include!(concat!(env!("OUT_DIR"), "/bones_native.rs"));
}
#[path = "steps/native.rs"]
mod native_steps;

use brefwiz_cucumber_steps::TwoPass;
use native_steps::NativeWorld;

#[tokio::main]
async fn main() {
    let features = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../features/native_client_generation.feature"
    )
    .to_owned();
    TwoPass {
        skip_tags: vec!["wip".into()],
        isolated_tag: "isolated".into(),
        parallel: 1,
    }
    .run::<NativeWorld>(features)
    .await;
}
