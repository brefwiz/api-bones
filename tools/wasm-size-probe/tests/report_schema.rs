// SPDX-License-Identifier: MIT
//! `api-bones.wasm-size/v1`: what a report must hold and what it derives.

use api_bones_wasm_size_probe::report::{
    LOGIN_PAGE_CEILING, Meta, REQUIRED_VARIANTS, Report, SCHEMA, Variant, derive, run,
};

fn variant(name: &str, size: u64) -> Variant {
    Variant {
        name: name.to_owned(),
        features: vec![name.to_owned()],
        profile: "wasm-size".to_owned(),
        wasm_opt: name != "floor-no-opt",
        raw: size,
        gzip: size / 3,
        brotli: size / 4,
        glue_gzip: 1_000,
    }
}

fn variants() -> Vec<Variant> {
    let size = |name: &str| match name {
        "floor" | "floor-no-opt" => 100_000,
        "methods-10" => 110_000,
        "methods-50" => 150_000,
        "second-service" => 104_000,
        "all" => 200_000,
        _ => 120_000,
    };
    REQUIRED_VARIANTS
        .iter()
        .map(|name| variant(name, size(name)))
        .collect()
}

fn report() -> Report {
    Report::new(
        "abc123",
        "rustc 1.90.0",
        "0.2.128",
        "version_119",
        variants(),
    )
    .expect("a complete variant set makes a report")
}

#[test]
fn report_declares_its_schema_and_provenance() {
    let json = serde_json::to_value(report()).unwrap();
    assert_eq!(json["schema"], SCHEMA);
    for field in ["commit", "rustc", "wasm_bindgen", "binaryen"] {
        assert!(json[field].as_str().is_some_and(|value| !value.is_empty()));
    }
    let first = &json["variants"][0];
    for field in [
        "name",
        "features",
        "profile",
        "wasm_opt",
        "raw",
        "gzip",
        "brotli",
        "glue_gzip",
    ] {
        assert!(first.get(field).is_some(), "variant field {field}");
    }
}

#[test]
fn derived_costs_follow_the_variants() {
    let derived = report().derived;
    assert_eq!(derived.floor.raw, 100_000);
    assert_eq!(derived.deltas["page"].raw, 20_000);
    assert_eq!(derived.per_method.raw, 1_000);
    assert_eq!(derived.second_service.raw, 4_000);
    assert_eq!(derived.ceiling, LOGIN_PAGE_CEILING);
    assert_eq!(derived.options.len(), 3);
    let a = &derived.options["A"];
    assert_eq!(a.headroom_gzip, 149_261 - a.size.gzip);
    assert!(derived.options["B"].size.raw > a.size.raw);
}

#[test]
fn a_missing_variant_is_refused() {
    let mut set = variants();
    set.retain(|variant| variant.name != "stream");
    assert!(derive(&set).is_err());
    assert!(Report::new("c", "r", "w", "b", set).is_err());
}

#[test]
fn an_invalid_report_is_refused() {
    let mut bad = report();
    bad.schema = "other".to_owned();
    assert!(bad.validate().is_err());

    let mut bad = report();
    bad.commit = " ".to_owned();
    assert!(bad.validate().is_err());

    let mut bad = report();
    bad.variants[0].raw = 0;
    assert!(bad.validate().is_err());

    let mut bad = report();
    bad.variants[0].wasm_opt = false;
    assert!(bad.validate().is_err());

    let mut bad = report();
    bad.derived.per_method.raw += 1;
    assert!(bad.validate().is_err());
}

#[test]
fn markdown_lists_every_variant_and_option() {
    let markdown = report().markdown();
    for name in REQUIRED_VARIANTS {
        assert!(markdown.contains(&format!("| {name} |")), "{name}");
    }
    for option in ["| A |", "| B |", "| C |"] {
        assert!(markdown.contains(option));
    }
    assert!(markdown.contains("149261"));
}

#[test]
fn measure_compresses_the_module() {
    let meta = Meta {
        features: vec!["floor".to_owned()],
        profile: "wasm-size".to_owned(),
        wasm_opt: true,
    };
    let wasm = vec![7_u8; 4096];
    let measured = Variant::measure("floor", &meta, &wasm, b"glue glue glue").unwrap();
    assert_eq!(measured.raw, 4096);
    assert!(measured.gzip > 0 && measured.gzip < measured.raw);
    assert!(measured.brotli > 0 && measured.brotli < measured.raw);
    assert!(measured.glue_gzip > 0);
}

