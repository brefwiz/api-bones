// SPDX-License-Identifier: MIT
//! `wrapper kotlin` against the test service's Layer 2 crate: the files it
//! writes are the goldens under `fixtures/wrapper-kotlin`, and the bindgen
//! output it is given is copied into the package unchanged.
//!
//! The goldens are derived by hand from the generator's rules, not captured
//! from a run: no Kotlin compiler checks them, so a reviewer reads them as the
//! specification of the output.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

const GOLDEN_CLIENT: &str = include_str!("fixtures/wrapper-kotlin/ApiBonesClient.kt");
const GOLDEN_ERRORS: &str = include_str!("fixtures/wrapper-kotlin/Errors.kt");
const GOLDEN_PAGES: &str = include_str!("fixtures/wrapper-kotlin/Pages.kt");
const GOLDEN_GRADLE: &str = include_str!("fixtures/wrapper-kotlin/build.gradle.kts");
const GOLDEN_BUF: &str = include_str!("fixtures/wrapper-kotlin/buf.gen.yaml");
const GOLDEN_SURFACE: &str = include_str!("fixtures/wrapper-kotlin/api-surface.kt.txt");
const GOLDEN_METHODS: &str = include_str!("fixtures/wrapper-kotlin/l2-methods.json");
const BINDINGS_INPUT: &str = include_str!(
    "fixtures/wrapper-kotlin/bindings-input/uniffi/api_bones_test_service_l2/api_bones_test_service_l2.kt"
);
const SPEC: &str = include_str!("fixtures/wrapper-kotlin/spec.fixture.md");

/// The package directory of the SPEC's one kotlin package.
const ROOT: &str = "src/main/kotlin/com/brefwiz/apibones/testservice";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wrapper-kotlin")
}

