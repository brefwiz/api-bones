// SPDX-License-Identifier: MIT
//! `features --crate <dir> [--check]`: the Cargo features that gate the wasm
//! exports of a Layer 2 crate.
//!
//! The crate's sources are read with `api-bones-sdk-model`, the parse the
//! `#[client]` macro uses, and the block between the two marker lines of the
//! crate's `Cargo.toml` is rewritten to hold one feature per `sdk_surfaces` id
//! and per exported method, plus `sdk-all` and `wasm`. With `--check` nothing
//! is written and a block that differs fails.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail, ensure};
use api_bones_sdk_model::{Model, gate_features};

/// The line that opens the generated block.
pub const BEGIN: &str = "# api-bones-sdk-gen features: begin";
/// The line that closes the generated block.
pub const END: &str = "# api-bones-sdk-gen features: end";

/// The feature that turns on the SDK's wasm support.
const WASM: &str = "wasm = [\"api-bones-sdk/wasm\"]";

/// The Rust sources under `dir`, in path order.
fn sources(dir: &Path, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    let mut entries = fs::read_dir(dir)
        .with_context(|| format!("cannot read {}", dir.display()))?
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(fs::DirEntry::path);
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            sources(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
    Ok(())
}

/// The annotated items of every source under `<krate>/src`.
pub fn read_model(krate: &Path) -> anyhow::Result<Model> {
    let mut paths = Vec::new();
    sources(&krate.join("src"), &mut paths)?;
    let mut files = Vec::new();
    for path in &paths {
        let text =
            fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
        let file =
            syn::parse_file(&text).with_context(|| format!("cannot parse {}", path.display()))?;
        files.push(file);
    }
    Model::from_files(&files)
        .with_context(|| format!("{} breaks a rule of the SDK macros", krate.display()))
}

/// The marker lines and the features between them.
pub fn render(model: &Model) -> String {
    let mut names = BTreeSet::new();
    names.insert("sdk-all".to_owned());
    for client in &model.clients {
        names.insert(gate_features(&client.surface, "")[1].clone());
        for export in &client.exports {
            let [_, surface, method] = gate_features(&export.surface, &export.method);
            names.insert(surface);
            names.insert(method);
        }
    }
    let mut block = format!("{BEGIN}\n");
    for name in &names {
        block.push_str(&format!("{name} = []\n"));
    }
    block.push_str(WASM);
    block.push('\n');
    block.push_str(END);
    block.push('\n');
    block
}

/// `manifest` with the lines from [`BEGIN`] to [`END`] replaced by `block`.
pub fn splice(manifest: &str, block: &str) -> anyhow::Result<String> {
    let lines: Vec<&str> = manifest.lines().collect();
    let begin = lines.iter().position(|line| line.trim() == BEGIN);
    let end = lines.iter().position(|line| line.trim() == END);
    let (Some(begin), Some(end)) = (begin, end) else {
        bail!("the manifest needs the lines `{BEGIN}` and `{END}` inside its [features] table");
    };
    ensure!(begin < end, "`{BEGIN}` must come before `{END}`");
    let mut out = String::new();
    for line in &lines[..begin] {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(block);
    for line in &lines[end + 1..] {
        out.push_str(line);
        out.push('\n');
    }
    Ok(out)
}

/// Rewrites the block of `<krate>/Cargo.toml`; with `check`, fails instead of
/// rewriting when the block is out of date.
pub fn run(krate: &Path, check: bool) -> anyhow::Result<()> {
    let model = read_model(krate)?;
    let manifest_path = krate.join("Cargo.toml");
    let manifest = fs::read_to_string(&manifest_path)
        .with_context(|| format!("cannot read {}", manifest_path.display()))?;
    let updated = splice(&manifest, &render(&model))
        .with_context(|| format!("cannot update {}", manifest_path.display()))?;
    if updated == manifest {
        return Ok(());
    }
    if check {
        bail!(
            "{} is out of date; run `api-bones-sdk-gen features --crate {}`",
            manifest_path.display(),
            krate.display()
        );
    }
    fs::write(&manifest_path, updated)
        .with_context(|| format!("cannot write {}", manifest_path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIB: &str = r#"
        #[client(surface = "connect-pagination")]
        impl NumbersClient {
            #[export]
            pub async fn list(&self) -> ListCall<Number> { body() }
        }

        #[client(surface = "connect-batch")]
        impl WidgetsClient {
            #[export]
            pub async fn get_widget(&self, id: String) -> Result<Widget, Error> { body() }
            #[export(surface = "connect-write")]
            pub async fn clear(&self) -> Result<(), Error> { body() }
        }

        #[client(surface = "idle")]
        impl IdleClient {}

        #[record]
        pub struct Number { pub value: i64 }
        #[record]
        pub struct Widget { pub id: String }
    "#;

    const MANIFEST: &str = "[package]\nname = \"x\"\n\n[features]\ndefault = []\n# api-bones-sdk-gen features: begin\nsdk-old = []\n# api-bones-sdk-gen features: end\nother = []\n";

    fn model() -> Model {
        Model::from_source(LIB).expect("a model")
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "api-bones-sdk-gen-features-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src/nested")).expect("a directory");
        dir
    }

    #[test]
    fn the_block_has_one_feature_per_surface_and_method_and_the_wasm_feature() {
        assert_eq!(
            render(&model()),
            "# api-bones-sdk-gen features: begin\n\
sdk-all = []\n\
sdk-connect-batch = []\n\
sdk-connect-pagination = []\n\
sdk-connect-write = []\n\
sdk-idle = []\n\
sdk-m-clear = []\n\
sdk-m-get_widget = []\n\
sdk-m-list = []\n\
wasm = [\"api-bones-sdk/wasm\"]\n\
# api-bones-sdk-gen features: end\n"
        );
    }

    #[test]
    fn a_crate_without_clients_has_the_two_fixed_features() {
        assert_eq!(
            render(&Model::default()),
            "# api-bones-sdk-gen features: begin\nsdk-all = []\nwasm = [\"api-bones-sdk/wasm\"]\n# api-bones-sdk-gen features: end\n"
        );
    }

    #[test]
    fn splicing_replaces_only_the_lines_between_the_markers() {
        let out = splice(MANIFEST, "# api-bones-sdk-gen features: begin\nsdk-all = []\n# api-bones-sdk-gen features: end\n")
            .expect("spliced");
        assert_eq!(
            out,
            "[package]\nname = \"x\"\n\n[features]\ndefault = []\n# api-bones-sdk-gen features: begin\nsdk-all = []\n# api-bones-sdk-gen features: end\nother = []\n"
        );
    }

    #[test]
    fn splicing_needs_both_markers_in_order() {
        let error = splice("[features]\n", "x\n").expect_err("no markers");
        assert!(error.to_string().contains(BEGIN), "{error}");
        let only_begin = format!("{BEGIN}\n");
        assert!(splice(&only_begin, "x\n").is_err());
        let reversed = format!("{END}\n{BEGIN}\n");
        let error = splice(&reversed, "x\n").expect_err("reversed");
        assert!(error.to_string().contains("must come before"), "{error}");
    }

    #[test]
    fn the_model_is_read_from_every_source_under_src() {
        let dir = scratch("read");
        fs::write(
            dir.join("src/lib.rs"),
            "#[record] pub struct Number { pub value: i64 }",
        )
        .expect("a file");
        fs::write(
            dir.join("src/nested/client.rs"),
            "#[client(surface = \"s\")] impl C { #[export(sync)] pub fn f(&self) {} }",
        )
        .expect("a file");
        fs::write(dir.join("src/notes.txt"), "not rust").expect("a file");
        let model = read_model(&dir).expect("a model");
        assert_eq!(model.records.len(), 1);
        assert_eq!(model.clients[0].exports[0].method, "f");
        fs::remove_dir_all(&dir).expect("cleaned");
    }

    #[test]
    fn a_missing_src_an_unparsable_source_and_a_broken_rule_are_reported() {
        let dir = scratch("broken");
        fs::remove_dir_all(dir.join("src")).expect("removed");
        let error = read_model(&dir).expect_err("no src");
        assert!(error.to_string().contains("cannot read"), "{error}");
        fs::create_dir_all(dir.join("src")).expect("a directory");
        fs::write(dir.join("src/lib.rs"), "fn (").expect("a file");
        let error = read_model(&dir).expect_err("unparsable");
        assert!(error.to_string().contains("cannot parse"), "{error}");
        fs::write(dir.join("src/lib.rs"), "#[record] struct R { id: u8 }").expect("a file");
        let error = read_model(&dir).expect_err("a rule");
        assert!(format!("{error:#}").contains("8 and 16 bit"), "{error:#}");
        fs::remove_dir_all(&dir).expect("cleaned");
    }

    #[test]
    fn running_rewrites_the_manifest_and_check_accepts_the_result() {
        let dir = scratch("run");
        fs::write(dir.join("src/lib.rs"), LIB).expect("a file");
        fs::write(dir.join("Cargo.toml"), MANIFEST).expect("a file");
        run(&dir, true).expect_err("the block is stale");
        assert_eq!(
            fs::read_to_string(dir.join("Cargo.toml")).expect("read"),
            MANIFEST
        );
        run(&dir, false).expect("rewritten");
        let written = fs::read_to_string(dir.join("Cargo.toml")).expect("read");
        assert!(written.contains("sdk-m-get_widget = []\n"), "{written}");
        assert!(!written.contains("sdk-old"), "{written}");
        assert!(written.ends_with("other = []\n"), "{written}");
        run(&dir, true).expect("up to date");
        run(&dir, false).expect("nothing to rewrite");
        fs::remove_dir_all(&dir).expect("cleaned");
    }

    #[test]
    fn running_needs_a_readable_manifest_with_markers() {
        let dir = scratch("manifest");
        fs::write(dir.join("src/lib.rs"), LIB).expect("a file");
        let error = run(&dir, false).expect_err("no manifest");
        assert!(error.to_string().contains("cannot read"), "{error}");
        fs::write(dir.join("Cargo.toml"), "[features]\n").expect("a file");
        let error = run(&dir, false).expect_err("no markers");
        assert!(format!("{error:#}").contains(BEGIN), "{error:#}");
        fs::remove_dir_all(&dir).expect("cleaned");
    }
}
