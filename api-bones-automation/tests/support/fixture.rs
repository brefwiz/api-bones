// SPDX-License-Identifier: MIT
//! Compile a fixture contract to the encoded descriptor set the checks read.

use crate::resolver::{PATH, Resolver};
use protox::Compiler;

pub fn descriptor_set(source: &str) -> Vec<u8> {
    let mut compiler = Compiler::with_file_resolver(Resolver(source.to_owned()));
    compiler.include_imports(true);
    compiler
        .open_file(PATH)
        .unwrap_or_else(|e| panic!("fixture does not compile: {e}\n{source}"));
    compiler.encode_file_descriptor_set()
}