fn args(pairs: &[(&str, &str)]) -> Vec<String> {
    pairs
        .iter()
        .flat_map(|(flag, value)| [format!("--{flag}"), (*value).to_owned()])
        .collect()
}

/// Writes one directory per variant under `root/variants`, each holding its
/// meta, module and glue, and returns that directory.
fn write_variants(root: &std::path::Path) -> std::path::PathBuf {
    let dir = root.join("variants");
    for variant in variants() {
        let path = dir.join(&variant.name);
        std::fs::create_dir_all(&path).unwrap();
        let meta = Meta {
            features: variant.features.clone(),
            profile: variant.profile.clone(),
            wasm_opt: variant.wasm_opt,
        };
        std::fs::write(path.join("meta.json"), serde_json::to_vec(&meta).unwrap()).unwrap();
        let body: Vec<u8> = (0..variant.raw / 50).map(|n| (n % 251) as u8).collect();
        std::fs::write(path.join("module.wasm"), body).unwrap();
        std::fs::write(path.join("glue.js"), b"export default 1;").unwrap();
    }
    dir
}

#[test]
fn run_writes_report_and_summary_from_variant_directories() {
    let root = std::env::temp_dir().join(format!("wasm-size-run-{}", std::process::id()));
    let dir = write_variants(&root);
    let out = root.join("report.json");
    let summary = root.join("summary.md");
    let dir_arg = dir.to_string_lossy().into_owned();
    let out_arg = out.to_string_lossy().into_owned();
    let summary_arg = summary.to_string_lossy().into_owned();
    let full = [
        ("dir", dir_arg.as_str()),
        ("commit", "abc"),
        ("rustc", "rustc 1.90.0"),
        ("wasm-bindgen", "0.2.128"),
        ("binaryen", "version_119"),
        ("out", out_arg.as_str()),
        ("summary", summary_arg.as_str()),
    ];
    let markdown = run(&args(&full)).expect("run succeeds");
    assert!(markdown.contains("wasm size probe"));
    let written: Report = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
    written.validate().unwrap();
    assert_eq!(std::fs::read_to_string(&summary).unwrap(), markdown);

    assert!(run(&args(&full[1..])).is_err(), "a missing flag is refused");
    assert!(run(&["--bogus".to_owned()]).is_err());
    assert!(run(&["--dir".to_owned()]).is_err());

    let empty = root.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let empty_arg = empty.to_string_lossy().into_owned();
    let mut none = full;
    none[0] = ("dir", empty_arg.as_str());
    assert!(
        run(&args(&none)).is_err(),
        "an empty variant set is refused"
    );

    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn lib_run_writes_markdown_to_the_sink_and_reports_errors() {
    let mut sink = Vec::new();
    let error = api_bones_wasm_size_probe::run(&["--bogus".to_owned()], &mut sink).unwrap_err();
    assert!(error.to_string().contains("unknown argument"));
    assert_eq!(sink.len(), 0);

    let root = std::env::temp_dir().join(format!("wasm-size-lib-run-{}", std::process::id()));
    let dir = write_variants(&root);
    let out = root.join("report.json");
    let summary = root.join("summary.md");
    let (dir, out, summary) = (
        dir.to_string_lossy().into_owned(),
        out.to_string_lossy().into_owned(),
        summary.to_string_lossy().into_owned(),
    );
    let full = args(&[
        ("dir", dir.as_str()),
        ("commit", "abc"),
        ("rustc", "rustc 1.90.0"),
        ("wasm-bindgen", "0.2.128"),
        ("binaryen", "version_119"),
        ("out", out.as_str()),
        ("summary", summary.as_str()),
    ]);
    api_bones_wasm_size_probe::run(&full, &mut sink).expect("run succeeds");
    assert!(String::from_utf8(sink).unwrap().contains("wasm size probe"));
    let ok = api_bones_wasm_size_probe::exit_code(&full);
    assert_eq!(
        format!("{ok:?}"),
        format!("{:?}", std::process::ExitCode::SUCCESS)
    );
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn exit_code_is_failure_on_a_bad_argument() {
    let code = api_bones_wasm_size_probe::exit_code(&["--bogus".to_owned()]);
    assert_eq!(
        format!("{code:?}"),
        format!("{:?}", std::process::ExitCode::FAILURE)
    );
}
