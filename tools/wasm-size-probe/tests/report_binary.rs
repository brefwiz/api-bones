// SPDX-License-Identifier: MIT
//! The `wasm-size-report` binary end to end: a complete variant set writes the
//! report and summary, a bad invocation exits non-zero with a message.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use api_bones_wasm_size_probe::report::{REQUIRED_VARIANTS, SCHEMA};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wasm-size-report-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("a directory");
    dir
}

fn size(name: &str) -> usize {
    match name {
        "floor" | "floor-no-opt" => 100_000,
        "methods-10" => 110_000,
        "methods-50" => 150_000,
        "second-service" => 104_000,
        "all" => 200_000,
        _ => 120_000,
    }
}

fn write_variants(dir: &Path) {
    for name in REQUIRED_VARIANTS {
        let root = dir.join(name);
        fs::create_dir_all(&root).expect("a variant directory");
        let meta = format!(
            "{{\"features\":[\"{name}\"],\"profile\":\"wasm-size\",\"wasm_opt\":{}}}",
            name != "floor-no-opt"
        );
        fs::write(root.join("meta.json"), meta).expect("a meta");
        let module: Vec<u8> = (0..size(name))
            .map(|i| u8::try_from(i % 251).unwrap_or(0))
            .collect();
        fs::write(root.join("module.wasm"), module).expect("a module");
        fs::write(root.join("glue.js"), "export function glue() {}\n").expect("a glue");
    }
}

fn report(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_wasm-size-report"))
        .args(args)
        .output()
        .expect("the report binary runs")
}

#[test]
fn a_complete_variant_set_writes_the_report_and_the_summary() {
    let dir = scratch("ok");
    let variants = dir.join("variants");
    write_variants(&variants);
    let out = dir.join("report.json");
    let summary = dir.join("summary.md");
    let output = report(&[
        "--dir",
        variants.to_str().unwrap(),
        "--commit",
        "abc123",
        "--rustc",
        "rustc 1.90.0",
        "--wasm-bindgen",
        "0.2.128",
        "--binaryen",
        "version_119",
        "--out",
        out.to_str().unwrap(),
        "--summary",
        summary.to_str().unwrap(),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(&format!(
            "### wasm size probe ({SCHEMA})\n\ncommit `abc123`"
        )),
        "{stdout}"
    );
    let json: serde_json::Value =
        serde_json::from_slice(&fs::read(&out).expect("a report")).expect("json");
    assert_eq!(json["schema"], SCHEMA);
    assert_eq!(
        json["variants"].as_array().map(Vec::len),
        Some(REQUIRED_VARIANTS.len())
    );
    assert_eq!(
        fs::read_to_string(&summary).expect("a summary").trim_end(),
        stdout.trim_end()
    );
}

#[test]
fn an_unknown_flag_exits_non_zero_and_says_so() {
    let output = report(&["--nope", "x"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("wasm-size-report: unknown argument `--nope`"),
        "{stderr}"
    );
    assert_eq!(output.stdout.len(), 0);
}
