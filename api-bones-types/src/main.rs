// SPDX-License-Identifier: MIT
//! Writes the combined api-bones JSON Schema to the path given as the only argument.

use std::{env, path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let (Some(out_path), None) = (args.next().map(PathBuf::from), args.next()) else {
        eprintln!("usage: api-bones-schema-gen <output-path>");
        return ExitCode::from(2);
    };
    match api_bones_schema_gen::write_schema(&out_path) {
        Ok(()) => {
            println!("wrote {}", out_path.display());
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("api-bones-schema-gen: {}: {err}", out_path.display());
            ExitCode::FAILURE
        }
    }
}
