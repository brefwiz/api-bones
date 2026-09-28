// SPDX-License-Identifier: MIT
use std::{env, fs, path::PathBuf};

fn compile(files: &[PathBuf], include: &PathBuf) -> Vec<u8> {
    let mut compiler = protox::Compiler::new([include]).expect("create proto compiler");
    compiler.include_imports(true).include_source_info(true);
    compiler.open_files(files).expect("compile proto fixtures");
    compiler.encode_file_descriptor_set()
}

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let root = manifest.join("../..");
    let include = root.join("api-bones-protos/proto");
    let fixture = include.join("fixtures/native.proto");
    let descriptors = compile(std::slice::from_ref(&fixture), &include);
    let (_, rust) = api_bones_sdk_gen::native::generate_plugin_output(
        &descriptors,
        &["fixtures/native.proto".to_owned()],
        "rust",
    )
    .expect("generate native Rust fixture through plugin core")
    .expect("native fixture emits plugin output");
    fs::write(
        PathBuf::from(env::var("OUT_DIR").expect("out dir")).join("bones_native.rs"),
        rust,
    )
    .expect("write generated native Rust fixture");
    fs::write(
        PathBuf::from(env::var("OUT_DIR").expect("out dir")).join("native.pb"),
        &descriptors,
    )
    .expect("write native descriptor fixture");
    let ambiguous = include.join("fixtures/native-ambiguous.proto");
    let ambiguous_descriptors = compile(std::slice::from_ref(&ambiguous), &include);
    fs::write(
        PathBuf::from(env::var("OUT_DIR").expect("out dir")).join("native-ambiguous.pb"),
        ambiguous_descriptors,
    )
    .expect("write ambiguous native descriptor fixture");
    for name in [
        "native-leading-digit",
        "native-keyword",
        "native-constructor",
        "native-transport",
        "native-collision",
        "native-rust2024-keyword",
    ] {
        let source = include.join(format!("fixtures/{name}.proto"));
        let descriptors = compile(std::slice::from_ref(&source), &include);
        fs::write(
            PathBuf::from(env::var("OUT_DIR").expect("out dir")).join(format!("{name}.pb")),
            descriptors,
        )
        .expect("write invalid identifier descriptor");
        println!("cargo:rerun-if-changed={}", source.display());
    }
    let same_a = include.join("fixtures/native-same-name-a.proto");
    let same_b = include.join("fixtures/native-same-name-b.proto");
    let same = compile(&[same_a, same_b], &include);
    fs::write(
        PathBuf::from(env::var("OUT_DIR").expect("out dir")).join("native-same-name.pb"),
        same,
    )
    .expect("write same-name descriptors");
    let collision_a = include.join("fixtures/native-symbol-collision-a.proto");
    let collision_b = include.join("fixtures/native-symbol-collision-b.proto");
    let collision = compile(&[collision_a, collision_b], &include);
    fs::write(
        PathBuf::from(env::var("OUT_DIR").expect("out dir")).join("native-symbol-collision.pb"),
        collision,
    )
    .expect("write generated-symbol collision descriptors");
    let rpc_only = include.join("fixtures/rpc-only.proto");
    fs::write(
        PathBuf::from(env::var("OUT_DIR").expect("out dir")).join("rpc-only.pb"),
        compile(std::slice::from_ref(&rpc_only), &include),
    )
    .expect("write ordinary RPC descriptor");
    let second = include.join("fixtures/other/native-second.proto");
    fs::write(
        PathBuf::from(env::var("OUT_DIR").expect("out dir")).join("native-multi.pb"),
        compile(&[fixture.clone(), second], &include),
    )
    .expect("write multi-directory native descriptor");
    println!("cargo:rerun-if-changed={}", fixture.display());
    println!("cargo:rerun-if-changed={}", ambiguous.display());
    println!(
        "cargo:rerun-if-changed={}",
        include.join("bones/v1/annotations.proto").display()
    );
}
