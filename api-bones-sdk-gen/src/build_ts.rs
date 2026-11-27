// SPDX-License-Identifier: MIT
//! `wrapper build-ts`: the wasm build of a Layer 2 crate, its wasm-bindgen
//! glue, the generated TypeScript sources and the compiled package.
//!
//! The wasm is built for `wasm32-unknown-unknown` with the crate's `wasm`
//! and `sdk-all` features, so every client the package declares is exported,
//! under the generator's own size profile ([`WASM_PROFILE`]), bound with
//! `wasm-bindgen --target web --weak-refs` and shrunk with `wasm-opt -Oz`; no
//! `wasm-pack` is involved. The profile travels as `--config` arguments, so
//! the consumer's own profiles are never read or changed, and the module that
//! ships is the module the size probe measures.
//!
//! The SHA-256 of the module as `wasm-opt` leaves it is stamped into the
//! generated `init.ts`; the loader reads no digest of its own except when the
//! run collects evidence.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, ensure};
use sha2::{Digest, Sha256};

use crate::wrapper_ts::{self, Contract, SOURCE_DIR};

/// The cargo profile the wasm is built under.
pub const WASM_PROFILE_NAME: &str = "api-bones-wasm";

/// The settings of [`WASM_PROFILE_NAME`], as the `--config` values cargo takes:
/// release, optimised for size, one codegen unit, fat LTO, abort on panic,
/// stripped. `tools/wasm-size-probe/build-report.sh` repeats them; a test pins
/// the two together.
pub const WASM_PROFILE: [&str; 6] = [
    "profile.api-bones-wasm.inherits=\"release\"",
    "profile.api-bones-wasm.opt-level=\"z\"",
    "profile.api-bones-wasm.lto=\"fat\"",
    "profile.api-bones-wasm.codegen-units=1",
    "profile.api-bones-wasm.panic=\"abort\"",
    "profile.api-bones-wasm.strip=true",
];

/// The features rustc's `wasm32-unknown-unknown` output uses; `wasm-opt`
/// rejects the module at validation without them.
pub const WASM_OPT_FEATURES: [&str; 7] = [
    "--enable-bulk-memory",
    "--enable-bulk-memory-opt",
    "--enable-sign-ext",
    "--enable-nontrapping-float-to-int",
    "--enable-mutable-globals",
    "--enable-reference-types",
    "--enable-multivalue",
];

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

    /// The bytes of the file at `path`, which a step wrote.
    ///
    /// # Errors
    ///
    /// A file that cannot be read.
    fn read(&self, path: &Path) -> anyhow::Result<Vec<u8>>;
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

    fn read(&self, path: &Path) -> anyhow::Result<Vec<u8>> {
        std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))
    }
}

/// The lowercase hexadecimal SHA-256 of `bytes`.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        })
}

/// Runs `command`, failing with its name when it does not succeed.
fn run(command: &mut Command, what: &str) -> anyhow::Result<()> {
    let status = command
        .status()
        .with_context(|| format!("cannot start {what}"))?;
    ensure!(status.success(), "{what} exited with {status}");
    Ok(())
}

/// The `buf generate` template that writes the protobuf-es messages of the
/// descriptor set into the directory the generated sources import them from.
const PROTOC_GEN_ES_TEMPLATE: &str = r#"{"version":"v2","plugins":[{"local":"./node_modules/.bin/protoc-gen-es","out":"src/generated/proto","opt":["target=ts","import_extension=js"]}]}"#;

/// The descriptor set a build runs on: the contract read from it, and its
/// absolute path for the protobuf-es generation.
pub struct Descriptors<'a> {
    pub contract: &'a Contract,
    pub path: &'a Path,
}

