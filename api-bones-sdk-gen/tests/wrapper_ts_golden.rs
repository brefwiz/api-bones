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
const TRACKED_METHODS: &str = include_str!("../../api-bones-test-service-sdk-ts/l2-methods.json");
const TRACKED_SURFACE: &str = include_str!("../../api-bones-test-service-sdk-ts/api-surface.d.ts");
const TRACKED_MANIFEST: &str = include_str!("../../api-bones-test-service-sdk-ts/package.json");

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn generate(name: &str) -> PathBuf {
    generate_from(name, &repo().join("SPEC.md"))
}

fn run_from(name: &str, spec: &Path) -> (std::process::Output, PathBuf) {
    let out = std::env::temp_dir().join(format!(
        "api-bones-sdk-gen-wrapper-{}-{name}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&out);
    let output = Command::new(env!("CARGO_BIN_EXE_api-bones-sdk-gen"))
        .args(["wrapper", "typescript", "--crate"])
        .arg(repo().join("tests/l2"))
        .arg("--spec")
        .arg(spec)
        .args(["--surface", "sdk-call", "--out"])
        .arg(&out)
        .output()
        .expect("the generator runs");
    (output, out)
}

fn generate_from(name: &str, spec: &Path) -> PathBuf {
    let (output, out) = run_from(name, spec);
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
    assert_eq!(read(&out, "src/generated/client.ts"), GOLDEN_CLIENT);
    fs::remove_dir_all(&out).expect("cleaned");
}

#[test]
fn the_snapshot_and_manifest_written_are_the_tracked_ones() {
    let out = generate("tracked");
    assert_eq!(read(&out, "l2-methods.json"), TRACKED_METHODS);
    assert_eq!(read(&out, "api-surface.d.ts"), TRACKED_SURFACE);
    let (written, written_runtime) = without_runtime(&read(&out, "package.json"));
    let (tracked, tracked_runtime) = without_runtime(TRACKED_MANIFEST);
    assert_eq!(written, tracked);
    assert_eq!(
        written["peerDependencies"]["@brefwiz/api-bones-sdk-runtime"],
        ">=0.2.0"
    );
    assert!(written.get("private").is_none());
    let link = tracked_runtime
        .strip_prefix("file:")
        .expect("a tracked file: link");
    assert_eq!(
        repo()
            .join("api-bones-test-service-sdk-ts")
            .join(link)
            .canonicalize()
            .expect("the tracked link resolves"),
        repo()
            .join("api-bones-sdk-runtime-ts")
            .canonicalize()
            .expect("the runtime package")
    );
    assert!(written_runtime.starts_with("file:"), "{written_runtime}");
    fs::remove_dir_all(&out).expect("cleaned");
}

/// The manifest without its runtime dev link (which spells the path from the
/// package directory), and that link. The runtime peer range stays in the
/// manifest, so the comparison covers it.
fn without_runtime(text: &str) -> (serde_json::Value, String) {
    let mut manifest: serde_json::Value = serde_json::from_str(text).expect("json");
    let runtime = manifest["devDependencies"]
        .as_object_mut()
        .and_then(|dependencies| dependencies.remove("@brefwiz/api-bones-sdk-runtime"))
        .and_then(|version| version.as_str().map(str::to_owned))
        .expect("a runtime dev link");
    (manifest, runtime)
}

#[test]
fn the_runtime_next_to_the_spec_is_linked_relative_to_the_output() {
    let out = generate("link");
    let (_, runtime) = without_runtime(&read(&out, "package.json"));
    let link = runtime.strip_prefix("file:").expect("a file: link");
    assert_eq!(
        out.join(link).canonicalize().expect("the link resolves"),
        repo()
            .join("api-bones-sdk-runtime-ts")
            .canonicalize()
            .expect("the runtime package")
    );
    fs::remove_dir_all(&out).expect("cleaned");
}

#[test]
fn a_published_wrapper_without_the_runtime_beside_its_spec_is_refused() {
    let home = std::env::temp_dir().join(format!(
        "api-bones-sdk-gen-wrapper-{}-lone-spec",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(&home).expect("a directory");
    fs::copy(repo().join("SPEC.md"), home.join("SPEC.md")).expect("the SPEC");
    let (output, out) = run_from("floor", &home.join("SPEC.md"));
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("publishConfig.registry"));
    assert!(!out.join("package.json").exists());
    fs::remove_dir_all(&home).expect("cleaned");
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
            read(&out, &format!("src/generated/{file}")),
            golden,
            "{file}"
        );
    }
    fs::remove_dir_all(&out).expect("cleaned");
}

#[test]
fn the_generated_version_is_the_placeholder_never_the_crate_version() {
    let out = generate("version");
    let manifest: serde_json::Value =
        serde_json::from_str(&read(&out, "package.json")).expect("a manifest");
    assert_eq!(manifest["version"], "0.0.0");
    assert!(read(&out, "src/generated/init.ts").contains("version: \"0.0.0\""));
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
