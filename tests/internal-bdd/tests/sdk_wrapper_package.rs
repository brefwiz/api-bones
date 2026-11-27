// SPDX-License-Identifier: MIT
//! Runs `features/sdk_wrapper_package.feature`.
//!
//! The wrapper generator is build tooling no SDK call reaches (SPEC.md
//! `internal_behavior_owners`); each scenario writes a service of Layer 2
//! crates to a scratch directory and runs the generator over it.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use cucumber::{World, given, then, when};

/// A scratch directory holding one service: its SPEC, its crates and the
/// output directory of each crate's wrapper.
struct Service(PathBuf);

impl Service {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "api-bones-sdk-wrapper-package-{}-{}",
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

impl Drop for Service {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Default, World)]
struct WrapperWorld {
    service: Option<Service>,
    /// Each crate with the package its tracked manifest names.
    crates: BTreeMap<String, String>,
    /// The packages the SPEC lists, when it lists fewer than the crates name.
    spec_packages: Option<Vec<String>>,
    /// The surfaces whose TypeScript target is another package than the
    /// crate's own, by surface id.
    retargeted: BTreeMap<String, String>,
    /// Endpoints a surface names that its crate does not serve.
    unserved: BTreeMap<String, String>,
    /// Async exports of a crate: (Type, surface, Rust method), by crate.
    exports: BTreeMap<String, Vec<(String, String, String)>>,
    /// Endpoints a surface serves: (endpoint, Rust method), by surface.
    served: BTreeMap<String, Vec<(String, String)>>,
    outcomes: BTreeMap<String, Output>,
}

impl std::fmt::Debug for WrapperWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WrapperWorld").finish()
    }
}

