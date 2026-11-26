// SPDX-License-Identifier: MIT
//! `wrapper typescript` against the test service's Layer 2 crate: the client
//! it writes is the golden one, and the manifest, `l2-methods.json` and
//! `api-surface.d.ts` it writes are the ones tracked beside the package.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const GOLDEN_CLIENT: &str = include_str!("fixtures/wrapper-ts/client.ts");
const GOLDEN_TYPES: &str = include_str!("fixtures/wrapper-ts/types.ts");
const GOLDEN_ERRORS: &str = include_str!("fixtures/wrapper-ts/errors.ts");
const GOLDEN_INDEX: &str = include_str!("fixtures/wrapper-ts/index.ts");
const GOLDEN_INIT: &str = include_str!("fixtures/wrapper-ts/init.ts");
const TRACKED_METHODS: &str = include_str!("../../tests/l2/typescript/l2-methods.json");
const TRACKED_SURFACE: &str = include_str!("../../tests/l2/typescript/api-surface.d.ts");
const TRACKED_MANIFEST: &str = include_str!("../../tests/l2/typescript/package.json");

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn generate(name: &str) -> PathBuf {
    let out = std::env::temp_dir().join(format!(
        "api-bones-sdk-gen-wrapper-{}-{name}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&out);
    let output = Command::new(env!("CARGO_BIN_EXE_api-bones-sdk-gen"))
        .args(["wrapper", "typescript", "--crate"])
        .arg(repo().join("tests/l2"))
        .arg("--spec")
        .arg(repo().join("SPEC.md"))
        .args(["--surface", "sdk-call", "--out"])
        .arg(&out)
        .output()
        .expect("the generator runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    out
}

fn read(out: &Path, path: &str) -> String {
    fs::read_to_string(out.join(path)).expect("a generated file")
}

#[test]
fn the_client_written_is_the_golden_one() {
    let out = generate("client");
    assert_eq!(read(&out, "src/generated-l2/client.ts"), GOLDEN_CLIENT);
    fs::remove_dir_all(&out).expect("cleaned");
}

#[test]
fn the_snapshot_and_manifest_written_are_the_tracked_ones() {
    let out = generate("tracked");
    assert_eq!(read(&out, "l2-methods.json"), TRACKED_METHODS);
    assert_eq!(read(&out, "api-surface.d.ts"), TRACKED_SURFACE);
    let written: serde_json::Value =
        serde_json::from_str(&read(&out, "package.json")).expect("json");
    let tracked: serde_json::Value = serde_json::from_str(TRACKED_MANIFEST).expect("json");
    assert_eq!(written, tracked);
    fs::remove_dir_all(&out).expect("cleaned");
}

#[test]
fn the_support_files_written_are_the_golden_ones() {
    let out = generate("files");
    for (file, golden) in [
        ("types.ts", GOLDEN_TYPES),
        ("errors.ts", GOLDEN_ERRORS),
        ("index.ts", GOLDEN_INDEX),
        ("init.ts", GOLDEN_INIT),
    ] {
        assert_eq!(
            read(&out, &format!("src/generated-l2/{file}")),
            golden,
            "{file}"
        );
    }
    fs::remove_dir_all(&out).expect("cleaned");
}

#[test]
fn a_surface_the_spec_does_not_list_fails_the_command() {
    let output = Command::new(env!("CARGO_BIN_EXE_api-bones-sdk-gen"))
        .args(["wrapper", "typescript", "--crate"])
        .arg(repo().join("tests/l2"))
        .arg("--spec")
        .arg(repo().join("SPEC.md"))
        .args(["--surface", "no-such-surface", "--out"])
        .arg(std::env::temp_dir().join("api-bones-sdk-gen-wrapper-none"))
        .output()
        .expect("the generator runs");
    assert!(!output.status.success());
}

#[test]
fn size_reports_the_schema_and_the_byte_count() {
    let wasm = std::env::temp_dir().join(format!(
        "api-bones-sdk-gen-size-{}.wasm",
        std::process::id()
    ));
    fs::write(&wasm, [0u8; 12]).expect("a module");
    let output = Command::new(env!("CARGO_BIN_EXE_api-bones-sdk-gen"))
        .args(["wrapper", "size", "--crate-name", "numbers", "--wasm"])
        .arg(&wasm)
        .output()
        .expect("the generator runs");
    fs::remove_file(&wasm).expect("cleaned");
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("\"api-bones.wasm-size/v1\""), "{text}");
    assert!(text.contains("\"bytes\": 12"), "{text}");
}
