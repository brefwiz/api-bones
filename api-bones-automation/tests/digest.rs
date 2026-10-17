// SPDX-License-Identifier: MIT
//! Recomputes every golden vector of `api-bones-protos/testdata/declaration-digest`
//! with the crate's own `declaration_digest`.

use std::fs;
use std::path::{Path, PathBuf};

use api_bones_automation::declaration_digest;

fn cases() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../api-bones-protos/testdata/declaration-digest")
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect(root, &path, out);
        } else {
            let import = path
                .strip_prefix(root)
                .unwrap()
                .to_str()
                .unwrap()
                .replace('\\', "/");
            out.push((import, fs::read(&path).unwrap()));
        }
    }
}

fn vector(case: &str) -> (Vec<(String, Vec<u8>)>, String) {
    let dir = cases().join(case);
    let repo = dir.join("repo");
    let mut files = Vec::new();
    collect(&repo, &repo, &mut files);
    let expected = fs::read_to_string(dir.join("expected.txt")).unwrap();
    (files, expected.trim().to_owned())
}

#[test]
fn every_golden_vector_recomputes() {
    let mut seen = 0;
    for entry in fs::read_dir(cases()).unwrap() {
        let path = entry.unwrap().path();
        if !path.is_dir() {
            continue;
        }
        let case = path.file_name().unwrap().to_str().unwrap().to_owned();
        let (files, expected) = vector(&case);
        assert_eq!(declaration_digest(&files), expected, "{case}");
        seen += 1;
    }
    assert!(seen >= 4, "the vectors are missing: {seen}");
}

#[test]
fn the_digest_does_not_depend_on_the_order_files_are_listed() {
    let (mut files, expected) = vector("several-files");
    files.reverse();
    assert_eq!(declaration_digest(&files), expected);
}

#[test]
fn a_changed_byte_or_path_changes_the_digest() {
    let (files, expected) = vector("several-files");
    let mut edited = files.clone();
    edited[0].1.push(b'\n');
    assert_ne!(declaration_digest(&edited), expected);
    let mut renamed = files;
    renamed[0].0.push('x');
    assert_ne!(declaration_digest(&renamed), expected);
}

#[test]
fn the_empty_set_is_the_sha256_of_nothing() {
    assert_eq!(
        declaration_digest(&[]),
        "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}
