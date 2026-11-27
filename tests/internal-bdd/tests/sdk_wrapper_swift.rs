// SPDX-License-Identifier: MIT
//! Runs `features/sdk_wrapper_swift.feature`.
//!
//! The Swift wrapper generator is build tooling no SDK call reaches (SPEC.md
//! `internal_behavior_owners`); each scenario runs it over the Layer 2 crate
//! in `tests/l2` with the SPEC and uniffi bindings under
//! `api-bones-sdk-gen/tests/fixtures/wrapper-swift`, altered by its steps.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use cucumber::{World, given, then, when};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn fixtures() -> PathBuf {
    Path::new(ROOT).join("api-bones-sdk-gen/tests/fixtures/wrapper-swift")
}

/// A scratch directory removed when the scenario's world drops.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "api-bones-sdk-wrapper-swift-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("a clock")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("a scratch directory");
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Default, World)]
struct SwiftWorld {
    scratch: Option<Scratch>,
    /// The SPEC text the generator reads.
    spec: String,
    bindings: Option<PathBuf>,
    /// A field the options message of the client declares beyond its own.
    extra_option: Option<String>,
    outcome: Option<Output>,
}

impl std::fmt::Debug for SwiftWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SwiftWorld").finish()
    }
}

impl SwiftWorld {
    fn dir(&mut self) -> PathBuf {
        self.scratch.get_or_insert_with(Scratch::new).0.clone()
    }

    fn replace_in_spec(&mut self, from: &str, to: &str) {
        let altered = self.spec.replacen(from, to, 1);
        assert_ne!(altered, self.spec, "`{from}` is rewritten");
        self.spec = altered;
    }

    /// The descriptor set of the test service's contract protos, with the
    /// declared extra options field, written under the scratch directory.
    fn descriptor(&mut self) -> PathBuf {
        let dir = self.dir();
        let includes = dir.join("includes");
        for (relative, bytes) in api_bones_protos::files() {
            let path = includes.join(relative);
            fs::create_dir_all(path.parent().expect("a directory")).expect("created");
            fs::write(path, bytes).expect("a proto");
        }
        let contract = dir.join("contract");
        let source = Path::new(ROOT).join("tests/l2/proto");
        let mut files = Vec::new();
        copy_protos(&source, &source, &contract, &mut files);
        if let Some(field) = &self.extra_option {
            let numbers = contract.join("contract/sdk/v1/numbers.proto");
            let text = fs::read_to_string(&numbers).expect("the proto");
            let altered = text.replacen(
                "optional uint32 default_page_size = 1;",
                &format!("optional uint32 default_page_size = 1;\n  optional string {field} = 2;"),
                1,
            );
            assert_ne!(altered, text, "the options message is rewritten");
            fs::write(&numbers, altered).expect("the proto");
        }
        let mut compiler = protox::Compiler::new([&contract, &includes]).expect("include paths");
        compiler.include_imports(true);
        compiler.open_files(&files).expect("the protos compile");
        let file = dir.join("l2.fds");
        fs::write(&file, compiler.encode_file_descriptor_set()).expect("a descriptor set");
        file
    }

    fn out(&mut self) -> PathBuf {
        self.dir().join("out")
    }
}

