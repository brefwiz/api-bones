// SPDX-License-Identifier: MIT
//! Root certificates for builds that do not read the platform trust store.
//!
//! With neither `SSL_CERT_FILE` nor `SSL_CERT_DIR` set the bundled Mozilla
//! roots are trusted. When either is set, only the certificates they name are
//! trusted, matching the rule `rustls-native-certs` applies.
use std::ffi::OsString;
use std::fmt::Display;
use std::path::Path;

use connectrpc::rustls::RootCertStore;
use rustls_pki_types::CertificateDer;
use rustls_pki_types::pem::PemObject;

use super::transport::RootStoreError;

/// Reads one environment variable; the seam that keeps the lookup testable.
pub(super) type EnvLookup<'a> = &'a dyn Fn(&str) -> Option<OsString>;

pub(super) fn root_store(env: EnvLookup<'_>) -> Result<RootCertStore, RootStoreError> {
    let file = env("SSL_CERT_FILE").filter(|value| !value.is_empty());
    let dirs = env("SSL_CERT_DIR").filter(|value| !value.is_empty());
    if file.is_none() && dirs.is_none() {
        return Ok(RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        });
    }
    let mut store = RootCertStore::empty();
    if let Some(file) = file {
        let file = Path::new(&file);
        if add_pem_file(&mut store, file)? == 0 {
            return Err(load_error(file, &"no certificates found"));
        }
    }
    if let Some(dirs) = dirs {
        for dir in std::env::split_paths(&dirs) {
            add_pem_dir(&mut store, &dir)?;
        }
    }
    Ok(store)
}

/// Add every regular file in `dir`; files holding no PEM certificate add
/// nothing, and entries that cannot be listed are skipped.
fn add_pem_dir(store: &mut RootCertStore, dir: &Path) -> Result<(), RootStoreError> {
    let entries = std::fs::read_dir(dir).map_err(|err| load_error(dir, &err))?;
    for path in entries.flatten().map(|entry| entry.path()) {
        if path.is_file() {
            add_pem_file(store, &path)?;
        }
    }
    Ok(())
}

/// Add every certificate in the PEM file at `path`; returns how many.
fn add_pem_file(store: &mut RootCertStore, path: &Path) -> Result<usize, RootStoreError> {
    let certs = CertificateDer::pem_file_iter(path).map_err(|err| load_error(path, &err))?;
    let mut added = 0;
    for cert in certs {
        let cert = cert.map_err(|err| load_error(path, &err))?;
        store.add(cert).map_err(|err| load_error(path, &err))?;
        added += 1;
    }
    Ok(added)
}

fn load_error(path: &Path, reason: &impl Display) -> RootStoreError {
    RootStoreError {
        path: path.to_path_buf(),
        reason: reason.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CERTS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/connect/testdata/certs");
    const EXTRA: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/connect/testdata/extra");
    const INVALID: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/connect/testdata/invalid");
    const ONE: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/connect/testdata/certs/one.pem"
    );

    fn lookup(file: Option<&str>, dirs: Option<OsString>) -> impl Fn(&str) -> Option<OsString> {
        let file = file.map(OsString::from);
        move |key| match key {
            "SSL_CERT_FILE" => file.clone(),
            "SSL_CERT_DIR" => dirs.clone(),
            _ => None,
        }
    }

    fn dir(path: &str) -> Option<OsString> {
        Some(OsString::from(path))
    }

    fn load(file: Option<&str>, dirs: Option<OsString>) -> Result<RootCertStore, RootStoreError> {
        root_store(&lookup(file, dirs))
    }

    #[test]
    fn neither_variable_trusts_the_bundled_roots() {
        let store = load(None, None).unwrap();
        assert_eq!(store.roots.len(), webpki_roots::TLS_SERVER_ROOTS.len());
        assert_ne!(store.roots.len(), 0);
    }

    #[test]
    fn empty_variables_count_as_unset() {
        let store = load(Some(""), Some(OsString::new())).unwrap();
        assert_eq!(store.roots.len(), webpki_roots::TLS_SERVER_ROOTS.len());
    }

    #[test]
    fn cert_file_trusts_only_its_certificates() {
        assert_eq!(load(Some(ONE), None).unwrap().roots.len(), 1);
    }

    #[test]
    fn cert_file_without_certificates_is_rejected() {
        let notes = format!("{CERTS}/notes.txt");
        let err = load(Some(&notes), None).unwrap_err();
        assert!(err.to_string().contains("no certificates found"));
    }

    #[test]
    fn missing_cert_file_is_rejected() {
        let err = load(Some("/nonexistent/ca.pem"), None).unwrap_err();
        assert!(err.to_string().contains("/nonexistent/ca.pem"));
    }

    #[test]
    fn malformed_pem_names_its_file() {
        let path = format!("{INVALID}/bad.pem");
        let err = load(Some(&path), None).unwrap_err();
        assert_eq!(err.path, Path::new(&path));
    }

    #[test]
    fn certificate_with_invalid_der_is_rejected() {
        let path = format!("{INVALID}/not_der.pem");
        assert_eq!(load(Some(&path), None).unwrap_err().path, Path::new(&path));
    }

    #[test]
    fn cert_dir_trusts_every_pem_file_and_skips_other_files() {
        assert_eq!(load(None, dir(CERTS)).unwrap().roots.len(), 2);
    }

    #[test]
    fn cert_dir_list_trusts_each_directory() {
        let dirs = std::env::join_paths([CERTS, EXTRA]).unwrap();
        assert_eq!(load(None, Some(dirs)).unwrap().roots.len(), 3);
    }

    #[test]
    fn cert_dir_with_malformed_pem_is_rejected() {
        assert!(load(None, dir(INVALID)).is_err());
    }

    #[test]
    fn missing_cert_dir_is_rejected() {
        let err = load(None, dir("/nonexistent/certs")).unwrap_err();
        assert_eq!(err.path, Path::new("/nonexistent/certs"));
    }

    #[test]
    fn a_set_variable_excludes_the_bundled_roots() {
        let store = load(Some(ONE), dir(EXTRA)).unwrap();
        assert_eq!(store.roots.len(), 2);
    }
}
