// SPDX-License-Identifier: MIT
//! `api-bones-wasm-test-server --addr-file <path>`: serves the fetch transport
//! test service and writes its base URL to `<path>`.

use std::path::Path;
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let (Some(flag), Some(path), None) = (args.next(), args.next(), args.next()) else {
        eprintln!("usage: api-bones-wasm-test-server --addr-file <path>");
        return ExitCode::from(2);
    };
    if flag != "--addr-file" {
        eprintln!("usage: api-bones-wasm-test-server --addr-file <path>");
        return ExitCode::from(2);
    }
    match api_bones_wasm_test_server::serve(Path::new(&path)).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("api-bones-wasm-test-server: {error}");
            ExitCode::FAILURE
        }
    }
}
