// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! The Rust half of `../../../features/connect_root_store.feature`.
//!
//! The certificates are real CAs generated for the scenario, and the count
//! comes from the SDK's own `root_store()` called in this process while
//! `SSL_CERT_FILE` names the file.

use cucumber::{given, then, when};
use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, KeyPair};

use crate::world::RootStoreWorld;

fn ca_pem(name: &str) -> String {
    let key = KeyPair::generate().expect("ca key");
    let mut params = CertificateParams::new(Vec::new()).expect("ca params");
    params.distinguished_name.push(DnType::CommonName, name);
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.self_signed(&key).expect("ca cert").pem()
}

#[given(expr = "a PEM file holding {int} CA certificates")]
fn given_file(world: &mut RootStoreWorld, count: usize) {
    let bundle: String = (0..count).map(|n| ca_pem(&format!("root-{n}"))).collect();
    let path = std::env::temp_dir().join(format!(
        "api-bones-root-store-{}-{count}.pem",
        std::process::id()
    ));
    std::fs::write(&path, bundle).expect("write bundle");
    world.file = Some(path);
}

#[when("the root store is read with SSL_CERT_FILE naming that file")]
fn when_read(world: &mut RootStoreWorld) {
    let file = world.file.clone().expect("no file given");
    // SAFETY: the runner executes one scenario at a time and no other thread
    // reads the environment while the variable is set.
    unsafe {
        std::env::set_var("SSL_CERT_FILE", &file);
        std::env::remove_var("SSL_CERT_DIR");
    }
    world.trusted = Some(
        api_bones::connect::root_store()
            .expect("root store")
            .roots
            .len(),
    );
    // SAFETY: as above.
    unsafe {
        std::env::remove_var("SSL_CERT_FILE");
    }
}

#[then(expr = "the store trusts exactly {int} root certificates")]
fn then_trusts(world: &mut RootStoreWorld, expected: usize) {
    assert_eq!(world.trusted.expect("no store read"), expected);
    let _ = std::fs::remove_file(world.file.as_ref().expect("no file given"));
}
