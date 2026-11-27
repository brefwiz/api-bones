// SPDX-License-Identifier: MIT
//! Which npm package a wrapper is, and the refusals that keep one crate to one
//! package: the package is the `name` of the `package.json` tracked beside the
//! wrapper, every export belongs to a surface that lists it, every endpoint of
//! those surfaces is served, and no feature links another crate's wasm exports.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, anyhow, ensure};
use api_bones_sdk_model::{Model, SpecL2Methods, l2_methods};

use crate::wrapper_ts::{Package, runtime_dependency};

/// What the SPEC front matter, the crate's manifest and the tracked
/// `<out>/package.json` say about the package.
///
/// The package is the `name` of the `package.json` tracked in `out`: one
/// crate, one wrapper, one package. The manifest is resolved by `cargo
/// metadata`, so workspace-inherited fields carry their value. `out` is the
/// directory the package is written to; the runtime dependency link is
/// relative to it.
///
/// # Errors
///
/// A `package.json` that is missing, untracked or nameless; a SPEC without
/// front matter or without a surface that lists the package for TypeScript;
/// an export of a surface that does not list the package; a surface that lists
/// it and names an endpoint no export serves; a manifest whose features enable
/// the `wasm` feature of a dependency other than the SDK runtime; a manifest
/// `cargo metadata` rejects or that declares no license; an `out` path that
/// cannot be resolved.
pub(crate) fn package_of(spec: &Path, krate: &Path, out: &Path) -> anyhow::Result<Package> {
    let name = tracked_package_name(out)?;
    let text =
        std::fs::read_to_string(spec).with_context(|| format!("cannot read {}", spec.display()))?;
    let front = text
        .strip_prefix("---\n")
        .and_then(|rest| rest.split("\n---").next())
        .ok_or_else(|| anyhow!("{} has no front matter", spec.display()))?;
    let document: serde_yaml::Value = serde_yaml::from_str(front)?;
    let service = document["service"]
        .as_str()
        .ok_or_else(|| anyhow!("the SPEC names no service"))?
        .to_owned();
    let surfaces = document["sdk_surfaces"]
        .as_mapping()
        .ok_or_else(|| anyhow!("the SPEC lists no sdk_surfaces"))?;
    let mut spec_l2 = SpecL2Methods::default();
    let mut listed = BTreeMap::new();
    for (id, entry) in surfaces {
        let id = id.as_str().unwrap_or_default().to_owned();
        spec_l2.surfaces.insert(id.clone());
        let lists_package = entry["targets"]["typescript"]["packages"]
            .as_sequence()
            .into_iter()
            .flatten()
            .any(|package| package.as_str() == Some(name.as_str()));
        if !lists_package {
            continue;
        }
        let mut endpoints = Vec::new();
        if let Some(mapping) = entry["l2_methods"].as_mapping() {
            for (endpoint, languages) in mapping {
                let endpoint = endpoint.as_str().unwrap_or_default().to_owned();
                let rust = spec_l2.endpoints.entry(endpoint.clone()).or_default();
                for method in languages["rust"]
                    .as_sequence()
                    .into_iter()
                    .flatten()
                    .filter_map(|m| m.as_str())
                {
                    if !rust.iter().any(|known| known == method) {
                        rust.push(method.to_owned());
                    }
                }
                endpoints.push(endpoint);
            }
        }
        listed.insert(id, endpoints);
    }
    if listed.is_empty() {
        return Err(anyhow!(
            "no sdk_surfaces entry lists `{name}` as a typescript target package"
        ));
    }
    let manifest_path = krate.join("Cargo.toml");
    let output = Command::new("cargo")
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--manifest-path",
        ])
        .arg(&manifest_path)
        .output()
        .context("cannot start `cargo metadata`")?;
    if !output.status.success() {
        return Err(anyhow!(
            "`cargo metadata` failed for {}: {}",
            manifest_path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let manifest_path = manifest_path
        .canonicalize()
        .with_context(|| format!("cannot read the manifest of {}", krate.display()))?;
    let found = crate_metadata(&String::from_utf8_lossy(&output.stdout), &manifest_path)?;
    if let Some((feature, entry)) = &found.foreign_wasm {
        return Err(anyhow!(
            "feature `{feature}` of `{}` enables `{entry}`: a wrapper links no other crate's wasm exports, so compose through the Rust API and the crate's own #[export]",
            found.name
        ));
    }
    let (runtime, registry) = runtime_dependency(spec.parent().unwrap_or(Path::new("")), out)?;
    if !found.private && registry.is_none() {
        return Err(anyhow!(
            "a published wrapper publishes to its runtime's registry; the in-tree runtime package.json names no publishConfig.registry"
        ));
    }
    let package = Package {
        service,
        crate_name: found.name,
        name,
        license: found.license,
        private: found.private,
        runtime,
        registry,
        spec: spec_l2,
        listed,
        wasm_sha256: String::new(),
    };
    verify(&crate::features::read_model(krate)?, &package)?;
    Ok(package)
}

/// The `name` of the `package.json` that `out` tracks in git.
fn tracked_package_name(out: &Path) -> anyhow::Result<String> {
    let path = out.join("package.json");
    let text = std::fs::read_to_string(&path).with_context(|| {
        format!(
            "cannot read {}: the wrapper package is the name in the tracked package.json",
            path.display()
        )
    })?;
    let tracked = Command::new("git")
        .arg("-C")
        .arg(out)
        .args(["ls-files", "--error-unmatch", "--", "package.json"])
        .output()
        .context("cannot start `git`")?
        .status
        .success();
    if !tracked {
        return Err(anyhow!(
            "{} is not tracked by git: the wrapper package is the name in the tracked package.json",
            path.display()
        ));
    }
    let manifest: serde_json::Value =
        serde_json::from_str(&text).with_context(|| format!("{} is not JSON", path.display()))?;
    manifest["name"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("{} names no package", path.display()))
}

/// Every export names a surface that lists the package, and every endpoint of
/// every surface that lists it is served by an export of the crate.
fn verify(model: &Model, package: &Package) -> anyhow::Result<()> {
    for client in &model.clients {
        for export in &client.exports {
            ensure!(
                package.listed.contains_key(&export.surface),
                "package `{}` is not a typescript target of surface `{}`, which the exports of `{}` name",
                package.name,
                export.surface,
                package.crate_name
            );
        }
    }
    let document = l2_methods(
        model,
        &package.spec,
        &package.service,
        &package.crate_name,
        &package.name,
    )?;
    for (surface, endpoints) in &package.listed {
        for endpoint in endpoints {
            ensure!(
                document
                    .methods
                    .iter()
                    .any(|method| method.endpoints.contains(endpoint)),
                "surface `{surface}` lists package `{}` and names endpoint `{endpoint}`, which no export of `{}` serves",
                package.name,
                package.crate_name
            );
        }
    }
    Ok(())
}

/// What the Layer 2 crate's manifest resolves to, workspace inheritance
/// included.
#[derive(Debug, PartialEq, Eq)]
struct CrateMetadata {
    name: String,
    license: String,
    /// The crate says `publish = false`.
    private: bool,
    /// The first feature, and its entry, that enables the `wasm` feature of a
    /// dependency other than the SDK runtime.
    foreign_wasm: Option<(String, String)>,
}

/// The package of `manifest` in the `cargo metadata --no-deps` document
/// `json`.
fn crate_metadata(json: &str, manifest: &Path) -> anyhow::Result<CrateMetadata> {
    let document: serde_json::Value =
        serde_json::from_str(json).context("`cargo metadata` did not answer JSON")?;
    let package = document["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|package| {
            package["manifest_path"].as_str().is_some_and(|listed| {
                // Cargo lists the path as spelled to it; the caller's is
                // resolved, so a symlinked directory still matches.
                let listed = Path::new(listed);
                listed == manifest || listed.canonicalize().is_ok_and(|real| real == manifest)
            })
        })
        .ok_or_else(|| {
            anyhow!(
                "`cargo metadata` lists no package for {}",
                manifest.display()
            )
        })?;
    let text = |key: &str| package[key].as_str().map(str::to_owned);
    Ok(CrateMetadata {
        name: text("name").ok_or_else(|| anyhow!("the manifest names no crate"))?,
        license: text("license")
            .ok_or_else(|| anyhow!("the manifest of {} declares no license", manifest.display()))?,
        private: package["publish"]
            .as_array()
            .is_some_and(std::vec::Vec::is_empty),
        foreign_wasm: foreign_wasm(&package["features"]),
    })
}

/// The one dependency whose `wasm` feature a wrapper's own `wasm` feature
/// enables.
const RUNTIME_CRATE: &str = "api-bones-sdk";

/// The first feature of `features` (a `cargo metadata` feature map) with an
/// entry `<dependency>/wasm` or `<dependency>?/wasm` for another dependency
/// than [`RUNTIME_CRATE`].
fn foreign_wasm(features: &serde_json::Value) -> Option<(String, String)> {
    features.as_object()?.iter().find_map(|(feature, entries)| {
        entries
            .as_array()?
            .iter()
            .filter_map(serde_json::Value::as_str)
            .find(|entry| {
                entry
                    .strip_suffix("/wasm")
                    .is_some_and(|dependency| dependency.trim_end_matches('?') != RUNTIME_CRATE)
            })
            .map(|entry| (feature.clone(), entry.to_owned()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wrapper_ts::RUNTIME_FLOOR;
    use crate::wrapper_ts::testkit::{
        REGISTRY_RUNTIME_JSON, RUNTIME_JSON, TEST_REGISTRY, runtime_at, scratch,
    };

    const SPEC: &str = "---\nservice: svc\nsdk_surfaces:\n  s:\n    targets:\n      typescript:\n        packages: [\"@x/y\"]\n---\n";

    /// A git work tree at `out` that tracks a `package.json` named `name`.
    fn track_manifest(out: &Path, name: &str) {
        std::fs::create_dir_all(out).expect("out");
        std::fs::write(out.join("package.json"), format!("{{\"name\":\"{name}\"}}"))
            .expect("manifest");
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

    /// A workspace whose one member `c` holds `member` as its manifest.
    fn package_with(tag: &str, member: &str) -> anyhow::Result<Package> {
        package_in(tag, member, None)
    }

    fn package_in(tag: &str, member: &str, runtime: Option<&str>) -> anyhow::Result<Package> {
        let dir = scratch(tag);
        if let Some(json) = runtime {
            runtime_at(&dir, json);
        }
        std::fs::write(dir.join("SPEC.md"), SPEC).expect("spec");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[workspace]\nmembers = [\"c\"]\nresolver = \"3\"\n[workspace.package]\nversion = \"1.2.3\"\nlicense = \"Apache-2.0\"\n",
        )
        .expect("workspace");
        std::fs::create_dir_all(dir.join("c/src")).expect("member");
        std::fs::write(dir.join("c/src/lib.rs"), "").expect("source");
        std::fs::write(dir.join("c/Cargo.toml"), member).expect("manifest");
        track_manifest(&dir.join("out"), "@x/y");
        let result = package_of(&dir.join("SPEC.md"), &dir.join("c"), &dir.join("out"));
        std::fs::remove_dir_all(&dir).expect("cleaned");
        result
    }

    const INHERITING: &str = "[package]\nname = \"c\"\nversion.workspace = true\nedition = \"2024\"\nlicense.workspace = true\n";

    #[test]
    fn the_workspace_inherited_fields_resolve_to_their_values() {
        let package =
            package_in("inherit", INHERITING, Some(REGISTRY_RUNTIME_JSON)).expect("package");
        assert_eq!(package.crate_name, "c");
        assert_eq!(package.license, "Apache-2.0");
        assert!(!package.private);
        assert_eq!(package.registry.as_deref(), Some(TEST_REGISTRY));
    }

    #[test]
    fn a_published_wrapper_without_a_runtime_registry_is_refused() {
        for runtime in [None, Some(RUNTIME_JSON)] {
            let error = package_in("no-registry", INHERITING, runtime)
                .err()
                .expect("refused");
            assert!(
                error.to_string().contains("publishConfig.registry"),
                "{error}"
            );
        }
    }

    #[test]
    fn a_literal_manifest_resolves_and_publish_false_makes_the_package_private() {
        let package = package_with(
            "literal",
            "[package]\nname = \"c\"\nversion = \"4.5.6\"\nedition = \"2024\"\nlicense = \"MIT\"\npublish = false\n",
        )
        .expect("package");
        assert_eq!(package.license, "MIT");
        assert!(package.private);
        assert_eq!(package.runtime, format!(">={RUNTIME_FLOOR}"));
    }

    #[test]
    fn a_manifest_cargo_rejects_is_refused_with_its_message() {
        let error = package_with("broken", "[package\n").err().expect("refused");
        assert!(
            error.to_string().contains("`cargo metadata` failed"),
            "{error}"
        );
    }

    fn metadata(packages: &str) -> String {
        format!("{{\"packages\":[{packages}],\"workspace_members\":[]}}")
    }

    fn entry(manifest: &str, extra: &str) -> String {
        format!(
            "{{\"name\":\"n\",\"version\":\"1.0.0\",\"license\":\"MIT\",\"manifest_path\":\"{manifest}\"{extra}}}"
        )
    }

    #[test]
    fn the_package_of_the_manifest_is_picked_among_the_workspace_members() {
        let json = metadata(&format!(
            "{{\"name\":\"other\",\"version\":\"9.9.9\",\"license\":\"GPL-3.0\",\"manifest_path\":\"/w/o/Cargo.toml\",\"publish\":null}},{}",
            entry("/w/c/Cargo.toml", ",\"publish\":null")
        ));
        let found = crate_metadata(&json, Path::new("/w/c/Cargo.toml")).expect("found");
        assert_eq!(
            found,
            CrateMetadata {
                name: "n".to_owned(),
                license: "MIT".to_owned(),
                private: false,
                foreign_wasm: None,
            }
        );
    }

    #[test]
    fn only_an_empty_publish_list_makes_the_package_private() {
        for (publish, private) in [
            (",\"publish\":[]", true),
            (",\"publish\":null", false),
            (",\"publish\":[\"crates-io\"]", false),
            ("", false),
        ] {
            let json = metadata(&entry("/w/Cargo.toml", publish));
            let found = crate_metadata(&json, Path::new("/w/Cargo.toml")).expect("found");
            assert_eq!(found.private, private, "{publish}");
        }
    }

    #[test]
    fn only_the_wasm_feature_of_another_dependency_than_the_runtime_is_foreign() {
        let features = |json: &str| foreign_wasm(&serde_json::from_str(json).expect("json"));
        assert_eq!(features(r#"{"wasm":["api-bones-sdk/wasm"]}"#), None);
        assert_eq!(
            features(r#"{"wasm":["api-bones-sdk?/wasm"],"x":["a/std"]}"#),
            None
        );
        assert_eq!(features("null"), None);
        assert_eq!(
            features(r#"{"wasm":["api-bones-sdk/wasm","other-l2/wasm"]}"#),
            Some(("wasm".to_owned(), "other-l2/wasm".to_owned()))
        );
        assert_eq!(
            features(r#"{"all":["other-l2?/wasm"]}"#),
            Some(("all".to_owned(), "other-l2?/wasm".to_owned()))
        );
    }

    #[test]
    fn a_package_json_that_is_missing_or_untracked_names_no_package() {
        let dir = scratch("untracked");
        let error = tracked_package_name(&dir.join("absent")).expect_err("missing");
        assert!(
            error
                .to_string()
                .contains("the wrapper package is the name in the tracked package.json"),
            "{error}"
        );
        std::fs::write(dir.join("package.json"), "{\"name\":\"@x/y\"}").expect("manifest");
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(&dir)
                .args(["init", "-q"])
                .status()
                .expect("git runs")
                .success()
        );
        let error = tracked_package_name(&dir).expect_err("untracked");
        assert!(
            error.to_string().contains("is not tracked by git"),
            "{error}"
        );
        std::fs::remove_dir_all(&dir).expect("cleaned");
    }

    #[test]
    fn a_tracked_package_json_gives_its_name_or_is_refused_when_it_has_none() {
        let dir = scratch("tracked");
        track_manifest(&dir, "@x/y");
        assert_eq!(tracked_package_name(&dir).expect("name"), "@x/y");
        std::fs::write(dir.join("package.json"), "{}").expect("manifest");
        let error = tracked_package_name(&dir).expect_err("nameless");
        assert!(error.to_string().contains("names no package"), "{error}");
        std::fs::write(dir.join("package.json"), "[").expect("manifest");
        let error = tracked_package_name(&dir).expect_err("not json");
        assert!(error.to_string().contains("is not JSON"), "{error}");
        std::fs::remove_dir_all(&dir).expect("cleaned");
    }

    #[test]
    fn a_spec_that_lists_the_package_nowhere_is_refused() {
        let dir = scratch("unlisted");
        std::fs::write(dir.join("SPEC.md"), SPEC).expect("spec");
        track_manifest(&dir.join("out"), "@x/other");
        let error = package_of(&dir.join("SPEC.md"), &dir, &dir.join("out"))
            .err()
            .expect("unlisted");
        assert!(
            error
                .to_string()
                .contains("no sdk_surfaces entry lists `@x/other`"),
            "{error}"
        );
        std::fs::remove_dir_all(&dir).expect("cleaned");
    }

    fn model_of(source: &str) -> Model {
        Model::from_files(&[syn::parse_file(source).expect("source")]).expect("model")
    }

    fn listing(surfaces: &[(&str, &[&str])]) -> Package {
        let mut package = Package {
            service: "svc".to_owned(),
            crate_name: "c".to_owned(),
            name: "@x/y".to_owned(),
            license: "MIT".to_owned(),
            private: true,
            runtime: format!(">={RUNTIME_FLOOR}"),
            registry: None,
            spec: SpecL2Methods::default(),
            listed: BTreeMap::new(),
            wasm_sha256: String::new(),
        };
        for (surface, endpoints) in surfaces {
            package.spec.surfaces.insert((*surface).to_owned());
            package.listed.insert(
                (*surface).to_owned(),
                endpoints.iter().map(|e| (*e).to_owned()).collect(),
            );
            for endpoint in *endpoints {
                package
                    .spec
                    .endpoints
                    .insert((*endpoint).to_owned(), vec!["ping".to_owned()]);
            }
        }
        package
    }

    const PINGING: &str = "#[client(surface = \"s\", proto = g::v1)]\nimpl C {\n    #[export]\n    pub async fn ping(&self) -> Result<g::v1::Pong, Error> { body() }\n}\n";

    #[test]
    fn a_surface_that_does_not_list_the_package_for_an_export_is_refused() {
        let package = listing(&[("other", &[])]);
        let error = verify(&model_of(PINGING), &package).expect_err("unlisted surface");
        assert_eq!(
            error.to_string(),
            "package `@x/y` is not a typescript target of surface `s`, which the exports of `c` name"
        );
    }

    #[test]
    fn a_listing_surface_with_an_endpoint_no_export_serves_is_refused() {
        let mut package = listing(&[("s", &["p.S/Ping"]), ("t", &["p.S/Other"])]);
        package
            .spec
            .endpoints
            .insert("p.S/Other".to_owned(), vec!["other".to_owned()]);
        let error = verify(&model_of(PINGING), &package).expect_err("uncovered");
        assert_eq!(
            error.to_string(),
            "surface `t` lists package `@x/y` and names endpoint `p.S/Other`, which no export of `c` serves"
        );
    }

    #[test]
    fn exports_that_serve_every_listed_endpoint_pass() {
        let package = listing(&[("s", &["p.S/Ping"])]);
        verify(&model_of(PINGING), &package).expect("covered");
    }

    const TWO_SURFACES: &str = "#[client(surface = \"alpha\", proto = g::v1)]\nimpl A {\n    #[export]\n    pub async fn ping(&self) -> Result<g::v1::Pong, Error> { body() }\n}\n#[client(surface = \"beta\", proto = g::v1)]\nimpl B {\n    #[export]\n    pub async fn pong(&self) -> Result<g::v1::Pong, Error> { body() }\n}\n";

    const OVERRIDDEN: &str = "#[client(surface = \"alpha\", proto = g::v1)]\nimpl A {\n    #[export]\n    pub async fn ping(&self) -> Result<g::v1::Pong, Error> { body() }\n    #[export(surface = \"beta\")]\n    pub async fn pong(&self) -> Result<g::v1::Pong, Error> { body() }\n}\n";

    fn two_surface_package(listed: &[(&str, &[&str])]) -> Package {
        let mut package = listing(listed);
        package.spec.surfaces.insert("alpha".to_owned());
        package.spec.surfaces.insert("beta".to_owned());
        package
            .spec
            .endpoints
            .insert("p.S/E1".to_owned(), vec!["ping".to_owned()]);
        package
            .spec
            .endpoints
            .insert("p.S/E2".to_owned(), vec!["pong".to_owned()]);
        package
    }

    const BETA_REFUSAL: &str = "package `@x/y` is not a typescript target of surface `beta`, which the exports of `c` name";

    #[test]
    fn a_crate_exporting_on_two_surfaces_that_both_list_the_package_passes() {
        let package = two_surface_package(&[("alpha", &["p.S/E1"]), ("beta", &["p.S/E2"])]);
        verify(&model_of(TWO_SURFACES), &package).expect("both surfaces list the package");
    }

    #[test]
    fn a_second_exporting_surface_that_does_not_list_the_package_is_refused() {
        let package = two_surface_package(&[("alpha", &["p.S/E1"])]);
        let error = verify(&model_of(TWO_SURFACES), &package).expect_err("beta unlisted");
        assert_eq!(error.to_string(), BETA_REFUSAL);
    }

    #[test]
    fn an_export_naming_another_surface_than_its_client_is_refused_when_unlisted() {
        let package = two_surface_package(&[("alpha", &["p.S/E1"])]);
        let error = verify(&model_of(OVERRIDDEN), &package).expect_err("beta unlisted");
        assert_eq!(error.to_string(), BETA_REFUSAL);
    }

    #[test]
    fn a_document_that_lists_no_package_for_the_manifest_is_refused() {
        let json = metadata(&entry("/w/o/Cargo.toml", ""));
        let error = crate_metadata(&json, Path::new("/w/c/Cargo.toml")).expect_err("absent");
        assert!(error.to_string().contains("lists no package"), "{error}");
        let error = crate_metadata("{}", Path::new("/w/c/Cargo.toml")).expect_err("no packages");
        assert!(error.to_string().contains("lists no package"), "{error}");
    }

    #[test]
    fn a_document_that_is_not_json_is_refused() {
        let error = crate_metadata("not json", Path::new("/w/Cargo.toml")).expect_err("refused");
        assert!(error.to_string().contains("did not answer JSON"), "{error}");
    }

    #[test]
    fn a_package_without_a_name_or_a_license_is_refused() {
        let at = "\"manifest_path\":\"/w/Cargo.toml\"";
        for (fields, message) in [
            (
                "\"version\":\"1.0.0\",\"license\":\"MIT\"",
                "names no crate",
            ),
            (
                "\"name\":\"n\",\"version\":\"1.0.0\",\"license\":null",
                "declares no license",
            ),
        ] {
            let json = metadata(&format!("{{{fields},{at}}}"));
            let error = crate_metadata(&json, Path::new("/w/Cargo.toml")).expect_err(message);
            assert!(error.to_string().contains(message), "{error}");
        }
    }
}
