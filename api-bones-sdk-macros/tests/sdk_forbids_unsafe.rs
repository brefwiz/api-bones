// SPDX-License-Identifier: MIT
//! The core keeps its unconditional `forbid(unsafe_code)`: no feature, and so
//! no consumer that enables one, can relax it. The uniffi scaffolding, which
//! expands `unsafe`, lives in `api-bones-sdk-ffi` and never in the core.
//!
//! `forbid` cannot be overridden from inside the crate, so what there is to
//! check is that the line is there, unconditional, and that nothing in the
//! core names a way around it.

use std::path::{Path, PathBuf};

fn sdk_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../api-bones-sdk")
}

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("a readable directory") {
        let path = entry.expect("an entry").path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn the_core_forbids_unsafe_unconditionally() {
    let lib = std::fs::read_to_string(sdk_root().join("src/lib.rs")).expect("lib.rs");
    assert!(
        lib.lines()
            .any(|line| line.trim() == "#![forbid(unsafe_code)]"),
        "api-bones-sdk keeps `#![forbid(unsafe_code)]` as its own line"
    );
}

#[test]
fn nothing_in_the_core_relaxes_the_lint_or_links_uniffi() {
    let mut files = Vec::new();
    sources(&sdk_root().join("src"), &mut files);
    assert_ne!(files.len(), 0, "the core has sources to scan");
    for file in files {
        let text = std::fs::read_to_string(&file).expect("a source");
        for line in text.lines().filter(|l| !l.trim_start().starts_with("//")) {
            let relaxes = line.contains("allow(unsafe_code)")
                || line.contains("expect(unsafe_code)")
                || (line.contains("cfg_attr") && line.contains("unsafe_code"))
                || line.contains("uniffi");
            assert!(!relaxes, "{}: {line}", file.display());
        }
    }
    let manifest = std::fs::read_to_string(sdk_root().join("Cargo.toml")).expect("manifest");
    assert!(
        !manifest.contains("uniffi"),
        "the core depends on no uniffi crate"
    );
}