/// Builds the package of `krate` into `out` with the toolchain of this
/// machine.
///
/// # Errors
///
/// A step that cannot start or does not succeed, or a SPEC the sources do not
/// satisfy, or a descriptor set that does not declare the messages the
/// exports cross as.
pub fn run_build(
    spec: &Path,
    krate: &Path,
    surface: &str,
    out: &Path,
    target_dir: &Path,
    descriptor: &Path,
) -> anyhow::Result<()> {
    let contract = Contract::read(descriptor)?;
    let descriptor = std::path::absolute(descriptor)?;
    build(
        &System,
        spec,
        krate,
        surface,
        out,
        target_dir,
        &Descriptors {
            contract: &contract,
            path: &descriptor,
        },
    )
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
    descriptors: &Descriptors,
) -> anyhow::Result<()> {
    let Descriptors { contract, path } = descriptors;
    let mut package = wrapper_ts::package_of(spec, krate, surface, out)?;
    let glue = package.crate_name.replace('-', "_");
    tools.run(&Step {
        what: "cargo build for wasm32",
        program: "cargo",
        args: ["build", "--profile", WASM_PROFILE_NAME]
            .into_iter()
            .chain(
                WASM_PROFILE
                    .into_iter()
                    .flat_map(|setting| ["--config", setting]),
            )
            .chain([
                "--target",
                "wasm32-unknown-unknown",
                "--features",
                "wasm,sdk-all",
                "--manifest-path",
            ])
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
        .join("wasm32-unknown-unknown")
        .join(WASM_PROFILE_NAME)
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
    // A missing wasm-opt is a failed build, never a skipped step: the module
    // that ships is the one the size probe built.
    let bound = out.join(SOURCE_DIR).join(format!("{glue}_bg.wasm"));
    tools.run(&Step {
        what: "wasm-opt",
        program: "wasm-opt",
        args: ["-Oz"]
            .into_iter()
            .chain(WASM_OPT_FEATURES)
            .map(OsString::from)
            .chain([
                bound.clone().into_os_string(),
                OsString::from("-o"),
                bound.clone().into_os_string(),
            ])
            .collect(),
        dir: None,
    })?;
    package.wasm_sha256 = sha256_hex(&tools.read(&bound)?);
    let model = crate::features::read_model(krate)?;
    wrapper_ts::write(out, &wrapper_ts::generate(&model, &package, contract)?)?;
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
        what: "protoc-gen-es",
        program: "./node_modules/.bin/buf",
        args: vec![
            OsString::from("generate"),
            path.as_os_str().to_owned(),
            OsString::from("--template"),
            OsString::from(PROTOC_GEN_ES_TEMPLATE),
        ],
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

    /// The bytes the recorded `wasm-opt` is taken to have written.
    const OPTIMISED: &[u8] = b"optimised module";

    /// Records every step; fails the one named `fail_on`. It reads back
    /// [`OPTIMISED`] for any path, so no step needs a real module.
    struct Recorder {
        steps: RefCell<Vec<Step>>,
        fail_on: Option<&'static str>,
        read: RefCell<Vec<PathBuf>>,
    }

    impl Recorder {
        fn new(fail_on: Option<&'static str>) -> Self {
            Self {
                steps: RefCell::default(),
                fail_on,
                read: RefCell::default(),
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

        fn read(&self, path: &Path) -> anyhow::Result<Vec<u8>> {
            self.read.borrow_mut().push(path.to_owned());
            Ok(OPTIMISED.to_vec())
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
            &Descriptors {
                contract: &wrapper_ts::testkit::contract(),
                path: Path::new("/descriptor.binpb"),
            },
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
                "wasm-opt",
                "npm ci",
                "protoc-gen-es",
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
        assert!(
            !cargo.contains(&"--release".to_owned()),
            "the generator's profile, not the consumer's release"
        );
        let profile = cargo
            .iter()
            .position(|arg| arg == "--profile")
            .expect("a profile");
        assert_eq!(cargo[profile + 1], WASM_PROFILE_NAME);
        for setting in WASM_PROFILE {
            assert!(cargo.contains(&setting.to_owned()), "{setting}");
        }
        assert!(cargo.contains(&"wasm,sdk-all".to_owned()));
        assert!(cargo.iter().any(|arg| arg.ends_with("tests/l2/Cargo.toml")));
        let bindgen = args(&steps[1]);
        assert!(bindgen.contains(&"--weak-refs".to_owned()));
        assert_eq!(
            bindgen.last().map(String::as_str),
            Some("/target/wasm32-unknown-unknown/api-bones-wasm/api_bones_test_service_l2.wasm")
        );
        let opt = args(&steps[2]);
        assert_eq!(opt[0], "-Oz");
        for feature in WASM_OPT_FEATURES {
            assert!(opt.contains(&feature.to_owned()), "{feature}");
        }
        let module = opt[opt.len() - 1].clone();
        assert!(
            module.ends_with("api_bones_test_service_l2_bg.wasm"),
            "{module}"
        );
        assert_eq!(
            opt[opt.len() - 3],
            module,
            "wasm-opt rewrites the module in place"
        );
        assert_eq!(opt[opt.len() - 2], "-o");
        assert_eq!(steps[3].dir.as_deref(), Some(out.as_path()));
        assert_eq!(
            args(&steps[3]),
            ["ci", "--no-audit", "--no-fund"].map(str::to_owned)
        );
        assert_eq!(steps[4].dir.as_deref(), Some(out.as_path()));
        assert_eq!(steps[4].program, "./node_modules/.bin/buf");
        assert_eq!(
            args(&steps[4]),
            [
                "generate",
                "/descriptor.binpb",
                "--template",
                PROTOC_GEN_ES_TEMPLATE
            ]
            .map(str::to_owned)
        );
        assert_eq!(steps[5].dir.as_deref(), Some(out.as_path()));
        assert!(out.join(SOURCE_DIR).join("client.ts").is_file());
        assert!(out.join("package.json").is_file());
        std::fs::remove_dir_all(&out).expect("cleaned");
    }

    #[test]
    fn the_protobuf_es_template_writes_into_the_directory_the_sources_import() {
        let template: serde_json::Value =
            serde_json::from_str(PROTOC_GEN_ES_TEMPLATE).expect("json");
        assert_eq!(
            template,
            serde_json::json!({
                "version": "v2",
                "plugins": [{
                    "local": "./node_modules/.bin/protoc-gen-es",
                    "out": format!("{SOURCE_DIR}/{}", wrapper_ts::PROTO_DIR),
                    "opt": ["target=ts", "import_extension=js"]
                }]
            })
        );
    }

    #[test]
    fn a_failing_step_stops_the_build() {
        for (failing, ran) in [
            ("cargo build for wasm32", 1),
            ("wasm-bindgen", 2),
            ("wasm-opt", 3),
            ("npm ci", 4),
            ("protoc-gen-es", 5),
            ("npm run build", 6),
        ] {
            let out = scratch("failing");
            with_lockfile(&out);
            let tools = Recorder::new(Some(failing));
            let error = build_with(&tools, "sdk-call", &out).expect_err(failing);
            assert_eq!(error.to_string(), format!("{failing} failed"));
            assert_eq!(tools.steps.borrow().len(), ran, "{failing}");
            assert_eq!(
                out.join("package.json").exists(),
                ran >= 4,
                "{failing}: the sources are written after wasm-opt and before npm ci"
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
        assert_eq!(tools.steps.borrow().len(), 3);
        assert!(out.join("package.json").is_file());
        std::fs::remove_dir_all(&out).expect("cleaned");
    }

    #[test]
    fn the_size_probe_builds_under_the_generators_profile() {
        let script = std::fs::read_to_string(repo().join("tools/wasm-size-probe/build-report.sh"))
            .expect("the probe script");
        for setting in WASM_PROFILE {
            assert!(script.contains(setting), "the probe script lacks {setting}");
        }
        for feature in WASM_OPT_FEATURES {
            assert!(script.contains(feature), "the probe script lacks {feature}");
        }
        assert!(
            !std::fs::read_to_string(repo().join("Cargo.toml"))
                .expect("the root manifest")
                .contains("[profile.wasm-size"),
            "the generator owns the wasm profile; the workspace declares none"
        );
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
            &wrapper_ts::testkit::descriptor_file("absent-surface"),
        )
        .expect_err("no such surface");
        assert!(!out.exists());
    }

    #[test]
    fn a_missing_descriptor_set_fails_the_build_before_any_tool() {
        let out = scratch("no-descriptor");
        let error = run_build(
            &repo().join("SPEC.md"),
            &repo().join("tests/l2"),
            "sdk-call",
            &out,
            Path::new("/target"),
            Path::new("/no/such/descriptor.binpb"),
        )
        .expect_err("no descriptor set");
        assert!(error.to_string().contains("descriptor.binpb"), "{error}");
        assert!(!out.exists());
    }

    #[test]
    fn the_stamp_is_the_digest_of_the_module_wasm_opt_left() {
        let out = scratch("stamp");
        with_lockfile(&out);
        let tools = Recorder::new(None);
        build_with(&tools, "sdk-call", &out).expect("the build");
        assert_eq!(
            tools.read.borrow().as_slice(),
            [out.join(SOURCE_DIR)
                .join("api_bones_test_service_l2_bg.wasm")],
            "the module is read once, after wasm-opt, from where wasm-opt wrote it"
        );
        let init = std::fs::read_to_string(out.join(SOURCE_DIR).join("init.ts")).expect("init.ts");
        assert!(
            init.contains(&format!(
                "const WASM_SHA256 = \"{}\";",
                sha256_hex(OPTIMISED)
            )),
            "{init}"
        );
        std::fs::remove_dir_all(&out).expect("cleaned");
    }

    #[test]
    fn a_digest_is_lowercase_hex_of_sha256() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn the_system_reads_a_written_file_and_names_a_missing_one() {
        let dir = scratch("read");
        std::fs::create_dir_all(&dir).expect("a directory");
        std::fs::write(dir.join("m.wasm"), b"bytes").expect("a module");
        assert_eq!(System.read(&dir.join("m.wasm")).expect("read"), b"bytes");
        let error = System.read(&dir.join("absent.wasm")).expect_err("absent");
        assert!(error.to_string().contains("absent.wasm"), "{error}");
        std::fs::remove_dir_all(&dir).expect("cleaned");
    }
}
