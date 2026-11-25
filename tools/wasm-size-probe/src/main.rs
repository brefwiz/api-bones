// SPDX-License-Identifier: MIT
//! Wiring only: the work is `api_bones_wasm_size_probe::run`.

#[cfg(not(target_arch = "wasm32"))]
fn main() -> std::process::ExitCode {
    api_bones_wasm_size_probe::exit_code(&std::env::args().skip(1).collect::<Vec<_>>())
}

#[cfg(target_arch = "wasm32")]
fn main() {}
