// SPDX-License-Identifier: MIT
//! `wrapper size`: what an SDK package ships, as the `api-bones.sdk-size/v1`
//! document: the raw, gzip and brotli bytes of the wasm module, of its
//! wasm-bindgen glue and of the generated TypeScript. It measures; it gates
//! nothing.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, ensure};
use serde_json::{Value, json};

/// The schema id of the document.
pub const SCHEMA: &str = "api-bones.sdk-size/v1";

/// The brotli quality the report measures at: the highest, as a static host
/// would serve it.
const BROTLI_QUALITY: i32 = 11;

/// One artefact as shipped: its raw size and its size under each encoding a
/// host serves it in.
fn sizes(bytes: &[u8]) -> anyhow::Result<Value> {
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    gzip.write_all(bytes).context("gzip failed")?;
    let gzip = gzip.finish().context("gzip failed")?;
    let mut brotli = Vec::new();
    let params = brotli::enc::BrotliEncoderParams {
        quality: BROTLI_QUALITY,
        ..brotli::enc::BrotliEncoderParams::default()
    };
    brotli::BrotliCompress(&mut &bytes[..], &mut brotli, &params).context("brotli failed")?;
    Ok(json!({
        "raw": bytes.len(),
        "gzip": gzip.len(),
        "brotli": brotli.len(),
    }))
}

fn read(path: &Path) -> anyhow::Result<Vec<u8>> {
    std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))
}

/// Every `.ts` file under `dir`, in path order.
fn typescript_files(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_owned()];
    while let Some(next) = pending.pop() {
        let entries =
            std::fs::read_dir(&next).with_context(|| format!("cannot read {}", next.display()))?;
        for entry in entries {
            let path = entry?.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|ext| ext == "ts") {
                found.push(path);
            }
        }
    }
    found.sort();
    Ok(found)
}

/// The document for the module at `wasm`, its `glue` and the TypeScript under
/// `typescript`, written as two-space JSON with a final newline.
///
/// # Errors
///
/// A file cannot be read, a directory holds no TypeScript, or an encoder
/// fails.
pub fn document(
    wasm: &Path,
    glue: &Path,
    typescript: &Path,
    crate_name: &str,
) -> anyhow::Result<String> {
    let files = typescript_files(typescript)?;
    ensure!(
        !files.is_empty(),
        "{} holds no generated TypeScript",
        typescript.display()
    );
    let mut generated = Vec::new();
    for file in &files {
        generated.extend(read(file)?);
    }
    let mut text = serde_json::to_string_pretty(&json!({
        "schema": SCHEMA,
        "crate": crate_name,
        "wasm": sizes(&read(wasm)?)?,
        "glue": sizes(&read(glue)?)?,
        "typescript": sizes(&generated)?,
    }))?;
    text.push('\n');
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sdk-size-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src/nested")).expect("a directory");
        dir
    }

    #[test]
    fn sizes_state_the_raw_gzip_and_brotli_bytes() {
        let value = sizes(&[7u8; 64]).expect("sizes");
        assert_eq!(value["raw"], 64);
        assert!(value["gzip"].as_u64().is_some_and(|n| n > 0 && n < 64));
        assert!(value["brotli"].as_u64().is_some_and(|n| n > 0 && n < 64));
    }

    #[test]
    fn the_document_states_the_schema_crate_and_every_artefact() {
        let dir = scratch("document");
        std::fs::write(dir.join("m.wasm"), [0u8; 7]).expect("a module");
        std::fs::write(dir.join("g.js"), "export {};\n").expect("a glue");
        std::fs::write(dir.join("src/a.ts"), "export const a = 1;\n").expect("a source");
        std::fs::write(dir.join("src/nested/b.ts"), "export const b = 2;\n").expect("a source");
        std::fs::write(dir.join("src/ignored.json"), "{}").expect("not typescript");
        let text = document(
            &dir.join("m.wasm"),
            &dir.join("g.js"),
            &dir.join("src"),
            "numbers",
        )
        .expect("a document");
        let value: Value = serde_json::from_str(&text).expect("json");
        assert!(text.ends_with("}\n"));
        assert_eq!(value["schema"], SCHEMA);
        assert_eq!(value["crate"], "numbers");
        assert_eq!(value["wasm"]["raw"], 7);
        assert_eq!(value["glue"]["raw"], 11);
        assert_eq!(value["typescript"]["raw"], 40);
        std::fs::remove_dir_all(&dir).expect("cleaned");
    }

    #[test]
    fn a_missing_artefact_is_refused() {
        let dir = scratch("missing");
        std::fs::write(dir.join("src/a.ts"), "x").expect("a source");
        let missing = |wasm: &str, glue: &str, ts: &str| {
            document(&dir.join(wasm), &dir.join(glue), &dir.join(ts), "n")
        };
        std::fs::write(dir.join("g.js"), "x").expect("a glue");
        assert!(missing("absent.wasm", "g.js", "src").is_err());
        std::fs::write(dir.join("m.wasm"), "x").expect("a module");
        assert!(missing("m.wasm", "absent.js", "src").is_err());
        assert!(missing("m.wasm", "g.js", "absent").is_err());
        std::fs::remove_dir_all(&dir).expect("cleaned");
    }

    #[test]
    fn a_directory_without_typescript_is_refused() {
        let dir = scratch("empty");
        std::fs::write(dir.join("m.wasm"), "x").expect("a module");
        std::fs::write(dir.join("g.js"), "x").expect("a glue");
        let error = document(
            &dir.join("m.wasm"),
            &dir.join("g.js"),
            &dir.join("src"),
            "n",
        )
        .expect_err("no typescript");
        assert!(
            error.to_string().contains("no generated TypeScript"),
            "{error}"
        );
        std::fs::remove_dir_all(&dir).expect("cleaned");
    }
}
