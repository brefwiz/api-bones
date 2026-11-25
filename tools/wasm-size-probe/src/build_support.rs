// SPDX-License-Identifier: MIT
//! The build script's logic: generates the Rust clients of
//! `tests/fixtures/proto/bones/probe/v1/probe.proto` and the `bones.v1`
//! messages it carries,
//! and writes the descriptor set the batch runtime reads its batch relations
//! from. `build.rs` includes this file by path; the lib includes it for tests.

use std::path::{Path, PathBuf};

/// The probe service proto, relative to the proto roots.
pub const PROBE: &str = "bones/probe/v1/probe.proto";

/// The canonical messages the probe service names. `annotations.proto` is
/// compiled as an import only: its options live in the descriptor set, and the
/// probe has no use for generated option types.
pub const CANONICAL: [&str; 4] = [
    "bones/v1/pagination.proto",
    "bones/v1/batch.proto",
    "bones/v1/errors.proto",
    "bones/v1/stream.proto",
];

/// The cargo directives that re-run the build script.
pub const RERUN: [&str; 3] = [
    "cargo:rerun-if-changed=tests/fixtures/proto",
    "cargo:rerun-if-changed=../../api-bones-protos/proto",
    "cargo:rerun-if-env-changed=BREFWIZ_CODEGEN_INPUTS_SHA",
];

/// Every proto file compiled, probe first.
pub fn proto_files() -> Vec<&'static str> {
    std::iter::once(PROBE).chain(CANONICAL).collect()
}

/// The include roots: the probe protos, then the canonical ones.
pub fn proto_roots(manifest: &Path) -> [PathBuf; 2] {
    [
        manifest.join("tests/fixtures/proto"),
        manifest.join("../../api-bones-protos/proto"),
    ]
}

/// Compiles the protos, writes `probe.fds` and the generated clients into
/// `out_dir`, and returns the descriptor set path.
pub fn generate(manifest: &Path, out_dir: &Path) -> Result<PathBuf, String> {
    let files = proto_files();
    let mut compiler = protox::Compiler::new(proto_roots(manifest))
        .map_err(|e| format!("proto include paths: {e}"))?;
    compiler.include_imports(true);
    compiler
        .open_files(&files)
        .map_err(|e| format!("compile the probe protos: {e}"))?;
    let descriptor_set = out_dir.join("probe.fds");
    std::fs::write(&descriptor_set, compiler.encode_file_descriptor_set())
        .map_err(|e| format!("write descriptor set: {e}"))?;

    connectrpc_build::Config::new()
        .files(&files)
        .descriptor_set(&descriptor_set)
        .out_dir(out_dir)
        .generate_json(true)
        .emit_register_fn(false)
        .include_file("_connectrpc.rs")
        .compile()
        .map_err(|e| format!("generate the probe clients: {e}"))?;
    Ok(descriptor_set)
}

/// The whole build step: generate, then print the rerun directives.
pub fn build(manifest: &str, out_dir: &Path) {
    generate(Path::new(manifest), out_dir).expect("the probe protos generate");
    for directive in RERUN {
        println!("{directive}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_start_with_the_probe_then_the_canonical_messages() {
        let files = proto_files();
        assert_eq!(files[0], PROBE);
        assert_eq!(&files[1..], CANONICAL);
    }

    #[test]
    fn roots_hang_off_the_manifest() {
        let roots = proto_roots(Path::new("/m"));
        assert_eq!(roots[0], Path::new("/m/tests/fixtures/proto"));
        assert_eq!(roots[1], Path::new("/m/../../api-bones-protos/proto"));
    }

    #[test]
    fn generate_writes_the_descriptor_set_and_include_file() {
        let out = std::env::temp_dir().join(format!("wasm-size-build-{}", std::process::id()));
        std::fs::create_dir_all(&out).unwrap();
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let fds = generate(manifest, &out).expect("the real protos generate");
        assert!(std::fs::metadata(&fds).unwrap().len() > 0);
        assert!(out.join("_connectrpc.rs").exists());
        std::fs::remove_dir_all(&out).unwrap();
    }

    #[test]
    fn a_missing_proto_root_is_an_error() {
        let out = std::env::temp_dir().join(format!("wasm-size-bad-{}", std::process::id()));
        std::fs::create_dir_all(&out).unwrap();
        let error = generate(Path::new("/nonexistent/a/b"), &out).unwrap_err();
        assert!(error.contains("compile the probe protos"), "{error}");
        std::fs::remove_dir_all(&out).unwrap();
    }

    #[test]
    fn an_unwritable_out_dir_is_an_error() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let error = generate(manifest, Path::new("/nonexistent/out")).unwrap_err();
        assert!(error.contains("write descriptor set"), "{error}");
    }

    #[test]
    fn build_generates_and_announces_reruns() {
        let out = std::env::temp_dir().join(format!("wasm-size-full-{}", std::process::id()));
        std::fs::create_dir_all(&out).unwrap();
        build(env!("CARGO_MANIFEST_DIR"), &out);
        assert!(out.join("probe.fds").exists());
        assert!(RERUN.iter().all(|d| d.starts_with("cargo:rerun-if-")));
        std::fs::remove_dir_all(&out).unwrap();
    }
}