/// The descriptor set of the test service's contract protos, with the shipped
/// `bones` protos they import, written once to a file.
fn descriptor() -> &'static Path {
    static FILE: OnceLock<PathBuf> = OnceLock::new();
    FILE.get_or_init(|| {
        let root = std::env::temp_dir().join(format!(
            "api-bones-sdk-gen-kotlin-{}-protos",
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

/// A scratch directory of its own, empty.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "api-bones-sdk-gen-kotlin-{}-{name}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("a directory");
    dir
}

fn spec_file(name: &str, text: &str) -> PathBuf {
    let path = scratch(&format!("{name}-spec")).join("SPEC.md");
    fs::write(&path, text).expect("a spec");
    path
}

fn wrapper_run(spec: &Path, bindings: &Path, out: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_api-bones-sdk-gen"))
        .args(["wrapper", "kotlin", "--crate"])
        .arg(repo().join("tests/l2"))
        .arg("--spec")
        .arg(spec)
        .arg("--descriptor")
        .arg(descriptor())
        .arg("--bindings")
        .arg(bindings)
        .arg("--out")
        .arg(out)
        .output()
        .expect("the generator runs")
}

fn generate(name: &str) -> PathBuf {
    let out = scratch(name);
    let output = wrapper_run(
        &spec_file(name, SPEC),
        &fixtures().join("bindings-input"),
        &out,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    out
}

fn refused(name: &str, spec: &str, bindings: &Path) -> String {
    let out = scratch(name);
    let output = wrapper_run(&spec_file(name, spec), bindings, &out);
    assert!(!output.status.success(), "the generator accepted {name}");
    assert!(
        !out.join("build.gradle.kts").exists(),
        "a refused run wrote a package"
    );
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn read(out: &Path, path: &str) -> String {
    fs::read_to_string(out.join(path)).expect("a generated file")
}

#[test]
fn the_client_written_is_the_golden_one() {
    let out = generate("client");
    assert_eq!(
        read(&out, &format!("{ROOT}/ApiBonesClient.kt")),
        GOLDEN_CLIENT
    );
}

#[test]
fn the_errors_and_pages_written_are_the_golden_ones() {
    let out = generate("support");
    assert_eq!(read(&out, &format!("{ROOT}/Errors.kt")), GOLDEN_ERRORS);
    assert_eq!(read(&out, &format!("{ROOT}/Pages.kt")), GOLDEN_PAGES);
}

#[test]
fn the_build_files_and_documents_written_are_the_golden_ones() {
    let out = generate("build");
    assert_eq!(read(&out, "build.gradle.kts"), GOLDEN_GRADLE);
    assert_eq!(read(&out, "buf.gen.yaml"), GOLDEN_BUF);
    assert_eq!(read(&out, "api-surface.kt.txt"), GOLDEN_SURFACE);
    assert_eq!(read(&out, "l2-methods.json"), GOLDEN_METHODS);
}

#[test]
fn the_bindgen_output_is_copied_unchanged_beside_the_client() {
    let out = generate("bindings");
    assert_eq!(
        read(
            &out,
            &format!("{ROOT}/ffi/uniffi/api_bones_test_service_l2/api_bones_test_service_l2.kt")
        ),
        BINDINGS_INPUT
    );
}

#[test]
fn a_package_file_names_no_root_pin_or_verifier() {
    let out = generate("custody");
    let files = [
        read(&out, &format!("{ROOT}/ApiBonesClient.kt")),
        read(&out, &format!("{ROOT}/Errors.kt")),
        read(&out, &format!("{ROOT}/Pages.kt")),
    ];
    for text in &files {
        let lower = text.to_lowercase();
        for word in [
            "trustroot",
            "cacert",
            "verifier",
            "insecure",
            "tls",
            "pinset",
        ] {
            assert!(!lower.contains(word), "a generated file names `{word}`");
        }
    }
    let client = &files[0];
    assert_eq!(
        client.matches("baseUrl").count(),
        2,
        "the address is taken only by the entry's constructor"
    );
    assert!(
        client.contains("class ApiBonesSdk(baseUrl: String, host: FfiHost)"),
        "{client}"
    );
}

#[test]
fn a_person_is_never_shown_the_diagnostic_message() {
    let out = generate("message");
    let errors = read(&out, &format!("{ROOT}/Errors.kt"));
    assert!(!errors.contains(".message"), "{errors}");
    assert!(errors.contains("Exception(call.code)"), "{errors}");
    assert!(errors.contains("hasRefusedRule"), "{errors}");
}

#[test]
fn a_spec_that_declares_no_kotlin_package_is_refused() {
    let spec = SPEC.replace(
        "      kotlin:\n        delivery: package\n        packages: [com.brefwiz.apibones.testservice]\n",
        "      typescript:\n        delivery: package\n        packages: [\"@brefwiz/x\"]\n",
    );
    let stderr = refused("none", &spec, &fixtures().join("bindings-input"));
    assert!(
        stderr.contains("no sdk_surfaces entry declares a kotlin target package"),
        "{stderr}"
    );
}

#[test]
fn a_spec_that_declares_two_kotlin_packages_is_refused() {
    let spec = SPEC.replace(
        "packages: [com.brefwiz.apibones.testservice]\n",
        "packages: [com.brefwiz.apibones.testservice, com.brefwiz.other]\n",
    );
    let stderr = refused("two", &spec, &fixtures().join("bindings-input"));
    assert!(stderr.contains("one package per language"), "{stderr}");
}

#[test]
fn a_package_that_is_not_a_lower_case_dotted_name_is_refused() {
    let spec = SPEC.replace("com.brefwiz.apibones.testservice", "Com.Brefwiz-Bad");
    let stderr = refused("name", &spec, &fixtures().join("bindings-input"));
    assert!(stderr.contains("not a lower-case dotted name"), "{stderr}");
}

#[test]
fn an_export_of_a_surface_that_is_no_kotlin_target_is_refused() {
    let spec = SPEC.replace(
        "---\nFixture",
        "  other:\n    targets:\n      kotlin:\n        delivery: package\n        packages: [com.brefwiz.apibones.testservice]\n---\nFixture",
    )
    .replace(
        "    targets:\n      kotlin:\n        delivery: package\n        packages: [com.brefwiz.apibones.testservice]\n  other:",
        "    targets:\n      typescript:\n        delivery: package\n        packages: [\"@brefwiz/x\"]\n  other:",
    );
    let stderr = refused("surface", &spec, &fixtures().join("bindings-input"));
    assert!(
        stderr.contains("is not a kotlin target of surface `sdk-call`"),
        "{stderr}"
    );
}

#[test]
fn an_empty_bindings_directory_is_refused() {
    let empty = scratch("empty-bindings");
    let stderr = refused("empty", SPEC, &empty);
    assert!(stderr.contains("holds no file"), "{stderr}");
}

#[test]
fn bindings_that_declare_no_ffi_handle_are_refused() {
    let bindings = scratch("handleless-bindings");
    fs::write(bindings.join("other.kt"), "package uniffi.other\n").expect("a file");
    let stderr = refused("handleless", SPEC, &bindings);
    assert!(stderr.contains("declare no FfiHandle"), "{stderr}");
}

#[test]
fn a_missing_bindings_directory_is_refused() {
    let missing = scratch("missing-parent").join("absent");
    let stderr = refused("absent", SPEC, &missing);
    assert!(stderr.contains("cannot read"), "{stderr}");
}
