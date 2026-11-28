// SPDX-License-Identifier: MIT
//! `wrapper typescript` against the test service's Layer 2 crate: the client
//! it writes is the golden one, and the manifest, `l2-methods.json` and
//! `api-surface.d.ts` it writes are the ones tracked beside the package.

use std::fs;
use std::path::{Path, PathBuf};

use std::process::Command;

mod common;

use common::{descriptor, repo};

/// The digest the generator is told the module has; the golden `init.ts`
/// states it.
const STAMP: &str = "abababababababababababababababababababababababababababababababab";

const GOLDEN_CLIENT: &str = include_str!("fixtures/wrapper-ts/client.ts");
const GOLDEN_TYPES: &str = include_str!("fixtures/wrapper-ts/types.ts");
const GOLDEN_ERRORS: &str = include_str!("fixtures/wrapper-ts/errors.ts");
const GOLDEN_INDEX: &str = include_str!("fixtures/wrapper-ts/index.ts");
const GOLDEN_INIT: &str = include_str!("fixtures/wrapper-ts/init.ts");
const TRACKED_METHODS: &str = include_str!("../../api-bones-test-service-sdk-ts/l2-methods.json");
const TRACKED_SURFACE: &str = include_str!("../../api-bones-test-service-sdk-ts/api-surface.d.ts");
const TRACKED_MANIFEST: &str = include_str!("../../api-bones-test-service-sdk-ts/package.json");

fn generate(name: &str) -> PathBuf {
    generate_from(name, &repo().join("SPEC.md"))
}

/// The package the SPEC lists for the test service's Layer 2 crate.
const PACKAGE: &str = "@brefwiz/api-bones-test-service-sdk";

/// A git work tree at `out` that tracks a `package.json` named `name`, the way
/// a repository tracks the manifest of a wrapper.
fn track_manifest(out: &Path, name: &str) {
    fs::create_dir_all(out).expect("a directory");
    fs::write(out.join("package.json"), format!("{{\"name\":\"{name}\"}}")).expect("a manifest");
    for args in [["init", "-q"], ["add", "package.json"]] {
        let status = Command::new("git")
            .arg("-C")
            .arg(out)
            .args(args)
            .status()
            .expect("git runs");
        assert!(status.success(), "git {args:?}");
    }
}

fn wrapper_run(krate: &Path, spec: &Path, out: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_api-bones-sdk-gen"))
        .args(["wrapper", "typescript", "--crate"])
        .arg(krate)
        .arg("--spec")
        .arg(spec)
        .args(["--wasm-sha256", STAMP, "--descriptor"])
        .arg(descriptor())
        .arg("--out")
        .arg(out)
        .output()
        .expect("the generator runs")
}

