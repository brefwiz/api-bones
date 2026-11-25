// SPDX-License-Identifier: MIT
//! Every type of the allowed set compiles through the macros, and every type
//! outside it is refused with a message that names the position, the reason
//! and the allowed set.

use std::process::Command;

/// The index of the `brefwiz` registry the workspace manifests name on their
/// path dependencies; the same one as `.cargo/config.toml`.
const REGISTRY_INDEX: &str = "sparse+https://git.brefwiz.com/api/packages/brefwiz/cargo/";

/// Set in the child process that runs the body.
const SCOPED: &str = "API_BONES_SDK_MACROS_COMPILE_SCOPED";

/// The rustflags of every trybuild case.
///
/// trybuild removes `RUSTFLAGS` from its cargo calls and passes its own flags
/// through `--config`, which cargo ranks below `CARGO_ENCODED_RUSTFLAGS`, so
/// that variable is the one place a flag reaches the case's `rustc`, and it
/// replaces every other rustflags source (a `-D warnings` of the surrounding
/// build included). The first five arguments are the flags trybuild passes itself,
/// kept so the expected diagnostics render as they do without this variable.
///
/// The `feature` values the macros gate their exports on (`sdk-all`,
/// `sdk-<surface>`, `sdk-m-<method>`) are declared by the crate that carries
/// the annotated impl. A trybuild case is a bare project that declares none,
/// so the cfg checker is told that any `feature` value is expected there.
const CASE_RUSTFLAGS: [&str; 6] = [
    "--cfg",
    "trybuild",
    "--verbose",
    "--diagnostic-width=140",
    "-Adead_code",
    "--check-cfg=cfg(feature,values(any()))",
];

/// Runs `body` in a child process of this test binary that knows the
/// `brefwiz` registry.
///
/// trybuild builds its cases in a project under the cargo target directory,
/// outside the workspace, so cargo finds no `.cargo/config.toml` there and
/// cannot resolve the registry the copied manifests name. The registry index
/// is passed as an environment variable instead, which a workspace crate
/// cannot set on its own process (`unsafe_code` is denied).
fn in_registry_scope(test: &str, body: impl FnOnce()) {
    if std::env::var_os(SCOPED).is_some() {
        body();
        return;
    }
    let status = Command::new(std::env::current_exe().expect("this test binary"))
        .args(["--exact", test, "--nocapture", "--test-threads", "1"])
        .env(SCOPED, "1")
        .env("CARGO_REGISTRIES_BREFWIZ_INDEX", REGISTRY_INDEX)
        .env_remove("RUSTFLAGS")
        .env("CARGO_ENCODED_RUSTFLAGS", CASE_RUSTFLAGS.join("\x1f"))
        .status()
        .expect("the test binary runs");
    assert!(status.success(), "{test} failed: {status}");
}

#[test]
fn the_allowed_set_compiles() {
    in_registry_scope("the_allowed_set_compiles", || {
        trybuild::TestCases::new().pass("tests/compile_pass/*.rs");
    });
}

#[test]
fn what_is_outside_the_allowed_set_is_refused() {
    in_registry_scope("what_is_outside_the_allowed_set_is_refused", || {
        trybuild::TestCases::new().compile_fail("tests/compile_fail/*.rs");
    });
}
