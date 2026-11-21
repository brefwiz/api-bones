// SPDX-License-Identifier: MIT
//! The formatting data is the CLDR release `cldr.json` records for Rust.

use std::collections::{BTreeSet, HashMap};
use std::process::Command;

use serde_json::Value;

fn recorded() -> Value {
    serde_json::from_str(include_str!("../cldr.json")).expect("cldr.json is valid JSON")
}

fn recorded_rust_release() -> String {
    recorded()["rust"]
        .as_str()
        .expect("cldr.json records a rust entry")
        .to_owned()
}

fn metadata() -> Value {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--locked", "--offline"])
        .arg("--manifest-path")
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .output()
        .expect("cargo metadata runs");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("cargo metadata prints JSON")
}

/// The `[package.metadata.sources.cldr] tagged` of every `icu_*_data` package
/// this crate's compiled data comes from.
fn compiled_data_releases(metadata: &Value) -> BTreeSet<(String, String)> {
    let packages: HashMap<&str, &Value> = metadata["packages"]
        .as_array()
        .expect("packages")
        .iter()
        .map(|package| (package["id"].as_str().expect("id"), package))
        .collect();
    let edges: HashMap<&str, Vec<&str>> = metadata["resolve"]["nodes"]
        .as_array()
        .expect("resolve nodes")
        .iter()
        .map(|node| {
            let deps = node["dependencies"]
                .as_array()
                .expect("dependencies")
                .iter()
                .filter_map(Value::as_str)
                .collect();
            (node["id"].as_str().expect("id"), deps)
        })
        .collect();
    let root = packages
        .iter()
        .find(|(_, package)| package["name"] == "api-bones-i18n")
        .map(|(id, _)| *id)
        .expect("api-bones-i18n is in the graph");

    let mut seen = BTreeSet::new();
    let mut stack = vec![root];
    let mut releases = BTreeSet::new();
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        let package = packages[id];
        let name = package["name"].as_str().expect("name");
        if name.starts_with("icu_") && name.ends_with("_data") {
            let tagged = package["metadata"]["sources"]["cldr"]["tagged"]
                .as_str()
                .unwrap_or_else(|| panic!("{name} declares no CLDR release"));
            releases.insert((name.to_owned(), tagged.to_owned()));
        }
        stack.extend(edges[id].iter().copied());
    }
    releases
}

#[test]
fn the_compiled_data_is_the_recorded_cldr_release() {
    let recorded = recorded_rust_release();
    let releases = compiled_data_releases(&metadata());
    assert!(
        releases.len() >= 4,
        "the compiled locale data crates were not found: {releases:?}"
    );
    for (name, tagged) in &releases {
        assert_eq!(*tagged, recorded, "{name}");
    }
}

#[test]
fn the_record_names_a_release_per_runtime() {
    let record = recorded();
    let entries = record.as_object().expect("cldr.json is an object");
    assert_eq!(entries.len(), 2);
    assert!(entries["rust"].is_string());
    assert!(entries["node"].is_string());
}