fn run_from(name: &str, spec: &Path) -> (std::process::Output, PathBuf) {
    let out = std::env::temp_dir().join(format!(
        "api-bones-sdk-gen-wrapper-{}-{name}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&out);
    track_manifest(&out, PACKAGE);
    let output = wrapper_run(&repo().join("tests/l2"), spec, &out);
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
fn a_tracked_snapshot_missing_a_field_is_refused_and_left_untouched() {
    let out = std::env::temp_dir().join(format!(
        "api-bones-sdk-gen-wrapper-{}-drift",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&out);
    track_manifest(&out, PACKAGE);
    let tracked = fs::read_to_string(repo().join("api-bones-test-service-sdk-ts/l2-methods.json"))
        .expect("the tracked snapshot");
    let stripped: String = tracked
        .lines()
        .filter(|line| !line.trim_start().starts_with("\"kind\":"))
        .map(|line| format!("{line}\n"))
        .collect();
    assert_ne!(stripped, tracked);
    fs::write(out.join("l2-methods.json"), &stripped).expect("a drifted snapshot");
    let output = wrapper_run(&repo().join("tests/l2"), &repo().join("SPEC.md"), &out);
    assert!(!output.status.success(), "a drifted snapshot was replaced");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("missing field `kind`"), "{stderr}");
    assert_eq!(read(&out, "l2-methods.json"), stripped);
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
        ">=0.3.0"
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
        "api-bones-sdk-gen-wrapper-{}-lone-spec-home",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(&home).expect("a directory");
    fs::copy(repo().join("SPEC.md"), home.join("SPEC.md")).expect("the SPEC");
    let (output, out) = run_from("floor", &home.join("SPEC.md"));
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("publishConfig.registry"));
    assert!(!read(&out, "package.json").contains("version"));
    fs::remove_dir_all(&out).expect("cleaned");
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

fn refusal(name: &str, spec: &Path, package: Option<&str>, tracked: bool) -> String {
    let out = std::env::temp_dir().join(format!(
        "api-bones-sdk-gen-wrapper-{}-{name}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&out);
    if let Some(package) = package {
        track_manifest(&out, package);
        if !tracked {
            let status = Command::new("git")
                .arg("-C")
                .arg(&out)
                .args(["rm", "-q", "--cached", "package.json"])
                .status()
                .expect("git runs");
            assert!(status.success());
        }
    }
    let output = wrapper_run(&repo().join("tests/l2"), spec, &out);
    assert!(!output.status.success());
    let _ = fs::remove_dir_all(&out);
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn a_package_json_that_is_missing_or_untracked_fails_the_command() {
    let spec = repo().join("SPEC.md");
    let missing = refusal("no-manifest", &spec, None, true);
    assert!(
        missing
            .contains("package.json: the wrapper package is the name in the tracked package.json"),
        "{missing}"
    );
    let untracked = refusal("untracked-manifest", &spec, Some(PACKAGE), false);
    assert!(
        untracked.contains(
            "package.json is not tracked by git: the wrapper package is the name in the tracked package.json"
        ),
        "{untracked}"
    );
}

#[test]
fn a_package_the_spec_lists_for_no_surface_fails_the_command() {
    let stderr = refusal(
        "unlisted-package",
        &repo().join("SPEC.md"),
        Some("@brefwiz/other"),
        true,
    );
    assert!(
        stderr.contains(
            "no sdk_surfaces entry lists `@brefwiz/other` as a typescript target package"
        ),
        "{stderr}"
    );
}

/// The in-tree runtime package beside a copied SPEC: a published wrapper takes
/// its registry from it, so a SPEC without it would be refused before the
/// listing checks run.
fn link_runtime(home: &Path) {
    let runtime = home.join("api-bones-sdk-runtime-ts");
    fs::create_dir_all(&runtime).expect("a directory");
    fs::copy(
        repo().join("api-bones-sdk-runtime-ts/package.json"),
        runtime.join("package.json"),
    )
    .expect("the runtime manifest");
}

#[test]
fn a_package_that_one_exporting_surface_does_not_list_fails_the_command() {
    let spec = fs::read_to_string(repo().join("SPEC.md")).expect("the SPEC");
    let home = std::env::temp_dir().join(format!(
        "api-bones-sdk-gen-wrapper-{}-one-surface-home",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(&home).expect("a directory");
    // The sdk-wasm-load surface lists the package; sdk-call stops listing it.
    let altered = spec.replacen(
        "      typescript:\n        delivery: package\n        packages: [\"@brefwiz/api-bones-test-service-sdk\"]\n  # Rust callers use",
        "      typescript:\n        delivery: package\n        packages: [\"@brefwiz/elsewhere\"]\n  # Rust callers use",
        1,
    );
    assert_ne!(altered, spec, "the sdk-call surface target is rewritten");
    fs::write(home.join("SPEC.md"), altered).expect("the SPEC");
    link_runtime(&home);
    let stderr = refusal("one-surface", &home.join("SPEC.md"), Some(PACKAGE), true);
    assert!(
        stderr.contains(
            "package `@brefwiz/api-bones-test-service-sdk` is not a typescript target of surface `sdk-call`, which the exports of `api-bones-test-service-l2` name"
        ),
        "{stderr}"
    );
    fs::remove_dir_all(&home).expect("cleaned");
}

#[test]
fn a_listing_surface_with_an_endpoint_the_crate_does_not_serve_fails_the_command() {
    let spec = fs::read_to_string(repo().join("SPEC.md")).expect("the SPEC");
    let home = std::env::temp_dir().join(format!(
        "api-bones-sdk-gen-wrapper-{}-uncovered-home",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(&home).expect("a directory");
    let altered = spec.replacen(
        "  sdk-call:\n    contract: tests/sdk-call/features/sdk_call.feature\n    l2_methods:\n",
        "  sdk-call:\n    contract: tests/sdk-call/features/sdk_call.feature\n    l2_methods:\n      contract.v1.NumbersService/Unserved:\n        rust: [unserved]\n        typescript: [unserved]\n",
        1,
    );
    assert_ne!(altered, spec, "the sdk-call endpoints are extended");
    fs::write(home.join("SPEC.md"), altered).expect("the SPEC");
    link_runtime(&home);
    let stderr = refusal("uncovered", &home.join("SPEC.md"), Some(PACKAGE), true);
    assert!(
        stderr.contains(
            "surface `sdk-call` lists package `@brefwiz/api-bones-test-service-sdk` and names endpoint `contract.v1.NumbersService/Unserved`, which no export of `api-bones-test-service-l2` serves"
        ),
        "{stderr}"
    );
    fs::remove_dir_all(&home).expect("cleaned");
}

#[test]
fn size_reports_the_schema_and_the_byte_count() {
    let dir = std::env::temp_dir().join(format!("api-bones-sdk-gen-size-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("ts")).expect("a directory");
    fs::write(dir.join("m.wasm"), [0u8; 12]).expect("a module");
    fs::write(dir.join("glue.js"), "export {};\n").expect("a glue");
    fs::write(dir.join("ts/client.ts"), "export {};\n").expect("a source");
    let output = Command::new(env!("CARGO_BIN_EXE_api-bones-sdk-gen"))
        .args(["wrapper", "size", "--crate-name", "numbers", "--wasm"])
        .arg(dir.join("m.wasm"))
        .arg("--glue")
        .arg(dir.join("glue.js"))
        .arg("--typescript")
        .arg(dir.join("ts"))
        .output()
        .expect("the generator runs");
    fs::remove_dir_all(&dir).expect("cleaned");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let document: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("a JSON document");
    assert_eq!(document["schema"], "api-bones.sdk-size/v1");
    assert_eq!(document["crate"], "numbers");
    assert_eq!(document["wasm"]["raw"], 12);
    assert_eq!(document["glue"]["raw"], 11);
    assert_eq!(document["typescript"]["raw"], 11);
}
