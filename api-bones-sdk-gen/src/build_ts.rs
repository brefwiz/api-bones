// SPDX-License-Identifier: MIT
//! `wrapper build-ts`: the wasm build of a Layer 2 crate, its wasm-bindgen
//! glue, the generated TypeScript sources and the compiled package.
//!
//! The wasm is built for `wasm32-unknown-unknown` with the crate's `wasm`
//! and `sdk-all` features, so every client the package declares is exported,
//! and bound with `wasm-bindgen --target web --weak-refs`; no
//! `wasm-pack` is involved.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, ensure};

use crate::wrapper_ts::{self, SOURCE_DIR};

/// One external command of the build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// What the step is, named in its failure.
    pub what: &'static str,
    pub program: &'static str,
    pub args: Vec<OsString>,
    /// Where the command runs; the current directory when `None`.
    pub dir: Option<PathBuf>,
}

/// Where the build's external commands run: the toolchain of the machine, or
/// a record of them.
pub trait Tools {
    /// Runs `step`, failing with its name when it does not succeed.
    ///
    /// # Errors
    ///
    /// A command that cannot start or does not succeed.
    fn run(&self, step: &Step) -> anyhow::Result<()>;
}

/// The toolchain installed on this machine.
pub struct System;

impl Tools for System {
    fn run(&self, step: &Step) -> anyhow::Result<()> {
        let mut command = Command::new(step.program);
        command.args(&step.args);
        if let Some(dir) = &step.dir {
            command.current_dir(dir);
        }
        run(&mut command, step.what)
    }
}

/// Runs `command`, failing with its name when it does not succeed.
fn run(command: &mut Command, what: &str) -> anyhow::Result<()> {
    let status = command
        .status()
        .with_context(|| format!("cannot start {what}"))?;
    ensure!(status.success(), "{what} exited with {status}");
    Ok(())
}

/// Builds the package of `krate` into `out` with the toolchain of this
/// machine.
///
/// # Errors
///
/// A step that cannot start or does not succeed, or a SPEC the sources do not
/// satisfy.
pub fn run_build(
    spec: &Path,
    krate: &Path,
    surface: &str,
    out: &Path,
    target_dir: &Path,
) -> anyhow::Result<()> {
    build(&System, spec, krate, surface, out, target_dir)
}

