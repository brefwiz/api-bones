// SPDX-License-Identifier: MIT
//! The wasm size probe.
//!
//! On `wasm32` this crate is the module under test: one exported entry
//! point per capability, each present only when its cargo feature is on, over
//! the clients generated from `tests/fixtures/proto/bones/probe/v1/probe.proto`. The linker
//! keeps what an export reaches, so a variant's size is the floor (one unary
//! RPC) plus what that capability pulls in. Natively the crate is the report
//! model (`report`) the `wasm-size-report` binary fills in from the built
//! variants.
//!
//! Page and batch both run on the `Batched` client of `api-bones-batch`, so
//! each of those two variants carries that runtime; their deltas overlap by it.

#[cfg(not(target_arch = "wasm32"))]
pub mod report;

#[cfg(test)]
mod build_support;

/// Runs the report over `args` and writes the markdown summary to `out`.
///
/// # Errors
/// A bad argument, an unreadable variant set, an invalid report, or a failed
/// write.
#[cfg(not(target_arch = "wasm32"))]
pub fn run(args: &[String], out: &mut impl std::io::Write) -> Result<(), report::ReportError> {
    let markdown = report::run(args)?;
    writeln!(out, "{markdown}")?;
    Ok(())
}

/// Runs the report to stdout and maps the outcome to a process exit code,
/// printing a failure to stderr.
#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn exit_code(args: &[String]) -> std::process::ExitCode {
    match run(args, &mut std::io::stdout()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("wasm-size-report: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(target_arch = "wasm32")]
#[expect(
    clippy::too_many_lines,
    reason = "connectrpc and buffa generate this module; a fix here dies at the next codegen run"
)]
mod generated {
    connectrpc::include_generated!();
}

#[cfg(target_arch = "wasm32")]
mod capability;
#[cfg(target_arch = "wasm32")]
mod exports;
