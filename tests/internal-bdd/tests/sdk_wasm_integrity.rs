// SPDX-License-Identifier: MIT
//! Runs `features/sdk_wasm_integrity.feature`.
//!
//! The wasm load check and the boundary stamp of a generated TypeScript wrapper
//! fault only when the built artifact itself is altered, which no SDK call can
//! do (SPEC.md `internal_behavior_owners`). Each scenario alters a copy of the
//! built `api-bones-test-service-sdk-ts` package and opens it with
//! `tests/node/open_sdk.mjs`.

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use cucumber::{World, given, then, when};
use serde_json::Value;

const PACKAGE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../api-bones-test-service-sdk-ts"
);
const OPEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/node/open_sdk.mjs");
const SCRATCH: &str = env!("CARGO_TARGET_TMPDIR");
/// The refusal comes before any request leaves the SDK, so no server answers here.
const BASE_URL: &str = "http://127.0.0.1:1";

static NEXT: AtomicU32 = AtomicU32::new(0);

/// A directory of one scenario, removed when the scenario's world is dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let path = Path::new(SCRATCH).join(format!(
            "sdk-wasm-integrity-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("create the scenario directory");
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// How the SDK of the altered copy answered the caller.
struct Outcome {
    opened: bool,
    code: String,
    text: String,
    message: String,
}

#[derive(Default, World)]
struct IntegrityWorld {
    caller: Option<(String, String)>,
    evidence: Option<Scratch>,
    copy: Option<Scratch>,
    outcome: Option<Outcome>,
}

impl std::fmt::Debug for IntegrityWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IntegrityWorld").finish()
    }
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("create the copy directory");
    for entry in fs::read_dir(from).expect("read the package directory") {
        let entry = entry.expect("read a package entry");
        if entry.file_name() == "node_modules" {
            continue;
        }
        let target = to.join(entry.file_name());
        if entry.file_type().expect("read an entry type").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).expect("copy a package file");
        }
    }
}

impl IntegrityWorld {
    /// Copies the built wrapper package into a scenario directory; its own
    /// dependencies resolve from where the built package finds them.
    fn copy_built(&mut self) -> PathBuf {
        let package = Path::new(PACKAGE);
        assert!(
            package.join("dist/generated-l2/index.js").exists(),
            "the test-service wrapper is not built: build it with `wrapper build-ts` first"
        );
        let copy = Scratch::new("copy");
        copy_tree(package, &copy.0);
        let modules = package.join("node_modules");
        if modules.exists() {
            symlink(
                fs::canonicalize(modules).expect("resolve node_modules"),
                copy.0.join("node_modules"),
            )
            .expect("link node_modules");
        }
        self.copy = Some(copy);
        self.copy
            .as_ref()
            .expect("the copy was just made")
            .0
            .join("dist/generated-l2")
    }

    /// Opens the SDK of the altered copy from the caller's session.
    fn open(&mut self, dist: &Path) {
        let (subject, org) = self.caller.clone().expect("a Given named the caller");
        let mut node = Command::new("node");
        node.arg(OPEN)
            .arg(dist.join("index.js"))
            .arg(BASE_URL)
            .arg(subject)
            .arg(org);
        match &self.evidence {
            Some(evidence) => node.env("BREFWIZ_SDK_EVIDENCE", &evidence.0),
            None => node.env_remove("BREFWIZ_SDK_EVIDENCE"),
        };
        let run = node.output().expect("run node");
        let stdout = String::from_utf8_lossy(&run.stdout);
        assert!(
            run.status.success(),
            "node failed: {} {stdout}",
            String::from_utf8_lossy(&run.stderr)
        );
        let line = stdout.lines().last().expect("node printed the outcome");
        let json: Value = serde_json::from_str(line).expect("the outcome is JSON");
        let field = |name: &str| json[name].as_str().unwrap_or_default().to_owned();
        self.outcome = Some(Outcome {
            opened: json["opened"]
                .as_bool()
                .expect("the outcome says if it opened"),
            code: field("code"),
            text: format!("{} {}: {}", field("name"), field("code"), field("message")),
            message: field("message"),
        });
    }

    fn refusal(&self) -> &Outcome {
        let outcome = self.outcome.as_ref().expect("a When opened the SDK");
        assert!(!outcome.opened, "the SDK opened; it was to be refused");
        outcome
    }
}

#[given(expr = "the SDK caller is {string} of organization {string}")]
fn given_caller(world: &mut IntegrityWorld, subject: String, org: String) {
    world.caller = Some((subject, org));
}

#[given(expr = "the run collects SDK evidence in a fresh directory")]
fn given_evidence(world: &mut IntegrityWorld) {
    world.evidence = Some(Scratch::new("evidence"));
}

#[when(expr = "the caller opens an SDK whose wasm module has one byte flipped")]
fn when_flipped(world: &mut IntegrityWorld) {
    let dist = world.copy_built();
    let wasm = fs::read_dir(&dist)
        .expect("read the wrapper's dist")
        .map(|entry| entry.expect("read a dist entry").path())
        .find(|path| path.to_string_lossy().ends_with("_bg.wasm"))
        .expect("the wrapper carries a wasm module");
    let mut bytes = fs::read(&wasm).expect("read the wasm module");
    *bytes.last_mut().expect("the wasm module has bytes") ^= 0xff;
    fs::write(&wasm, bytes).expect("write the altered wasm module");
    world.open(&dist);
}

#[when(expr = "the caller opens an SDK whose wrapper speaks boundary version {int}")]
fn when_boundary(world: &mut IntegrityWorld, version: u32) {
    let dist = world.copy_built();
    let init = dist.join("init.js");
    let source = fs::read_to_string(&init).expect("read the wrapper's init.js");
    let stamped = "WRAPPER_BOUNDARY = 1";
    assert!(
        source.contains(stamped),
        "the wrapper's init.js does not carry {stamped}"
    );
    fs::write(
        &init,
        source.replace(stamped, &format!("WRAPPER_BOUNDARY = {version}")),
    )
    .expect("write the altered init.js");
    world.open(&dist);
}

#[then(expr = "the refusal says {string}")]
fn then_refusal_says(world: &mut IntegrityWorld, text: String) {
    let refusal = world.refusal();
    assert!(
        refusal.message.contains(&text),
        "refused with {}",
        refusal.text
    );
}

#[then("no SDK provenance was recorded")]
fn then_no_provenance(world: &mut IntegrityWorld) {
    let evidence = world.evidence.as_ref().expect("a Given named the evidence");
    let recorded = fs::read_to_string(evidence.0.join("sdk-provenance.jsonl")).unwrap_or_default();
    assert_eq!(recorded, "");
}

#[then(expr = "the call is refused with the code {string}")]
fn then_refused_with_code(world: &mut IntegrityWorld, code: String) {
    let refusal = world.refusal();
    assert_eq!(refusal.code, code, "refused with {}", refusal.text);
}

#[then("the refusal names the core, wrapper and runtime versions")]
fn then_names_versions(world: &mut IntegrityWorld) {
    let text = world.refusal().text.to_lowercase();
    for part in ["core", "wrapper", "runtime"] {
        assert!(
            text.contains(part),
            "the refusal does not name the {part} version: {text}"
        );
    }
}

#[tokio::main]
async fn main() {
    let features = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/features/sdk_wasm_integrity.feature"
    );
    IntegrityWorld::run(features).await;
}
