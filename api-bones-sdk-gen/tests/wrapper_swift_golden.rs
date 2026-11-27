// SPDX-License-Identifier: MIT
//! Diffs the tree `wrapper swift` emits for `tests/l2` against
//! `fixtures/wrapper-swift`, and asserts each refusal.
//!
//! `fixtures/wrapper-swift/golden` is the expected package and
//! `fixtures/wrapper-swift/bindings` the uniffi bindgen output the package
//! copies. A golden that drifts from the generator fails here.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

const PACKAGE_SWIFT: &str = include_str!("fixtures/wrapper-swift/golden/Package.swift");
const BUF_GEN: &str = include_str!("fixtures/wrapper-swift/golden/buf.gen.yaml");
const METHODS: &str = include_str!("fixtures/wrapper-swift/golden/l2-methods.json");
const SURFACE: &str = include_str!("fixtures/wrapper-swift/golden/api-surface.swift.txt");
const CLIENT: &str =
    include_str!("fixtures/wrapper-swift/golden/Sources/ApiBonesTestService/ApiBonesClient.swift");
const ERRORS: &str =
    include_str!("fixtures/wrapper-swift/golden/Sources/ApiBonesTestService/Errors.swift");
const PAGES: &str =
    include_str!("fixtures/wrapper-swift/golden/Sources/ApiBonesTestService/Pages.swift");
const SDK_FFI: &str = include_str!("fixtures/wrapper-swift/bindings/api_bones_sdk_ffi.swift");
const L2_FFI: &str =
    include_str!("fixtures/wrapper-swift/bindings/api_bones_test_service_l2.swift");

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wrapper-swift")
}