/// Builds the package of `krate` into `out`, running every external command
/// through `tools`. The SPEC is read before any command runs, and a failing
/// step stops the build.
///
/// # Errors
///
/// As [`run_build`].
pub fn build(
    tools: &dyn Tools,
    spec: &Path,
    krate: &Path,
    surface: &str,
    out: &Path,
    target_dir: &Path,
) -> anyhow::Result<()> {
    let package = wrapper_ts::package_of(spec, krate, surface)?;
    let glue = package.crate_name.replace('-', "_");
    tools.run(&Step {
        what: "cargo build for wasm32",
        program: "cargo",
        args: [
            "build",
            "--release",
            "--target",
            "wasm32-unknown-unknown",
            "--features",
            "wasm,sdk-all",
            "--manifest-path",
        ]
        .into_iter()
        .map(OsString::from)
        .chain([
            krate.join("Cargo.toml").into_os_string(),
            "--target-dir".into(),
            target_dir.as_os_str().to_owned(),
        ])
        .collect(),
        dir: None,
    })?;
    let wasm = target_dir
        .join("wasm32-unknown-unknown/release")
        .join(format!("{glue}.wasm"));
    tools.run(&Step {
        what: "wasm-bindgen",
        program: "wasm-bindgen",
        args: vec![
            OsString::from("--target"),
            OsString::from("web"),
            OsString::from("--weak-refs"),
            OsString::from("--out-name"),
            OsString::from(&glue),
            OsString::from("--out-dir"),
            out.join(SOURCE_DIR).into_os_string(),
            wasm.into_os_string(),
        ],
        dir: None,
    })?;
    let model = crate::features::read_model(krate)?;
    wrapper_ts::write(out, &wrapper_ts::generate(&model, &package)?)?;
    ensure!(
        out.join("package-lock.json").is_file(),
        "{} has no package-lock.json; run `npm install --package-lock-only` there and commit it",
        out.display()
    );
    tools.run(&Step {
        what: "npm ci",
        program: "npm",
        args: ["ci", "--no-audit", "--no-fund"]
            .into_iter()
            .map(OsString::from)
            .collect(),
        dir: Some(out.to_owned()),
    })?;
    tools.run(&Step {
        what: "npm run build",
        program: "npm",
        args: vec![OsString::from("run"), OsString::from("build")],
        dir: Some(out.to_owned()),
    })
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    fn repo() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "api-bones-sdk-gen-build-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// Records every step; fails the one named `fail_on`.
    struct Recorder {
        steps: RefCell<Vec<Step>>,
        fail_on: Option<&'static str>,
    }

    impl Recorder {
        fn new(fail_on: Option<&'static str>) -> Self {
            Self {
                steps: RefCell::default(),
                fail_on,
            }
        }

        fn whats(&self) -> Vec<&'static str> {
            self.steps.borrow().iter().map(|step| step.what).collect()
        }
    }

    impl Tools for Recorder {
        fn run(&self, step: &Step) -> anyhow::Result<()> {
            self.steps.borrow_mut().push(step.clone());
            ensure!(self.fail_on != Some(step.what), "{} failed", step.what);
            Ok(())
        }
    }

    fn with_lockfile(out: &Path) {
        std::fs::create_dir_all(out).expect("a directory");
        std::fs::write(out.join("package-lock.json"), "").expect("a lockfile");
    }

    fn build_with(tools: &Recorder, surface: &str, out: &Path) -> anyhow::Result<()> {
        build(
            tools,
            &repo().join("SPEC.md"),
            &repo().join("tests/l2"),
            surface,
            out,
            Path::new("/target"),
        )
    }

    #[test]
    fn a_succeeding_command_passes_and_a_failing_one_names_itself() {
        run(&mut Command::new("true"), "the true tool").expect("exit 0");
        let failed = run(&mut Command::new("false"), "the false tool").expect_err("exit 1");
        assert!(failed.to_string().starts_with("the false tool exited with"));
    }

    #[test]
    fn a_command_that_cannot_start_names_itself() {
        let missing = run(
            &mut Command::new("api-bones-no-such-tool"),
            "the missing tool",
        )
        .expect_err("not installed");
        assert_eq!(missing.to_string(), "cannot start the missing tool");
    }

    #[test]
    fn the_system_runs_a_step_in_its_directory() {
        let dir = scratch("system");
        std::fs::create_dir_all(&dir).expect("a directory");
        let step = |program, dir| Step {
            what: "a tool",
            program,
            args: vec!["-d".into(), ".".into()],
            dir,
        };
        System
            .run(&step("test", Some(dir.clone())))
            .expect("the directory is the working one");
        System.run(&step("test", None)).expect("here");
        assert!(System.run(&step("false", None)).is_err());
        std::fs::remove_dir_all(&dir).expect("cleaned");
    }

    #[test]
    fn the_build_runs_cargo_then_wasm_bindgen_then_writes_then_npm() {
        let out = scratch("steps");
        with_lockfile(&out);
        let tools = Recorder::new(None);
        build_with(&tools, "sdk-call", &out).expect("the build");
        assert_eq!(
            tools.whats(),
            [
                "cargo build for wasm32",
                "wasm-bindgen",
                "npm ci",
                "npm run build"
            ]
        );
        let steps = tools.steps.borrow();
        let args = |step: &Step| -> Vec<String> {
            step.args
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect()
        };
        let cargo = args(&steps[0]);
        assert!(cargo.contains(&"wasm32-unknown-unknown".to_owned()));
        assert!(cargo.contains(&"/target".to_owned()));
        assert!(cargo.contains(&"wasm,sdk-all".to_owned()));
        assert!(cargo.iter().any(|arg| arg.ends_with("tests/l2/Cargo.toml")));
        let bindgen = args(&steps[1]);
        assert!(bindgen.contains(&"--weak-refs".to_owned()));
        assert_eq!(
            bindgen.last().map(String::as_str),
            Some("/target/wasm32-unknown-unknown/release/api_bones_test_service_l2.wasm")
        );
        assert_eq!(steps[2].dir.as_deref(), Some(out.as_path()));
        assert_eq!(
            args(&steps[2]),
            ["ci", "--no-audit", "--no-fund"].map(str::to_owned)
        );
        assert_eq!(steps[3].dir.as_deref(), Some(out.as_path()));
        assert!(out.join(SOURCE_DIR).join("client.ts").is_file());
        assert!(out.join("package.json").is_file());
        std::fs::remove_dir_all(&out).expect("cleaned");
    }

    #[test]
    fn a_failing_step_stops_the_build() {
        for (failing, ran) in [
            ("cargo build for wasm32", 1),
            ("wasm-bindgen", 2),
            ("npm ci", 3),
            ("npm run build", 4),
        ] {
            let out = scratch("failing");
            with_lockfile(&out);
            let tools = Recorder::new(Some(failing));
            let error = build_with(&tools, "sdk-call", &out).expect_err(failing);
            assert_eq!(error.to_string(), format!("{failing} failed"));
            assert_eq!(tools.steps.borrow().len(), ran, "{failing}");
            assert_eq!(
                out.join("package.json").exists(),
                failing.starts_with("npm"),
                "{failing}: the sources are written between bindgen and npm"
            );
            let _ = std::fs::remove_dir_all(&out);
        }
    }

    #[test]
    fn a_build_without_a_lockfile_stops_before_npm() {
        let out = scratch("no-lockfile");
        let tools = Recorder::new(None);
        let error = build_with(&tools, "sdk-call", &out).expect_err("no lockfile");
        assert!(error.to_string().contains("package-lock.json"), "{error}");
        assert_eq!(tools.steps.borrow().len(), 2);
        assert!(out.join("package.json").is_file());
        std::fs::remove_dir_all(&out).expect("cleaned");
    }

    #[test]
    fn a_surface_the_spec_does_not_list_runs_no_tool() {
        let tools = Recorder::new(None);
        build_with(&tools, "no-such-surface", &scratch("absent")).expect_err("no such surface");
        assert_eq!(tools.steps.borrow().len(), 0);
    }

    #[test]
    fn the_system_build_fails_before_any_tool_for_an_absent_surface() {
        let out = scratch("system-absent");
        run_build(
            &repo().join("SPEC.md"),
            &repo().join("tests/l2"),
            "no-such-surface",
            &out,
            Path::new("/target"),
        )
        .expect_err("no such surface");
        assert!(!out.exists());
    }
}