impl WrapperWorld {
    fn root(&mut self) -> PathBuf {
        self.service.get_or_insert_with(Service::new).0.clone()
    }

    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .status()
            .expect("git runs");
        assert!(status.success(), "git {args:?}");
    }

    /// The SPEC: one surface per crate, whose TypeScript target is the
    /// crate's package unless a step moved it.
    fn write_spec(&mut self) {
        let mut spec = String::from("---\nservice: acme\nsdk_surfaces:\n");
        for (name, package) in &self.crates {
            let surface = format!("s-{name}");
            let target = self.retargeted.get(&surface).unwrap_or(package);
            if self
                .spec_packages
                .as_ref()
                .is_some_and(|listed| !listed.contains(target))
            {
                continue;
            }
            spec.push_str(&self.surface_entry(&surface, target));
        }
        // A surface an export names beyond the crate's own default.
        let mut extra: Vec<(String, String)> = Vec::new();
        for (name, list) in &self.exports {
            for (_, surface, _) in list {
                if !self
                    .crates
                    .keys()
                    .any(|krate| format!("s-{krate}") == *surface)
                {
                    extra.push((surface.clone(), self.crates[name].clone()));
                }
            }
        }
        let mut seen = Vec::new();
        for (surface, package) in extra {
            if !seen.contains(&surface) {
                spec.push_str(&self.surface_entry(&surface, &package));
                seen.push(surface);
            }
        }
        // A surface that does not export keeps the crate's own package listed.
        for (name, package) in &self.crates {
            if self.retargeted.contains_key(&format!("s-{name}")) {
                spec.push_str(&format!(
                    "  other-{name}:\n    targets:\n      typescript:\n        packages: [\"{package}\"]\n"
                ));
            }
        }
        spec.push_str("---\n");
        let root = self.root();
        fs::write(root.join("SPEC.md"), spec).expect("a SPEC");
    }

    fn surface_entry(&self, surface: &str, target: &str) -> String {
        let mut entry = format!("  {surface}:\n");
        let served = self.served.get(surface);
        if self.unserved.contains_key(surface) || served.is_some() {
            entry.push_str("    l2_methods:\n");
            if let Some(endpoint) = self.unserved.get(surface) {
                entry.push_str(&format!(
                    "      {endpoint}:\n        rust: [unserved]\n        typescript: [unserved]\n"
                ));
            }
            for (endpoint, method) in served.into_iter().flatten() {
                entry.push_str(&format!(
                    "      {endpoint}:\n        rust: [{method}]\n        typescript: [{method}]\n"
                ));
            }
        }
        entry.push_str(&format!(
            "    targets:\n      typescript:\n        packages: [\"{target}\"]\n"
        ));
        entry
    }

    fn write_exports(&mut self, name: &str) {
        let Some(list) = self.exports.get(name).cloned() else {
            return;
        };
        let source: String = list
            .iter()
            .map(|(ty, surface, method)| {
                format!(
                    "#[client(surface = \"{surface}\")]\nimpl {ty} {{\n    #[export]\n    pub async fn {method}(&self) -> Result<generated::acme::v1::Empty, Error> {{ body() }}\n}}\n"
                )
            })
            .collect();
        let path = self.root().join("crates").join(name).join("src/lib.rs");
        fs::write(path, source).expect("a source");
    }

    fn write_crate(&mut self, name: &str, wasm_of: Option<&str>) {
        let dir = self.root().join("crates").join(name);
        fs::create_dir_all(dir.join("src")).expect("a crate");
        let (features, dependencies) = match wasm_of {
            Some(other) => (
                format!("[features]\nwasm = [\"{other}/wasm\"]\n"),
                format!("[dependencies]\n{other} = {{ path = \"../{other}\", optional = true }}\n"),
            ),
            None => ("[features]\nwasm = []\n".to_owned(), String::new()),
        };
        fs::write(
            dir.join("Cargo.toml"),
            format!(
                "[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2024\"\nlicense = \"MIT\"\npublish = false\n\n{features}\n{dependencies}"
            ),
        )
        .expect("a manifest");
        let client = format!("{}Client", name.to_uppercase());
        fs::write(
            dir.join("src/lib.rs"),
            format!(
                "#[client(surface = \"s-{name}\")]\nimpl {client} {{\n    #[export(sync)]\n    pub fn label(&self) -> String {{ body() }}\n}}\n"
            ),
        )
        .expect("a source");
    }

    fn out(&mut self, name: &str) -> PathBuf {
        self.root().join(format!("{name}-sdk"))
    }

    fn generate(&mut self, name: &str) {
        self.write_exports(name);
        self.write_spec();
        let root = self.root();
        let descriptor = root.join("contract.fds");
        let protos = root.join("proto");
        fs::create_dir_all(protos.join("acme/v1")).expect("a proto directory");
        fs::write(
            protos.join("acme/v1/acme.proto"),
            "syntax = \"proto3\";\npackage acme.v1;\nimport \"google/protobuf/empty.proto\";\nimport \"google/protobuf/wrappers.proto\";\nmessage Empty {}\n",
        )
        .expect("a proto");
        let mut compiler = protox::Compiler::new([&protos]).expect("include paths");
        compiler.include_imports(true);
        compiler
            .open_file("acme/v1/acme.proto")
            .expect("a valid proto");
        fs::write(&descriptor, compiler.encode_file_descriptor_set()).expect("a descriptor set");
        let output = Command::new(env!("CARGO"))
            .args(["run", "--locked", "-q", "-p", "api-bones-sdk-gen", "--"])
            .args(["wrapper", "typescript", "--crate"])
            .arg(root.join("crates").join(name))
            .arg("--spec")
            .arg(root.join("SPEC.md"))
            .args([
                "--wasm-sha256",
                "abababababababababababababababababababababababababababababababab",
                "--descriptor",
            ])
            .arg(&descriptor)
            .arg("--out")
            .arg(self.out(name))
            .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
            .output()
            .expect("the generator runs");
        self.outcomes.insert(name.to_owned(), output);
    }
}

#[given(expr = "a service with the crate {string} published as {string}")]
#[given(expr = "the crate {string} published as {string}")]
fn given_crate(world: &mut WrapperWorld, name: String, package: String) {
    world.write_crate(&name, None);
    let out = world.out(&name);
    fs::create_dir_all(&out).expect("an output directory");
    fs::write(
        out.join("package.json"),
        format!("{{\"name\":\"{package}\"}}"),
    )
    .expect("a manifest");
    WrapperWorld::git(&out, &["init", "-q"]);
    WrapperWorld::git(&out, &["add", "package.json"]);
    world.crates.insert(name, package);
}

#[given(expr = "the SPEC lists only the package {string}")]
fn given_spec_lists_only(world: &mut WrapperWorld, package: String) {
    world.spec_packages = Some(vec![package]);
}

#[given(expr = "the package.json of the crate {string} is not tracked by git")]
fn given_untracked(world: &mut WrapperWorld, name: String) {
    let out = world.out(&name);
    WrapperWorld::git(&out, &["rm", "-q", "--cached", "package.json"]);
}

#[given(expr = "the crate {string} has no package.json")]
fn given_no_manifest(world: &mut WrapperWorld, name: String) {
    let out = world.out(&name);
    fs::remove_file(out.join("package.json")).expect("a manifest to remove");
}

#[given(expr = "the SPEC moves the surface {string} to the package {string}")]
fn given_surface_retargeted(world: &mut WrapperWorld, surface: String, package: String) {
    world.retargeted.insert(surface, package);
}

