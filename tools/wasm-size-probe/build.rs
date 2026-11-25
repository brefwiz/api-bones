// SPDX-License-Identifier: MIT
//! Generates the probe clients; the logic is `src/build_support.rs`.
#[path = "src/build_support.rs"]
mod build_support;

fn main() {
    let out = std::env::var_os("OUT_DIR").expect("OUT_DIR is set by cargo");
    build_support::build(env!("CARGO_MANIFEST_DIR"), out.as_ref());
}
