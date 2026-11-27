// SPDX-License-Identifier: MIT
//! `features --crate <dir> [--check]` against a Layer 2 crate written to a
//! scratch directory: the block it writes is the golden one, `--check` accepts
//! it and refuses a stale one.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const GOLDEN: &str = include_str!("fixtures/numbers.golden.toml");

const LIB: &str = r#"
#[client(surface = "connect-batch", proto = generated::sdk::v1)]
impl WidgetsClient {
    #[export(sync)]
    pub fn label(&self, widget: generated::sdk::v1::Widget) -> String { body() }
    #[export]
    pub async fn get_widget(&self, id: String) -> Result<generated::sdk::v1::Widget, Error> { body() }
    #[export]
    pub fn batch_get_widgets(&self, ids: Vec<String>) -> Batch<generated::sdk::v1::Widget> { body() }
}

#[client(surface = "connect-pagination", proto = generated::sdk::v1)]
impl NumbersClient {
    #[export]
    pub fn list(&self, limit: u32) -> ListCall<generated::sdk::v1::Number> { body() }
}
"#;

const MANIFEST: &str = "[package]\nname = \"numbers\"\nversion = \"0.0.0\"\n\n[features]\ndefault = []\n# api-bones-sdk-gen features: begin\nsdk-stale = []\n# api-bones-sdk-gen features: end\n";

fn krate(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "api-bones-sdk-gen-golden-{}-{name}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("src")).expect("a directory");
    fs::write(dir.join("src/lib.rs"), LIB).expect("a source");
    fs::write(dir.join("Cargo.toml"), MANIFEST).expect("a manifest");
    dir
}

fn features(dir: &Path, check: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_api-bones-sdk-gen"));
    command.arg("features").arg("--crate").arg(dir);
    if check {
        command.arg("--check");
    }
    command.output().expect("the generator runs")
}

fn block(dir: &Path) -> String {
    let manifest = fs::read_to_string(dir.join("Cargo.toml")).expect("a manifest");
    let from = manifest
        .find("# api-bones-sdk-gen features: begin")
        .expect("begin marker");
    manifest[from..].to_owned()
}

#[test]
fn the_block_written_is_the_golden_one() {
    let dir = krate("write");
    let output = features(&dir, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(block(&dir), GOLDEN);
    fs::remove_dir_all(&dir).expect("cleaned");
}

#[test]
fn check_accepts_the_golden_block() {
    let dir = krate("accept");
    assert!(features(&dir, false).status.success());
    let output = features(&dir, true);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    fs::remove_dir_all(&dir).expect("cleaned");
}

#[test]
fn check_refuses_a_stale_block_and_leaves_it_alone() {
    let dir = krate("stale");
    let output = features(&dir, true);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("is out of date"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(dir.join("Cargo.toml")).expect("a manifest"),
        MANIFEST
    );
    fs::remove_dir_all(&dir).expect("cleaned");
}

#[test]
fn a_source_that_breaks_a_macro_rule_fails_the_command() {
    let dir = krate("broken");
    fs::write(
        dir.join("src/lib.rs"),
        "#[client(surface = \"s\")] impl C { #[export] async fn f(&self) -> Result<Widget, Error> { body() } }",
    )
    .expect("a source");
    let output = features(&dir, false);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("bare name"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    fs::remove_dir_all(&dir).expect("cleaned");
}
