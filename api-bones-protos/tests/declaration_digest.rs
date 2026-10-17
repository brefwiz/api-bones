// SPDX-License-Identifier: MIT
//! Recomputes every golden declaration-digest vector under
//! `testdata/declaration-digest/<case>/`: `repo/` holds the files of the
//! product's repository that the declaring schema covers, `expected.txt` the
//! digest.

use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

/// The declaration digest of `files`, each an `(import path, source bytes)`
/// pair of the declaring schema whose source is in the product's repository.
///
/// Files are ordered by import path (bytewise); each contributes its import
/// path, a NUL byte, its source bytes and a NUL byte. Files the build resolves
/// from outside the repository (toolchain-bundled well-known types,
/// registry-resolved dependencies such as `bones/v1/*.proto`) are not passed.
/// The result is `sha256:` followed by the lowercase hex SHA-256.
fn declaration_digest(files: &[(String, Vec<u8>)]) -> String {
    let mut ordered: Vec<&(String, Vec<u8>)> = files.iter().collect();
    ordered.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    let mut hasher = Sha256::new();
    for (path, source) in ordered {
        hasher.update(path.as_bytes());
        hasher.update([0u8]);
        hasher.update(source);
        hasher.update([0u8]);
    }
    format!("sha256:{}", hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(char::from(DIGITS[usize::from(b >> 4)]));
        out.push(char::from(DIGITS[usize::from(b & 0xf)]));
    }
    out
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
    for entry in fs::read_dir(dir).expect("read vector dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            collect(root, &path, out);
        } else {
            let import = path
                .strip_prefix(root)
                .expect("under root")
                .to_str()
                .expect("utf-8 path")
                .replace('\\', "/");
            out.push((import, fs::read(&path).expect("read source")));
        }
    }
}

fn cases_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/declaration-digest")
}

fn vector_digest(case: &str) -> (String, String) {
    let dir = cases_dir().join(case);
    let repo = dir.join("repo");
    let mut files = Vec::new();
    collect(&repo, &repo, &mut files);
    let expected = fs::read_to_string(dir.join("expected.txt")).expect("expected.txt");
    (declaration_digest(&files), expected.trim().to_string())
}

#[test]
fn every_vector_recomputes() {
    let mut seen = 0;
    for entry in fs::read_dir(cases_dir()).expect("vectors dir") {
        let path = entry.expect("entry").path();
        if !path.is_dir() {
            continue;
        }
        let case = path.file_name().unwrap().to_str().unwrap().to_string();
        let (got, expected) = vector_digest(&case);
        assert_eq!(got, expected, "case {case}");
        seen += 1;
    }
    assert_eq!(seen, 4, "every golden case is exercised");
}

/// The expected values are literals computed outside this code with
/// `sha256sum` over the hand-assembled byte string, so the vectors do not
/// merely echo the implementation.
#[test]
fn vectors_match_independent_literals() {
    assert_eq!(
        vector_digest("single-file").1,
        "sha256:cedf4f5ac1ca682f25cc9a1f7d13c267c8c0b96f3a6a54dc22cac2c47e147515"
    );
    assert_eq!(
        vector_digest("several-files").1,
        "sha256:bb97f8f6813febdac70daa8234c688e3c0049321e608b0210b468a72cf4732dc"
    );
    assert_eq!(
        vector_digest("in-repo-import").1,
        "sha256:f6c5518120eb45e5f33b5ecfdfff5f26819c3f8aba35eeea5ad0165812db2d73"
    );
    assert_eq!(
        vector_digest("excluded-imports").1,
        "sha256:66fd8bc15549532a8400aff9bae1e15fc180852a7c8ddff5669747f3f481550b"
    );
}

#[test]
fn single_file_is_path_nul_source_nul() {
    let bytes = b"orders/v1/events.proto\0syntax = \"proto3\";\npackage orders.v1;\nmessage OrderPlaced { string id = 1; }\n\0";
    assert_eq!(
        format!("sha256:{}", hex(&Sha256::digest(bytes))),
        "sha256:cedf4f5ac1ca682f25cc9a1f7d13c267c8c0b96f3a6a54dc22cac2c47e147515"
    );
}

#[test]
fn input_order_does_not_change_the_digest() {
    let a = ("a/a.proto".to_string(), b"A".to_vec());
    let b = ("b/b.proto".to_string(), b"B".to_vec());
    assert_eq!(
        declaration_digest(&[a.clone(), b.clone()]),
        declaration_digest(&[b, a])
    );
}

#[test]
fn source_change_changes_the_digest() {
    let one = [("a.proto".to_string(), b"A".to_vec())];
    let two = [("a.proto".to_string(), b"B".to_vec())];
    assert_ne!(declaration_digest(&one), declaration_digest(&two));
}
