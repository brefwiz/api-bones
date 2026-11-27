// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! The library-feature-set half of `../../features/connect_library_root_store.feature`.
//!
//! This crate depends on `api-bones-page` and `api-bones-batch` alone, so
//! `api_bones::connect::root_store()` runs with the feature set a library
//! consumer gets: bundled webpki roots, with `SSL_CERT_FILE` read by the
//! webpki store and no operating system store. The certificates are real CAs
//! generated for the scenario and every result comes from the real
//! `root_store()` called in this process.
use std::path::PathBuf;

use api_bones::connect::RootStoreError;
use brefwiz_cucumber_steps::TwoPass;
use cucumber::{World, given, then, when};
use rcgen::{BasicConstraints, CertificateParams, DistinguishedName, DnType, IsCa, KeyPair};
use rustls::RootCertStore;

// Linked so cargo unifies api-bones features exactly as a library consumer's.
use api_bones_batch as _;
use api_bones_page as _;

#[derive(Debug, Default, World)]
struct LibraryWorld {
    file: Option<PathBuf>,
    read: Option<Result<RootCertStore, RootStoreError>>,
}

fn ca_pem(name: &str) -> String {
    let mut subject = DistinguishedName::new();
    subject.push(DnType::CommonName, name);
    let mut ca = CertificateParams::new(Vec::new()).expect("ca params");
    ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca.distinguished_name = subject;
    ca.self_signed(&KeyPair::generate().expect("ca key"))
        .expect("ca cert")
        .pem()
}

fn scenario_file(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "api-bones-library-roots-{}-{tag}.pem",
        std::process::id()
    ))
}

fn set_env(file: Option<&PathBuf>) {
    // SAFETY: the runner executes one scenario at a time and no other thread
    // reads the environment while it is edited.
    unsafe {
        match file {
            Some(file) => std::env::set_var("SSL_CERT_FILE", file),
            None => std::env::remove_var("SSL_CERT_FILE"),
        }
        std::env::remove_var("SSL_CERT_DIR");
    }
}

#[given(expr = "a PEM file holding {int} CA certificates")]
fn given_file(world: &mut LibraryWorld, count: usize) {
    let bundle: String = (0..count).map(|n| ca_pem(&format!("root-{n}"))).collect();
    let path = scenario_file(&count.to_string());
    std::fs::write(&path, bundle).expect("write bundle");
    world.file = Some(path);
}

#[given("no SSL_CERT_FILE or SSL_CERT_DIR is set")]
fn given_no_override(_world: &mut LibraryWorld) {
    set_env(None);
}

#[given("SSL_CERT_FILE names a file that does not exist")]
fn given_missing(world: &mut LibraryWorld) {
    let path = scenario_file("missing");
    let _ = std::fs::remove_file(&path);
    world.file = Some(path);
}

#[given("SSL_CERT_FILE names a file holding no certificate")]
fn given_empty(world: &mut LibraryWorld) {
    let path = scenario_file("empty");
    std::fs::write(&path, "no certificate here\n").expect("write file");
    world.file = Some(path);
}

#[when("the root store is read with SSL_CERT_FILE naming that file")]
fn when_read_named(world: &mut LibraryWorld) {
    set_env(world.file.as_ref());
    let read = api_bones::connect::root_store();
    set_env(None);
    world.read = Some(read);
}

#[when("a library-crate client reads the root store")]
fn when_read(world: &mut LibraryWorld) {
    let read = api_bones::connect::root_store();
    world.read = Some(read);
}

#[then(expr = "the store trusts exactly {int} root certificates")]
fn then_trusts(world: &mut LibraryWorld, expected: usize) {
    let read = world.read.as_ref().expect("no store read");
    let store = read.as_ref().expect("root store");
    assert_eq!(store.roots.len(), expected);
    let _ = std::fs::remove_file(world.file.as_ref().expect("no file given"));
}

#[then("the store trusts exactly the bundled webpki root set")]
fn then_bundled(world: &mut LibraryWorld) {
    let read = world.read.as_ref().expect("no store read");
    let store = read.as_ref().expect("root store");
    assert!(store.roots == webpki_roots::TLS_SERVER_ROOTS.to_vec());
}

#[then("reading the root store fails naming that file")]
fn then_fails(world: &mut LibraryWorld) {
    let read = world.read.as_ref().expect("no store read");
    let error = read.as_ref().expect_err("the read must fail");
    assert_eq!(
        Some(&error.path),
        world.file.as_ref(),
        "the failure names the file"
    );
    assert!(!error.reason.is_empty());
    let _ = std::fs::remove_file(world.file.as_ref().expect("no file given"));
}

#[tokio::main]
async fn main() {
    let features = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../features/connect_library_root_store.feature"
    )
    .to_owned();
    // Every scenario edits the environment while it runs.
    let two_pass = TwoPass {
        skip_tags: vec!["wip".into()],
        isolated_tag: "isolated".into(),
        parallel: 1,
    };
    two_pass.run::<LibraryWorld>(features).await;
}
