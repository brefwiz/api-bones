// SPDX-License-Identifier: MIT
//! Runs the shared Gherkin contract that both languages answer.
//!
//! `has_tag` is local rather than taken from the shared runner crate: this
//! repository publishes to crates.io under MIT and carries no dependency on
//! the internal platform crates, so a helper this small is written here rather
//! than imported.

mod steps;

use cucumber::World as _;
use cucumber::gherkin::{Feature, Scenario};
use steps::RetryWorld;

/// Whether `tag` is on the scenario or inherited from its feature.
fn has_tag(feature: &Feature, scenario: &Scenario, tag: &str) -> bool {
    let wanted = format!("@{tag}");
    feature.tags.iter().any(|each| *each == wanted || *each == tag)
        || scenario.tags.iter().any(|each| *each == wanted || *each == tag)
}

#[tokio::main]
async fn main() {
    RetryWorld::cucumber()
        .max_concurrent_scenarios(Some(1))
        .filter_run_and_exit("../features", |f, _r, s| has_tag(f, s, "connect"))
        .await;
}