fn copy_protos(root: &Path, dir: &Path, to: &Path, files: &mut Vec<String>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .expect("a proto directory")
        .map(|entry| entry.expect("an entry").path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            copy_protos(root, &path, to, files);
        } else if path.extension().is_some_and(|ext| ext == "proto") {
            let relative = path.strip_prefix(root).expect("under the root");
            let target = to.join(relative);
            fs::create_dir_all(target.parent().expect("a directory")).expect("a directory");
            fs::copy(&path, &target).expect("a copied proto");
            files.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
}

fn the_spec(packages: &str) -> String {
    let spec = fs::read_to_string(fixtures().join("spec.md")).expect("the SPEC");
    spec.replacen(
        "packages: [ApiBonesTestService]",
        &format!("packages: [{packages}]"),
        1,
    )
}

#[given(expr = "the test service crate and the SPEC that lists the swift package {string}")]
#[given(expr = "the test service crate and the SPEC that lists the swift packages {string}")]
fn given_spec(world: &mut SwiftWorld, packages: String) {
    world.spec = the_spec(&packages);
}

#[given("the test service crate and the SPEC that lists no swift package")]
fn given_no_package(world: &mut SwiftWorld) {
    world.spec = the_spec("");
}

#[given(expr = "the surface names the endpoint {string}")]
fn given_unserved(world: &mut SwiftWorld, endpoint: String) {
    world.replace_in_spec(
        "    l2_methods:\n",
        &format!("    l2_methods:\n      {endpoint}:\n        rust: [unserved]\n"),
    );
}

#[given(expr = "the SPEC renames the surface {string} to {string}")]
fn given_renamed(world: &mut SwiftWorld, from: String, to: String) {
    world.replace_in_spec(&format!("  {from}:\n"), &format!("  {to}:\n"));
}

#[given(expr = "the options message of the client declares the field {string}")]
fn given_option(world: &mut SwiftWorld, field: String) {
    world.extra_option = Some(field);
}

#[given("the uniffi bindings of the test service")]
fn given_bindings(world: &mut SwiftWorld) {
    world.bindings = Some(fixtures().join("bindings"));
}

#[given(expr = "the uniffi bindings of the test service without the function {string}")]
fn given_bindings_without(world: &mut SwiftWorld, function: String) {
    let dir = world.dir().join("bindings");
    fs::create_dir_all(&dir).expect("a directory");
    for name in ["api_bones_sdk_ffi.swift", "api_bones_test_service_l2.swift"] {
        let text = fs::read_to_string(fixtures().join("bindings").join(name)).expect("a binding");
        fs::write(
            dir.join(name),
            text.replace(&function, &format!("{function}Renamed")),
        )
        .expect("a binding");
    }
    world.bindings = Some(dir);
}

#[given("an empty bindings directory")]
fn given_empty_bindings(world: &mut SwiftWorld) {
    let dir = world.dir().join("bindings");
    fs::create_dir_all(&dir).expect("a directory");
    world.bindings = Some(dir);
}

#[when("the Swift wrapper is generated")]
fn when_generated(world: &mut SwiftWorld) {
    let descriptor = world.descriptor();
    let spec = world.dir().join("SPEC.md");
    fs::write(&spec, &world.spec).expect("the SPEC");
    let out = world.out();
    let mut command = Command::new(env!("CARGO"));
    command
        .args(["run", "--locked", "-q", "-p", "api-bones-sdk-gen", "--"])
        .args(["wrapper", "swift", "--crate"])
        .arg(Path::new(ROOT).join("tests/l2"))
        .arg("--spec")
        .arg(&spec)
        .arg("--descriptor")
        .arg(&descriptor)
        .arg("--out")
        .arg(&out)
        .current_dir(ROOT);
    if let Some(bindings) = &world.bindings {
        command.arg("--bindings").arg(bindings);
    }
    world.outcome = Some(command.output().expect("the generator runs"));
}

fn outcome(world: &SwiftWorld) -> &Output {
    world.outcome.as_ref().expect("a generation ran")
}

#[then("the generation succeeded")]
fn then_succeeded(world: &mut SwiftWorld) {
    let output = outcome(world);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[then(expr = "the package holds the file {string}")]
fn then_holds(world: &mut SwiftWorld, relative: String) {
    assert!(world.out().join(&relative).is_file(), "{relative}");
}

#[then("the client of the package takes no root, pin or verifier")]
fn then_no_roots(world: &mut SwiftWorld) {
    let client = fs::read_to_string(
        world
            .out()
            .join("Sources/ApiBonesTestService/ApiBonesClient.swift"),
    )
    .expect("the client")
    .to_lowercase();
    for forbidden in ["root", "cert", "pin", "verif", "tls", "insecure"] {
        assert!(
            !client.contains(forbidden),
            "the client mentions `{forbidden}`"
        );
    }
}

#[then(expr = "the generation is refused with {string}")]
fn then_refused(world: &mut SwiftWorld, message: String) {
    let output = outcome(world);
    assert!(!output.status.success(), "the generation succeeded");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(&message), "{stderr}");
    assert!(!world.out().exists(), "a package was written");
}

#[then(expr = "the refusal says {string}")]
fn then_says(world: &mut SwiftWorld, message: String) {
    let stderr = String::from_utf8_lossy(&outcome(world).stderr).into_owned();
    assert!(stderr.contains(&message), "{stderr}");
}

#[tokio::main]
async fn main() {
    let features = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/features/sdk_wrapper_swift.feature"
    );
    SwiftWorld::run(features).await;
}
