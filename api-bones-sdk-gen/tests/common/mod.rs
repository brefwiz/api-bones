// SPDX-License-Identifier: MIT
//! Helpers the generator's integration tests share: the repository root and
//! the descriptor set of the test service's contract.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The repository root.
pub fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// The descriptor set of the test service's contract protos, with the shipped
/// `bones` protos they import, written once to a file.
pub fn descriptor() -> &'static Path {
    static FILE: OnceLock<PathBuf> = OnceLock::new();
    FILE.get_or_init(|| {
        let root = std::env::temp_dir().join(format!(
            "api-bones-sdk-gen-wrapper-{}-protos",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        for (relative, bytes) in api_bones_protos::files() {
            let path = root.join(relative);
            fs::create_dir_all(path.parent().expect("a directory")).expect("created");
            fs::write(path, bytes).expect("a proto");
        }
        let contract = repo().join("tests/l2/proto");
        let mut files = Vec::new();
        protos_under(&contract, &contract, &mut files);
        let mut compiler = protox::Compiler::new([&contract, &root]).expect("include paths");
        compiler.include_imports(true);
        compiler
            .open_files(&files)
            .expect("the contract protos compile");
        let file = root.join("l2.fds");
        fs::write(&file, compiler.encode_file_descriptor_set()).expect("a descriptor set");
        file
    })
}

fn protos_under(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .expect("a proto directory")
        .map(|entry| entry.expect("an entry").path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            protos_under(root, &path, out);
        } else if path.extension().is_some_and(|ext| ext == "proto") {
            let relative = path.strip_prefix(root).expect("under the root");
            out.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
}