/// The descriptor set of the test service's contract protos, with the shipped
/// `bones` protos they import, written once to a file.
fn descriptor() -> &'static Path {
    static FILE: OnceLock<PathBuf> = OnceLock::new();
    FILE.get_or_init(|| {
        let root = std::env::temp_dir().join(format!(
            "api-bones-sdk-gen-wrapper-swift-{}-protos",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        for (relative, bytes) in api_bones_protos::files() {
            let path = root.join(relative);
            fs::create_dir_all(path.parent().expect("a directory")).expect("created");
            fs::write(path, bytes).expect("a proto");
        }
        let contract = repo().join("tests/l2/proto");
        let mut files = Vec::new();
        protos_under(&contract, &contract, &mut files);
        let mut compiler = protox::Compiler::new([&contract, &root]).expect("include paths");
        compiler.include_imports(true);
        compiler
            .open_files(&files)
            .expect("the contract protos compile");
        let file = root.join("l2.fds");
        fs::write(&file, compiler.encode_file_descriptor_set()).expect("a descriptor set");
        file
    })
}

fn protos_under(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .expect("a proto directory")
        .map(|entry| entry.expect("an entry").path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            protos_under(root, &path, out);
        } else if path.extension().is_some_and(|ext| ext == "proto") {
            let relative = path.strip_prefix(root).expect("under the root");
            out.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "api-bones-sdk-gen-wrapper-swift-{}-{name}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    dir
}

fn wrapper_run(spec: &Path, bindings: Option<&Path>, out: &Path) -> std::process::Output {
    wrapper_run_with(spec, bindings, descriptor(), out)
}

fn wrapper_run_with(
    spec: &Path,
    bindings: Option<&Path>,
    descriptor: &Path,
    out: &Path,
) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_api-bones-sdk-gen"));
    command
        .args(["wrapper", "swift", "--crate"])
        .arg(repo().join("tests/l2"))
        .arg("--spec")
        .arg(spec)
        .arg("--descriptor")
        .arg(descriptor)
        .arg("--out")
        .arg(out);
    if let Some(bindings) = bindings {
        command.arg("--bindings").arg(bindings);
    }
    command.output().expect("the generator runs")
}

fn generate(name: &str) -> PathBuf {
    let out = scratch(name);
    let output = wrapper_run(
        &fixtures().join("spec.md"),
        Some(&fixtures().join("bindings")),
        &out,
    );
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

fn files_under(root: &Path, dir: &Path, out: &mut BTreeSet<String>) {
    for entry in fs::read_dir(dir).expect("a directory") {
        let path = entry.expect("an entry").path();
        if path.is_dir() {
            files_under(root, &path, out);
        } else {
            let relative = path.strip_prefix(root).expect("under the root");
            out.insert(relative.to_string_lossy().replace('\\', "/"));
        }
    }
}

#[test]
fn the_package_written_is_the_golden_one() {
    let out = generate("golden");
    for (path, golden) in [
        ("Package.swift", PACKAGE_SWIFT),
        ("buf.gen.yaml", BUF_GEN),
        ("l2-methods.json", METHODS),
        ("api-surface.swift.txt", SURFACE),
        ("Sources/ApiBonesTestService/ApiBonesClient.swift", CLIENT),
        ("Sources/ApiBonesTestService/Errors.swift", ERRORS),
        ("Sources/ApiBonesTestService/Pages.swift", PAGES),
        (
            "Sources/ApiBonesTestServiceFFI/api_bones_sdk_ffi.swift",
            SDK_FFI,
        ),
        (
            "Sources/ApiBonesTestServiceFFI/api_bones_test_service_l2.swift",
            L2_FFI,
        ),
    ] {
        assert_eq!(read(&out, path), golden, "{path}");
    }
    fs::remove_dir_all(&out).expect("cleaned");
}

#[test]
fn nothing_else_is_written() {
    let out = generate("files");
    let mut written = BTreeSet::new();
    files_under(&out, &out, &mut written);
    let expected: BTreeSet<String> = [
        "Package.swift",
        "buf.gen.yaml",
        "l2-methods.json",
        "api-surface.swift.txt",
        "Sources/ApiBonesTestService/ApiBonesClient.swift",
        "Sources/ApiBonesTestService/Errors.swift",
        "Sources/ApiBonesTestService/Pages.swift",
        "Sources/ApiBonesTestServiceFFI/api_bones_sdk_ffi.swift",
        "Sources/ApiBonesTestServiceFFI/api_bones_test_service_l2.swift",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    assert_eq!(written, expected);
    fs::remove_dir_all(&out).expect("cleaned");
}

#[test]
fn the_snapshot_is_the_typescript_one_under_the_swift_package() {
    let tracked = include_str!("../../api-bones-test-service-sdk-ts/l2-methods.json");
    assert_eq!(
        METHODS,
        tracked.replace("@brefwiz/api-bones-test-service-sdk", "ApiBonesTestService")
    );
}

#[test]
fn no_wrapper_api_takes_roots_pins_or_a_verifier() {
    for (file, text) in [("client", CLIENT), ("api surface", SURFACE)] {
        let lowered = text.to_lowercase();
        for forbidden in ["root", "cert", "pin", "verif", "tls", "insecure"] {
            assert!(
                !lowered.contains(forbidden),
                "the {file} mentions `{forbidden}`"
            );
        }
    }
    assert_eq!(CLIENT.matches("baseUrl: String").count(), 4);
}

#[test]
fn an_options_field_that_carries_a_ca_is_refused() {
    let contract = scratch("forbidden-option-protos");
    let source = repo().join("tests/l2/proto");
    let mut files = Vec::new();
    protos_under(&source, &source, &mut files);
    for relative in &files {
        let target = contract.join(relative);
        fs::create_dir_all(target.parent().expect("a directory")).expect("a directory");
        fs::copy(source.join(relative), &target).expect("a copied proto");
    }
    let numbers = contract.join("contract/sdk/v1/numbers.proto");
    let text = fs::read_to_string(&numbers).expect("the proto");
    let altered = text.replacen(
        "optional uint32 default_page_size = 1;",
        "optional uint32 default_page_size = 1;\n  optional string ca_pem = 2;",
        1,
    );
    assert_ne!(altered, text, "the options message is rewritten");
    fs::write(&numbers, altered).expect("the proto");
    let includes = scratch("forbidden-option-includes");
    for (relative, bytes) in api_bones_protos::files() {
        let path = includes.join(relative);
        fs::create_dir_all(path.parent().expect("a directory")).expect("created");
        fs::write(path, bytes).expect("a proto");
    }
    let mut compiler = protox::Compiler::new([&contract, &includes]).expect("include paths");
    compiler.include_imports(true);
    compiler.open_files(&files).expect("the protos compile");
    let set = scratch("forbidden-option-set");
    fs::create_dir_all(&set).expect("a directory");
    let descriptor = set.join("l2.fds");
    fs::write(&descriptor, compiler.encode_file_descriptor_set()).expect("a descriptor set");
    let out = scratch("forbidden-option-out");
    let output = wrapper_run_with(
        &fixtures().join("spec.md"),
        Some(&fixtures().join("bindings")),
        &descriptor,
        &out,
    );
    assert!(!output.status.success(), "the options field was accepted");
    assert!(!out.exists(), "a package was written");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("option `caPem`"), "{stderr}");
    assert!(
        stderr.contains("takes no root, pin or verifier"),
        "{stderr}"
    );
    for dir in [contract, includes, set] {
        fs::remove_dir_all(dir).expect("cleaned");
    }
}

/// A copy of the fixture SPEC with `from` replaced by `to`, written under a
/// directory of its own.
fn altered_spec(name: &str, from: &str, to: &str) -> PathBuf {
    let spec = fs::read_to_string(fixtures().join("spec.md")).expect("the SPEC");
    let altered = spec.replacen(from, to, 1);
    assert_ne!(altered, spec, "`{from}` is rewritten");
    let dir = scratch(name);
    fs::create_dir_all(&dir).expect("a directory");
    let path = dir.join("SPEC.md");
    fs::write(&path, altered).expect("the SPEC");
    path
}

fn refusal(name: &str, spec: &Path, bindings: Option<&Path>) -> String {
    let out = scratch(&format!("{name}-out"));
    let output = wrapper_run(spec, bindings, &out);
    assert!(!output.status.success(), "{name} was not refused");
    assert!(!out.exists(), "{name} wrote a package");
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn a_run_without_bindings_is_refused() {
    let stderr = refusal("no-bindings", &fixtures().join("spec.md"), None);
    assert!(stderr.contains("pass `--bindings <dir>`"), "{stderr}");
}

#[test]
fn an_empty_bindings_directory_is_refused() {
    let empty = scratch("empty-bindings");
    fs::create_dir_all(&empty).expect("a directory");
    let stderr = refusal("empty", &fixtures().join("spec.md"), Some(&empty));
    assert!(stderr.contains("holds no file"), "{stderr}");
    fs::remove_dir_all(&empty).expect("cleaned");
}

#[test]
fn bindings_that_lack_a_function_the_package_calls_are_refused() {
    let dir = scratch("missing-symbol");
    fs::create_dir_all(&dir).expect("a directory");
    fs::write(dir.join("api_bones_sdk_ffi.swift"), SDK_FFI).expect("a binding");
    fs::write(
        dir.join("api_bones_test_service_l2.swift"),
        L2_FFI.replace("apiBonesEventsClientWatch", "apiBonesEventsClientWatched"),
    )
    .expect("a binding");
    let stderr = refusal("missing-symbol", &fixtures().join("spec.md"), Some(&dir));
    assert!(
        stderr.contains("func apiBonesEventsClientWatch"),
        "{stderr}"
    );
    assert!(!stderr.contains("class NumbersClientFfi"), "{stderr}");
    fs::remove_dir_all(&dir).expect("cleaned");
}

#[test]
fn a_spec_that_lists_no_swift_package_is_refused() {
    let spec = altered_spec(
        "no-package",
        "      swift:\n        delivery: package\n        packages: [ApiBonesTestService]\n",
        "      swift:\n        delivery: package\n        packages: []\n",
    );
    let stderr = refusal("no-package", &spec, Some(&fixtures().join("bindings")));
    assert!(
        stderr.contains("no sdk_surfaces entry lists a swift target package"),
        "{stderr}"
    );
}

#[test]
fn two_swift_packages_are_refused() {
    let spec = altered_spec(
        "two-packages",
        "packages: [ApiBonesTestService]\n",
        "packages: [ApiBonesTestService, Other]\n",
    );
    let stderr = refusal("two-packages", &spec, Some(&fixtures().join("bindings")));
    assert!(stderr.contains("one crate is one package"), "{stderr}");
}

#[test]
fn an_endpoint_the_crate_does_not_serve_is_refused() {
    let spec = altered_spec(
        "unserved",
        "    l2_methods:\n",
        "    l2_methods:\n      contract.v1.NumbersService/Unserved:\n        rust: [unserved]\n",
    );
    let stderr = refusal("unserved", &spec, Some(&fixtures().join("bindings")));
    assert!(
        stderr.contains(
            "names endpoint `contract.v1.NumbersService/Unserved`, which no export of `api-bones-test-service-l2` serves"
        ),
        "{stderr}"
    );
}

#[test]
fn an_export_on_a_surface_that_does_not_list_the_package_is_refused() {
    let spec = altered_spec("unknown-surface", "  sdk-call:\n", "  other-surface:\n");
    let stderr = refusal("unknown-surface", &spec, Some(&fixtures().join("bindings")));
    assert!(
        stderr.contains(
            "package `ApiBonesTestService` is not a swift target of surface `sdk-call`, which the exports of `api-bones-test-service-l2` name"
        ),
        "{stderr}"
    );
}
