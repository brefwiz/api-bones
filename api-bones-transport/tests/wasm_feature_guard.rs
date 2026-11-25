// SPDX-License-Identifier: MIT
//! The browser build of the Connect client stack must not carry connectrpc's
//! `client` or `client-tls` features, which start tokio tasks and read
//! `std::time::Instant`, and no call may set `CallOptions.timeout`, which
//! connectrpc arms with a clock of its own. The deadline reaches the wire
//! through the one `Deadline` extension instead.
//!
//! Three checks, all native:
//!
//! 1. every manifest of the workspace enables those features only inside a
//!    `cfg(not(target_arch = "wasm32"))` table;
//! 2. the dependency graph Cargo resolves for `wasm32-unknown-unknown`, for
//!    each crate a browser build reaches, enables neither feature on
//!    connectrpc;
//! 3. no workspace source sets a call's timeout through `CallOptions`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The crates a browser build of a generated client reaches.
const BROWSER_CRATES: [&str; 3] = ["api-bones-transport", "api-bones-batch", "api-bones-page"];

/// The connectrpc features a browser must not enable.
const FORBIDDEN_FEATURES: [&str; 2] = ["client", "client-tls"];

/// The workspace root, one level above this crate.
fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits inside the workspace")
        .to_path_buf()
}

/// The root manifest and the manifest of every workspace member directory.
fn manifests() -> Vec<PathBuf> {
    let root = workspace();
    let mut found = vec![root.join("Cargo.toml")];
    for entry in fs::read_dir(&root).expect("read the workspace root") {
        let path = entry.expect("directory entry").path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if name.starts_with("api-bones-") && path.join("Cargo.toml").is_file() {
            found.push(path.join("Cargo.toml"));
        }
    }
    found.sort();
    found
}

/// The Rust sources of the root crate and of every member crate.
fn sources() -> Vec<PathBuf> {
    fn walk(directory: &Path, into: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(directory).expect("read a source directory") {
            let path = entry.expect("directory entry").path();
            if path.is_dir() {
                walk(&path, into);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                into.push(path);
            }
        }
    }
    let mut found = Vec::new();
    let root = workspace();
    walk(&root.join("src"), &mut found);
    for manifest in manifests() {
        let source = manifest.parent().expect("manifest directory").join("src");
        if source.is_dir() && source != root.join("src") {
            walk(&source, &mut found);
        }
    }
    found.sort();
    found
}

#[test]
fn connectrpc_client_features_sit_only_in_the_native_target_table() {
    let native_only = "cfg(not(target_arch = \"wasm32\"))";
    let mut offences = Vec::new();
    let mut checked = 0;
    for manifest in manifests() {
        let text = fs::read_to_string(&manifest).expect("read a manifest");
        let mut section = String::new();
        for (index, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.starts_with('[') {
                section = line.to_owned();
            }
            let declares_connectrpc =
                line.starts_with("connectrpc ") || line.starts_with("connectrpc=");
            if !declares_connectrpc {
                continue;
            }
            checked += 1;
            let enables = FORBIDDEN_FEATURES
                .iter()
                .any(|feature| line.contains(&format!("\"{feature}\"")));
            if enables && !section.contains(native_only) {
                offences.push(format!(
                    "{}:{}: `{line}` in `{section}`",
                    manifest.display(),
                    index + 1
                ));
            }
        }
    }
    assert!(checked >= 4, "the scan reached the connectrpc declarations");
    assert!(
        offences.is_empty(),
        "connectrpc's client features outside a non-wasm32 table:\n{}",
        offences.join("\n")
    );
}

#[test]
fn the_fetch_path_builds_no_native_http_client() {
    let needles = [
        ["Http", "Client"].concat(),
        ["Http2", "Connection"].concat(),
    ];
    let path = workspace()
        .join("api-bones-transport")
        .join("src")
        .join("fetch.rs");
    let text = fs::read_to_string(&path).expect("read the fetch source");
    assert!(!text.trim().is_empty(), "the scan read {}", path.display());
    for needle in &needles {
        assert!(
            !text.contains(needle.as_str()),
            "{} names `{needle}`; the fetch path must not build a native client",
            path.display()
        );
    }
}

#[test]
fn no_source_sets_a_call_timeout_through_call_options() {
    let needles = [
        ["with_", "timeout("].concat(),
        ["options.", "timeout ="].concat(),
        ["opts.", "timeout ="].concat(),
        ["CallOptions { ", "timeout"].concat(),
    ];
    let sources = sources();
    assert!(
        sources.len() > 20,
        "the scan reached the workspace sources, got {}",
        sources.len()
    );
    let mut offences = Vec::new();
    for source in sources {
        let text = fs::read_to_string(&source).expect("read a source file");
        for (index, line) in text.lines().enumerate() {
            if needles.iter().any(|needle| line.contains(needle.as_str())) {
                offences.push(format!(
                    "{}:{}: {}",
                    source.display(),
                    index + 1,
                    line.trim()
                ));
            }
        }
    }
    assert!(
        offences.is_empty(),
        "a call's timeout reaches the wire through the Deadline extension, never through \
         CallOptions:\n{}",
        offences.join("\n")
    );
}

/// What Cargo resolved for `wasm32-unknown-unknown`: the inverted feature tree
/// of connectrpc below `krate`, resolved without the network.
fn wasm_feature_tree(krate: &str) -> String {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .current_dir(workspace())
        .args(["tree", "--offline", "--target", "wasm32-unknown-unknown"])
        .args(["-p", krate])
        .args(["--edges", "features", "--invert", "connectrpc@0.9.1"])
        .output()
        .expect("run cargo tree");
    assert!(
        output.status.success(),
        "cargo tree for {krate} on wasm32 failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("cargo tree prints UTF-8")
}

#[test]
fn the_wasm32_graph_enables_no_connectrpc_client_feature() {
    for krate in BROWSER_CRATES {
        let tree = wasm_feature_tree(krate);
        assert!(
            tree.contains("connectrpc v0.9.1"),
            "{krate} reaches connectrpc on wasm32, tree was:\n{tree}"
        );
        for feature in FORBIDDEN_FEATURES {
            assert!(
                !tree.contains(&format!("feature \"{feature}\"")),
                "{krate} enables connectrpc's `{feature}` on wasm32:\n{tree}"
            );
        }
    }
}
