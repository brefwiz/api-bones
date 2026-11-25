// SPDX-License-Identifier: MIT
//! `wrapper size`: the size of a wasm module, as the `api-bones.wasm-size/v1`
//! document. It measures; it gates nothing.

use std::path::Path;

use anyhow::Context;

/// The schema id of the document.
pub const SCHEMA: &str = "api-bones.wasm-size/v1";

/// The document for the module at `wasm`, written as two-space JSON with a
/// final newline.
///
/// # Errors
///
/// The module cannot be read.
pub fn document(wasm: &Path, crate_name: &str) -> anyhow::Result<String> {
    let bytes = std::fs::metadata(wasm)
        .with_context(|| format!("cannot read {}", wasm.display()))?
        .len();
    let mut text = serde_json::to_string_pretty(&serde_json::json!({
        "schema": SCHEMA,
        "crate": crate_name,
        "bytes": bytes,
    }))?;
    text.push('\n');
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_document_states_the_schema_crate_and_byte_count() {
        let path = std::env::temp_dir().join(format!("wasm-size-{}.wasm", std::process::id()));
        std::fs::write(&path, [0u8; 7]).expect("a file");
        let text = document(&path, "numbers").expect("a document");
        std::fs::remove_file(&path).expect("cleaned");
        assert_eq!(
            text,
            "{\n  \"bytes\": 7,\n  \"crate\": \"numbers\",\n  \"schema\": \"api-bones.wasm-size/v1\"\n}\n"
        );
    }

    #[test]
    fn a_missing_module_is_refused() {
        assert!(document(Path::new("/nonexistent/x.wasm"), "n").is_err());
    }
}