#[given(expr = "the surface {string} names the endpoint {string}")]
fn given_unserved(world: &mut WrapperWorld, surface: String, endpoint: String) {
    world.unserved.insert(surface, endpoint);
}

#[given(expr = "the crate {string} enables the wasm feature of the crate {string}")]
fn given_foreign_wasm(world: &mut WrapperWorld, name: String, other: String) {
    world.write_crate(&other, None);
    world.write_crate(&name, Some(&other));
}

#[given(expr = "the crate {string} exports {string} on the Type {string} of the surface {string}")]
fn given_export(
    world: &mut WrapperWorld,
    name: String,
    method: String,
    ty: String,
    surface: String,
) {
    world
        .exports
        .entry(name)
        .or_default()
        .push((ty, surface, method));
}

#[given(expr = "the surface {string} serves the endpoint {string} through {string}")]
fn given_served(world: &mut WrapperWorld, surface: String, endpoint: String, method: String) {
    world
        .served
        .entry(surface)
        .or_default()
        .push((endpoint, method));
}

/// A complete snapshot document, as a JSON value, for the crate `name`.
fn snapshot(world: &WrapperWorld, name: &str, schema: &str) -> serde_json::Value {
    serde_json::json!({
        "schema": schema,
        "service": "acme",
        "crate": name,
        "package": world.crates[name],
        "methods": [{
            "export": "acme__Device__authorize",
            "type": "Device",
            "rust": "authorize",
            "typescript": "authorize",
            "surface": "s-device",
            "kind": "unary",
            "endpoints": ["acme.v1.Device/Authorize"]
        }]
    })
}

#[given(expr = "the crate {string} tracks an l2-methods.json of the schema {string}")]
fn given_snapshot_schema(world: &mut WrapperWorld, name: String, schema: String) {
    let text = snapshot(world, &name, &schema).to_string();
    fs::write(world.out(&name).join("l2-methods.json"), text).expect("a snapshot");
}

#[given(expr = "the crate {string} tracks an l2-methods.json without the field {string}")]
fn given_snapshot_without(world: &mut WrapperWorld, name: String, field: String) {
    let mut value = snapshot(world, &name, "api-bones.l2-methods/v1");
    value["methods"][0]
        .as_object_mut()
        .expect("an object")
        .remove(&field);
    fs::write(world.out(&name).join("l2-methods.json"), value.to_string()).expect("a snapshot");
}

#[when("the wrapper is generated for every crate")]
fn when_every(world: &mut WrapperWorld) {
    let names: Vec<String> = world.crates.keys().cloned().collect();
    for name in names {
        world.generate(&name);
    }
}

#[when(expr = "the wrapper is generated for the crate {string}")]
fn when_one(world: &mut WrapperWorld, name: String) {
    world.generate(&name);
}

#[then("every generation succeeded")]
fn then_succeeded(world: &mut WrapperWorld) {
    for (name, output) in &world.outcomes {
        assert!(
            output.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[then(expr = "the wrapper of the crate {string} is the package {string}")]
fn then_package(world: &mut WrapperWorld, name: String, package: String) {
    let out = world.out(&name);
    let methods: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(out.join("l2-methods.json")).expect("a snapshot"))
            .expect("json");
    assert_eq!(methods["package"], package.as_str());
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(out.join("package.json")).expect("a manifest"))
            .expect("json");
    assert_eq!(manifest["name"], package.as_str());
}

#[then(
    expr = "the snapshot of the crate {string} has {string} on the Type {string} serving only {string}"
)]
fn then_serving(
    world: &mut WrapperWorld,
    name: String,
    method: String,
    ty: String,
    endpoint: String,
) {
    let out = world.out(&name);
    let snapshot: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(out.join("l2-methods.json")).expect("a snapshot"))
            .expect("json");
    let found = snapshot["methods"]
        .as_array()
        .expect("methods")
        .iter()
        .find(|m| m["type"] == ty.as_str() && m["rust"] == method.as_str())
        .unwrap_or_else(|| panic!("no `{method}` on `{ty}`: {snapshot}"));
    assert_eq!(found["endpoints"], serde_json::json!([endpoint]));
}

#[then(expr = "the generation is refused with {string}")]
fn then_refused(world: &mut WrapperWorld, message: String) {
    let output = world.outcomes.values().next().expect("a generation ran");
    assert!(!output.status.success(), "the generation succeeded");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(&message), "{stderr}");
}

#[tokio::main]
async fn main() {
    let features = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/features/sdk_wrapper_package.feature"
    );
    WrapperWorld::run(features).await;
}
